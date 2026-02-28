//! `MLRunX` API Server
//!
//! This is the monolith API server that handles:
//! - HTTP REST API for queries and SDK HTTP transport
//! - gRPC API for high-throughput SDK ingestion
//!
//! Architecture: Single binary serving both protocols on different ports.
//!
//! # Ingest Modes
//!
//! - **Direct mode** (alpha): Writes directly to ClickHouse/Postgres without queues.
//!   Simpler setup, suitable for development and small deployments.
//!
//! - **Queued mode** (future): Writes through Redis/Kafka for better throughput
//!   and horizontal scaling.

#![allow(
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::redundant_pub_crate,
    clippy::future_not_send,
    clippy::significant_drop_tightening,
    clippy::option_if_let_else,
    dead_code,
    unused_imports
)] // Targeted lint exceptions — keep this list minimal.

mod auth;
mod config;
mod services;
mod storage;

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::{
    Extension, Json, Router,
    extract::State,
    http::{
        HeaderMap, HeaderValue, Method, StatusCode, Uri,
        header::{self, HeaderName},
    },
    middleware,
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use tonic::transport::Server as TonicServer;
use tower_http::{cors::CorsLayer, decompression::RequestDecompressionLayer};
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use auth::{ApiKeyStore, AuthContext, AuthMode, auth_middleware};
use mlrunx_api_http_types::{
    UiAuthLoginRequest, UiAuthLoginResponse, UiAuthLogoutResponse, UiAuthSessionResponse,
};
use mlrunx_api_policy::{UiRunOwnerPolicyError, enforce_ui_run_owner};
use mlrunx_proto::mlrunx::v1::ingest_service_server::IngestServiceServer;
use services::{
    CardinalityTracker, EventPayload, IdempotencyResult, IdempotencyStore, IngestServiceImpl,
    MetricPayload, ParamPayload, TagPayload, compute_payload_hash, ingest::InMemoryStore,
};
use storage::{
    AuditEventRow, AuthSessionAdminRow, CreateProjectInput, CreateRunInput, MetadataFilter,
    MetricRow, ProjectRepository, ProjectRow, RunEventInput, RunEventRow, RunFilterCondition,
    RunFilterExpr, RunFilterOperator, RunFilterTarget, RunListSortField, RunListSortOrder,
    RunRepository, RunRow, RunStatus as PostgresRunStatus, SqliteStore, UserProjectMembershipRow,
    UserRow,
};

/// Application state shared across handlers.
#[derive(Clone)]
pub struct AppState {
    store: Arc<InMemoryStore>,
    sqlite_store: Arc<SqliteStore>,
    key_store: Arc<ApiKeyStore>,
    idempotency_store: Arc<IdempotencyStore>,
    cardinality_tracker: Arc<CardinalityTracker>,
}

#[derive(Debug, Clone, Copy)]
enum EndpointRbacTier {
    Read,
    Write,
    Admin,
}

impl EndpointRbacTier {
    const fn scope(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Admin => "admin",
        }
    }

    const fn env_flag_name(self) -> &'static str {
        match self {
            Self::Read => "MLRUNX_RBAC_READ_ENFORCEMENT_ENABLED",
            Self::Write => "MLRUNX_RBAC_WRITE_ENFORCEMENT_ENABLED",
            Self::Admin => "MLRUNX_RBAC_ADMIN_ENFORCEMENT_ENABLED",
        }
    }
}

// =============================================================================
// HTTP Handlers
// =============================================================================

async fn health() -> &'static str {
    "ok"
}

async fn root() -> &'static str {
    "MLRunX API v0.1.0"
}

fn is_production_environment() -> bool {
    let raw = std::env::var("MLRUNX_ENVIRONMENT")
        .or_else(|_| std::env::var("APP_ENV"))
        .or_else(|_| std::env::var("ENVIRONMENT"))
        .or_else(|_| std::env::var("RUST_ENV"))
        .unwrap_or_else(|_| "development".to_string());

    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "prod" | "production"
    )
}

fn env_flag_default(name: &str, default_value: bool) -> bool {
    std::env::var(name).map_or(default_value, |v| {
        v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes")
    })
}

fn trust_proxy_headers() -> bool {
    env_flag_default("MLRUNX_TRUST_PROXY_HEADERS", false)
}

fn allow_insecure_local_dev() -> bool {
    env_flag_default("MLRUNX_ALLOW_INSECURE_LOCAL_DEV", false)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrustedProxyRule {
    V4 { network: u32, prefix_len: u8 },
    V6 { network: u128, prefix_len: u8 },
}

impl TrustedProxyRule {
    fn parse(raw: &str) -> Result<Self, String> {
        let value = raw.trim();
        if value.is_empty() {
            return Err("empty trusted proxy entry".to_string());
        }

        let (ip, prefix_len) = if let Some((ip_raw, prefix_raw)) = value.split_once('/') {
            let ip = ip_raw
                .trim()
                .parse::<IpAddr>()
                .map_err(|_| format!("invalid IP '{ip_raw}'"))?;
            let prefix_len = prefix_raw
                .trim()
                .parse::<u8>()
                .map_err(|_| format!("invalid prefix length '{prefix_raw}'"))?;
            (ip, prefix_len)
        } else {
            let ip = value
                .parse::<IpAddr>()
                .map_err(|_| format!("invalid IP '{value}'"))?;
            let full_len = match ip {
                IpAddr::V4(_) => 32,
                IpAddr::V6(_) => 128,
            };
            (ip, full_len)
        };

        match ip {
            IpAddr::V4(ipv4) => {
                if prefix_len > 32 {
                    return Err(format!("invalid IPv4 prefix length '{prefix_len}'"));
                }
                let mask = if prefix_len == 0 {
                    0
                } else {
                    u32::MAX << (32 - u32::from(prefix_len))
                };
                Ok(Self::V4 {
                    network: u32::from(ipv4) & mask,
                    prefix_len,
                })
            }
            IpAddr::V6(ipv6) => {
                if prefix_len > 128 {
                    return Err(format!("invalid IPv6 prefix length '{prefix_len}'"));
                }
                let mask = if prefix_len == 0 {
                    0
                } else {
                    u128::MAX << (128 - u32::from(prefix_len))
                };
                Ok(Self::V6 {
                    network: u128::from(ipv6) & mask,
                    prefix_len,
                })
            }
        }
    }

    fn matches(self, ip: IpAddr) -> bool {
        match (self, ip) {
            (
                Self::V4 {
                    network,
                    prefix_len,
                },
                IpAddr::V4(value),
            ) => {
                let mask = if prefix_len == 0 {
                    0
                } else {
                    u32::MAX << (32 - u32::from(prefix_len))
                };
                (u32::from(value) & mask) == network
            }
            (
                Self::V6 {
                    network,
                    prefix_len,
                },
                IpAddr::V6(value),
            ) => {
                let mask = if prefix_len == 0 {
                    0
                } else {
                    u128::MAX << (128 - u32::from(prefix_len))
                };
                (u128::from(value) & mask) == network
            }
            _ => false,
        }
    }
}

fn trusted_proxy_rules_from_env() -> Result<Vec<TrustedProxyRule>, String> {
    let raw = std::env::var("MLRUNX_TRUSTED_PROXY_CIDRS").unwrap_or_default();
    let mut rules = Vec::new();

    for entry in raw
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let rule = TrustedProxyRule::parse(entry)
            .map_err(|err| format!("Invalid MLRUNX_TRUSTED_PROXY_CIDRS entry '{entry}': {err}"))?;
        rules.push(rule);
    }

    Ok(rules)
}

/// Convert a storage error into a safe HTTP 500 response.
/// Logs the full error server-side but returns a generic message to the client.
fn internal_error(e: impl std::fmt::Display) -> (StatusCode, String) {
    warn!("Internal error: {e}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "Internal server error".to_string(),
    )
}

/// Validate that a path parameter looks like a safe identifier.
/// Accepts UUIDs, ULIDs, and short alphanumeric IDs up to 128 chars.
fn validate_path_id(id: &str, name: &str) -> Result<(), (StatusCode, String)> {
    if id.is_empty() || id.len() > 128 {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Invalid {name}: must be 1-128 characters"),
        ));
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Invalid {name}: only alphanumeric, dash, and underscore allowed"),
        ));
    }
    Ok(())
}

const ALLOWED_METADATA_NAMESPACES: &[&str] = &["parameters", "tags", "system", "dataset", "model"];
const METADATA_MAX_KEY_LEN: usize = 256;
const METADATA_MAX_DEPTH: usize = 4;
const METADATA_MAX_SEGMENT_LEN: usize = 64;
const PARAM_VALUE_MAX_LEN: usize = 4096;
const METRIC_ARRAY_MAX_LEN: usize = 1024;
const RUN_EVENT_MAX_LEN: usize = 16_000;
const STRUCTURED_JSON_MAX_DEPTH: usize = 8;
const STRUCTURED_JSON_MAX_STRING_LEN: usize = 2048;
const CHART_DATA_MAX_BYTES: usize = 32_000;
const CHART_LAYOUT_MAX_BYTES: usize = 12_000;
const CHART_OPTIONS_MAX_BYTES: usize = 12_000;
const CHART_METADATA_MAX_BYTES: usize = 8_000;
const IMAGE_METADATA_MAX_BYTES: usize = 8_000;

fn canonicalize_metadata_key(raw_key: &str, default_namespace: &str) -> Result<String, String> {
    let trimmed = raw_key.trim();
    if trimmed.is_empty() {
        return Err("metadata key cannot be empty".to_string());
    }

    let canonical = if trimmed.contains('.') {
        trimmed.to_string()
    } else {
        format!("{default_namespace}.{trimmed}")
    };

    if canonical.len() > METADATA_MAX_KEY_LEN {
        return Err(format!(
            "metadata key '{trimmed}' exceeds max length {METADATA_MAX_KEY_LEN}"
        ));
    }

    let segments: Vec<&str> = canonical.split('.').collect();
    if segments.len() > METADATA_MAX_DEPTH {
        return Err(format!(
            "metadata key '{canonical}' exceeds max depth {METADATA_MAX_DEPTH}"
        ));
    }

    let namespace = segments.first().copied().unwrap_or_default();
    if !ALLOWED_METADATA_NAMESPACES.contains(&namespace) {
        return Err(format!(
            "metadata key '{canonical}' uses unsupported namespace '{namespace}'"
        ));
    }

    for segment in segments {
        if segment.is_empty() {
            return Err(format!(
                "metadata key '{canonical}' contains empty path segments"
            ));
        }
        if segment.len() > METADATA_MAX_SEGMENT_LEN {
            return Err(format!(
                "metadata key segment '{segment}' exceeds max length {METADATA_MAX_SEGMENT_LEN}"
            ));
        }
        if !segment
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
        {
            return Err(format!(
                "metadata key '{canonical}' contains invalid characters; allowed: a-z, A-Z, 0-9, _, - and ."
            ));
        }
    }

    Ok(canonical)
}

fn canonical_and_legacy_metadata_key(
    raw_key: &str,
    default_namespace: &str,
) -> Result<(String, Option<String>), String> {
    let canonical = canonicalize_metadata_key(raw_key, default_namespace)?;
    let trimmed = raw_key.trim();
    let legacy = if trimmed.contains('.') {
        None
    } else {
        Some(trimmed.to_string())
    };
    Ok((canonical, legacy))
}

fn validate_tag_value(raw_value: &str, max_len: usize) -> Result<(), String> {
    if raw_value.len() > max_len {
        return Err(format!("tag value exceeds max length {max_len} characters"));
    }
    Ok(())
}

fn validate_param_value(raw_value: &str) -> Result<(), String> {
    if raw_value.len() > PARAM_VALUE_MAX_LEN {
        return Err(format!(
            "parameter value exceeds max length {PARAM_VALUE_MAX_LEN} characters"
        ));
    }
    Ok(())
}

fn json_value_depth(value: &serde_json::Value) -> usize {
    match value {
        serde_json::Value::Array(items) => {
            1 + items.iter().map(json_value_depth).max().unwrap_or(0)
        }
        serde_json::Value::Object(map) => 1 + map.values().map(json_value_depth).max().unwrap_or(0),
        _ => 1,
    }
}

fn stringify_config_param_value(value: &serde_json::Value) -> Result<String, String> {
    let depth = json_value_depth(value);
    if depth > METADATA_MAX_DEPTH {
        return Err(format!(
            "config value exceeds max depth {METADATA_MAX_DEPTH}"
        ));
    }

    let encoded = match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Bool(_) | serde_json::Value::Number(_) | serde_json::Value::Null => {
            value.to_string()
        }
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => serde_json::to_string(value)
            .map_err(|err| format!("failed to encode config value: {err}"))?,
    };

    validate_param_value(&encoded)?;
    Ok(encoded)
}

fn parse_metadata_filters(
    raw: Option<&str>,
    default_namespace: &str,
    query_name: &str,
) -> Result<Vec<MetadataFilter>, (StatusCode, String)> {
    let Some(value) = raw else {
        return Ok(Vec::new());
    };

    let mut filters = Vec::new();
    for token in value
        .split(',')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
    {
        let (raw_key, raw_value) = if let Some((left, right)) = token.split_once('=') {
            (left.trim(), Some(right.trim().to_string()))
        } else {
            (token, None)
        };

        if raw_key.is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("Invalid {query_name} filter '{token}': missing key"),
            ));
        }

        let (canonical_key, legacy_key) =
            canonical_and_legacy_metadata_key(raw_key, default_namespace).map_err(|err| {
                (
                    StatusCode::BAD_REQUEST,
                    format!("Invalid {query_name} filter '{token}': {err}"),
                )
            })?;

        filters.push(MetadataFilter {
            key: canonical_key,
            legacy_key,
            value: raw_value,
        });
    }

    Ok(filters)
}

fn normalize_created_at_filter(
    raw: &str,
    field_name: &str,
) -> Result<String, (StatusCode, String)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("{field_name} cannot be empty"),
        ));
    }

    if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(trimmed) {
        return Ok(parsed
            .with_timezone(&chrono::Utc)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string());
    }

    if let Ok(parsed) = chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S") {
        return Ok(parsed.format("%Y-%m-%d %H:%M:%S").to_string());
    }

    Err((
        StatusCode::BAD_REQUEST,
        format!("Invalid {field_name}: expected RFC3339 or 'YYYY-MM-DD HH:MM:SS' format"),
    ))
}

const RUN_FILTER_MAX_LEN: usize = 2048;
const RUN_FILTER_MAX_TOKENS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
enum RunFilterToken {
    Identifier(String),
    StringLiteral(String),
    Operator(String),
    And,
    Or,
    LParen,
    RParen,
}

fn tokenize_run_filter(raw: &str) -> Result<Vec<RunFilterToken>, String> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = raw.chars().collect();
    let mut idx = 0usize;

    while idx < chars.len() {
        let ch = chars[idx];
        if ch.is_whitespace() {
            idx += 1;
            continue;
        }

        match ch {
            '(' => {
                tokens.push(RunFilterToken::LParen);
                idx += 1;
                continue;
            }
            ')' => {
                tokens.push(RunFilterToken::RParen);
                idx += 1;
                continue;
            }
            '~' => {
                tokens.push(RunFilterToken::Operator("~".to_string()));
                idx += 1;
                continue;
            }
            '"' | '\'' => {
                let quote = ch;
                idx += 1;
                let mut value = String::new();
                let mut closed = false;
                while idx < chars.len() {
                    let current = chars[idx];
                    if current == '\\' && idx + 1 < chars.len() {
                        value.push(chars[idx + 1]);
                        idx += 2;
                        continue;
                    }
                    if current == quote {
                        closed = true;
                        idx += 1;
                        break;
                    }
                    value.push(current);
                    idx += 1;
                }
                if !closed {
                    return Err("unterminated string literal".to_string());
                }
                tokens.push(RunFilterToken::StringLiteral(value));
                continue;
            }
            '!' | '>' | '<' | '=' => {
                if idx + 1 < chars.len() {
                    let pair = [ch, chars[idx + 1]];
                    match pair {
                        ['!', '='] => {
                            tokens.push(RunFilterToken::Operator("!=".to_string()));
                            idx += 2;
                            continue;
                        }
                        ['>', '='] => {
                            tokens.push(RunFilterToken::Operator(">=".to_string()));
                            idx += 2;
                            continue;
                        }
                        ['<', '='] => {
                            tokens.push(RunFilterToken::Operator("<=".to_string()));
                            idx += 2;
                            continue;
                        }
                        _ => {}
                    }
                }

                if ch == '!' {
                    return Err("expected '!=' operator".to_string());
                }

                tokens.push(RunFilterToken::Operator(ch.to_string()));
                idx += 1;
                continue;
            }
            _ => {}
        }

        let start = idx;
        while idx < chars.len() {
            let current = chars[idx];
            if current.is_whitespace()
                || matches!(
                    current,
                    '(' | ')' | '~' | '!' | '>' | '<' | '=' | '"' | '\''
                )
            {
                break;
            }
            idx += 1;
        }

        let word = chars[start..idx].iter().collect::<String>();
        if word.is_empty() {
            return Err("unexpected token boundary".to_string());
        }

        if word.eq_ignore_ascii_case("and") {
            tokens.push(RunFilterToken::And);
        } else if word.eq_ignore_ascii_case("or") {
            tokens.push(RunFilterToken::Or);
        } else {
            tokens.push(RunFilterToken::Identifier(word));
        }
    }

    if tokens.len() > RUN_FILTER_MAX_TOKENS {
        return Err(format!(
            "filter expression exceeds max token count {RUN_FILTER_MAX_TOKENS}"
        ));
    }

    Ok(tokens)
}

struct RunFilterParser {
    tokens: Vec<RunFilterToken>,
    cursor: usize,
}

impl RunFilterParser {
    fn new(tokens: Vec<RunFilterToken>) -> Self {
        Self { tokens, cursor: 0 }
    }

    fn parse(mut self) -> Result<RunFilterExpr, String> {
        let expr = self.parse_or_expression()?;
        if self.cursor < self.tokens.len() {
            return Err("unexpected trailing token".to_string());
        }
        Ok(expr)
    }

    fn parse_or_expression(&mut self) -> Result<RunFilterExpr, String> {
        let mut expr = self.parse_and_expression()?;
        while matches!(self.peek(), Some(RunFilterToken::Or)) {
            self.cursor += 1;
            let right = self.parse_and_expression()?;
            expr = RunFilterExpr::Or(Box::new(expr), Box::new(right));
        }
        Ok(expr)
    }

    fn parse_and_expression(&mut self) -> Result<RunFilterExpr, String> {
        let mut expr = self.parse_primary_expression()?;
        while matches!(self.peek(), Some(RunFilterToken::And)) {
            self.cursor += 1;
            let right = self.parse_primary_expression()?;
            expr = RunFilterExpr::And(Box::new(expr), Box::new(right));
        }
        Ok(expr)
    }

    fn parse_primary_expression(&mut self) -> Result<RunFilterExpr, String> {
        if matches!(self.peek(), Some(RunFilterToken::LParen)) {
            self.cursor += 1;
            let expr = self.parse_or_expression()?;
            if !matches!(self.peek(), Some(RunFilterToken::RParen)) {
                return Err("expected ')'".to_string());
            }
            self.cursor += 1;
            return Ok(expr);
        }
        self.parse_condition_expression()
    }

    fn parse_condition_expression(&mut self) -> Result<RunFilterExpr, String> {
        let field = match self.next() {
            Some(RunFilterToken::Identifier(value)) => value,
            _ => return Err("expected field identifier".to_string()),
        };
        let op = match self.next() {
            Some(RunFilterToken::Operator(value)) => value,
            _ => return Err("expected comparison operator".to_string()),
        };
        let value = match self.next() {
            Some(RunFilterToken::Identifier(raw)) | Some(RunFilterToken::StringLiteral(raw)) => raw,
            _ => return Err("expected comparison value".to_string()),
        };

        let condition = build_run_filter_condition(&field, &op, &value)?;
        Ok(RunFilterExpr::Condition(condition))
    }

    fn next(&mut self) -> Option<RunFilterToken> {
        let token = self.tokens.get(self.cursor).cloned();
        if token.is_some() {
            self.cursor += 1;
        }
        token
    }

    fn peek(&self) -> Option<&RunFilterToken> {
        self.tokens.get(self.cursor)
    }
}

fn parse_run_filter_operator(raw: &str) -> Result<RunFilterOperator, String> {
    match raw {
        "=" => Ok(RunFilterOperator::Eq),
        "!=" => Ok(RunFilterOperator::NotEq),
        ">" => Ok(RunFilterOperator::Gt),
        ">=" => Ok(RunFilterOperator::Gte),
        "<" => Ok(RunFilterOperator::Lt),
        "<=" => Ok(RunFilterOperator::Lte),
        "~" => Ok(RunFilterOperator::Contains),
        _ => Err(format!("unsupported operator '{raw}'")),
    }
}

fn build_run_filter_condition(
    field_raw: &str,
    op_raw: &str,
    value_raw: &str,
) -> Result<RunFilterCondition, String> {
    let op = parse_run_filter_operator(op_raw)?;
    let field = field_raw.trim().to_ascii_lowercase();
    let mut value = value_raw.trim().to_string();

    if value.is_empty() {
        return Err("comparison value cannot be empty".to_string());
    }

    let condition = match field.as_str() {
        "project" | "project_id" => {
            if !matches!(
                op,
                RunFilterOperator::Eq | RunFilterOperator::NotEq | RunFilterOperator::Contains
            ) {
                return Err("project supports only '=', '!=', and '~' operators".to_string());
            }
            RunFilterCondition {
                target: RunFilterTarget::Project,
                op,
                value,
            }
        }
        "owner" | "owner_user_id" => {
            if !matches!(
                op,
                RunFilterOperator::Eq | RunFilterOperator::NotEq | RunFilterOperator::Contains
            ) {
                return Err("owner supports only '=', '!=', and '~' operators".to_string());
            }
            RunFilterCondition {
                target: RunFilterTarget::Owner,
                op,
                value,
            }
        }
        "status" => {
            if !matches!(
                op,
                RunFilterOperator::Eq | RunFilterOperator::NotEq | RunFilterOperator::Contains
            ) {
                return Err("status supports only '=', '!=', and '~' operators".to_string());
            }
            value = value.to_ascii_lowercase();
            RunFilterCondition {
                target: RunFilterTarget::Status,
                op,
                value,
            }
        }
        "name" => {
            if !matches!(
                op,
                RunFilterOperator::Eq | RunFilterOperator::NotEq | RunFilterOperator::Contains
            ) {
                return Err("name supports only '=', '!=', and '~' operators".to_string());
            }
            RunFilterCondition {
                target: RunFilterTarget::Name,
                op,
                value,
            }
        }
        "id" | "run_id" => {
            if !matches!(
                op,
                RunFilterOperator::Eq | RunFilterOperator::NotEq | RunFilterOperator::Contains
            ) {
                return Err("run_id supports only '=', '!=', and '~' operators".to_string());
            }
            RunFilterCondition {
                target: RunFilterTarget::RunId,
                op,
                value,
            }
        }
        "created_at" => {
            if !matches!(
                op,
                RunFilterOperator::Eq
                    | RunFilterOperator::NotEq
                    | RunFilterOperator::Gt
                    | RunFilterOperator::Gte
                    | RunFilterOperator::Lt
                    | RunFilterOperator::Lte
            ) {
                return Err(
                    "created_at supports only '=', '!=', '>', '>=', '<', and '<=' operators"
                        .to_string(),
                );
            }
            value = normalize_created_at_filter(&value, "filter.created_at")
                .map_err(|(_, message)| message)?;
            RunFilterCondition {
                target: RunFilterTarget::CreatedAt,
                op,
                value,
            }
        }
        _ => {
            if let Some(raw_key) = field
                .strip_prefix("tag.")
                .or_else(|| field.strip_prefix("tags."))
            {
                if !matches!(
                    op,
                    RunFilterOperator::Eq | RunFilterOperator::NotEq | RunFilterOperator::Contains
                ) {
                    return Err("tag filters support only '=', '!=', and '~' operators".to_string());
                }
                let (key, legacy_key) = canonical_and_legacy_metadata_key(raw_key, "tags")?;
                RunFilterCondition {
                    target: RunFilterTarget::Tag { key, legacy_key },
                    op,
                    value,
                }
            } else if let Some(raw_key) = field
                .strip_prefix("param.")
                .or_else(|| field.strip_prefix("params."))
            {
                if !matches!(
                    op,
                    RunFilterOperator::Eq | RunFilterOperator::NotEq | RunFilterOperator::Contains
                ) {
                    return Err(
                        "param filters support only '=', '!=', and '~' operators".to_string()
                    );
                }
                let (key, legacy_key) = canonical_and_legacy_metadata_key(raw_key, "parameters")?;
                RunFilterCondition {
                    target: RunFilterTarget::Param { key, legacy_key },
                    op,
                    value,
                }
            } else {
                return Err(format!("unsupported filter field '{field_raw}'"));
            }
        }
    };

    Ok(condition)
}

fn parse_run_filter_expr(raw: Option<&str>) -> Result<Option<RunFilterExpr>, (StatusCode, String)> {
    let Some(raw_filter) = raw else {
        return Ok(None);
    };
    let trimmed = raw_filter.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.len() > RUN_FILTER_MAX_LEN {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("filter expression exceeds max length {RUN_FILTER_MAX_LEN} characters"),
        ));
    }

    let tokens = tokenize_run_filter(trimmed).map_err(|err| {
        (
            StatusCode::BAD_REQUEST,
            format!("Invalid filter expression: {err}"),
        )
    })?;
    if tokens.is_empty() {
        return Ok(None);
    }

    let expr = RunFilterParser::new(tokens).parse().map_err(|err| {
        (
            StatusCode::BAD_REQUEST,
            format!("Invalid filter expression: {err}"),
        )
    })?;

    Ok(Some(expr))
}

fn parse_run_list_sort_field(raw: Option<&str>) -> Result<RunListSortField, (StatusCode, String)> {
    let Some(value) = raw else {
        return Ok(RunListSortField::CreatedAt);
    };
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "" | "created_at" | "created" => Ok(RunListSortField::CreatedAt),
        "updated_at" | "updated" => Ok(RunListSortField::UpdatedAt),
        "name" => Ok(RunListSortField::Name),
        "status" => Ok(RunListSortField::Status),
        "duration" | "duration_seconds" => Ok(RunListSortField::DurationSeconds),
        "metrics_count" | "metrics" => Ok(RunListSortField::MetricsCount),
        "params_count" | "params" => Ok(RunListSortField::ParamsCount),
        _ => Err((
            StatusCode::BAD_REQUEST,
            format!("Invalid sort_by value '{value}'"),
        )),
    }
}

fn parse_run_list_sort_order(raw: Option<&str>) -> Result<RunListSortOrder, (StatusCode, String)> {
    let Some(value) = raw else {
        return Ok(RunListSortOrder::Desc);
    };
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "" | "desc" | "descending" => Ok(RunListSortOrder::Desc),
        "asc" | "ascending" => Ok(RunListSortOrder::Asc),
        _ => Err((
            StatusCode::BAD_REQUEST,
            format!("Invalid sort_order value '{value}'"),
        )),
    }
}

const fn auth_mode_label(auth: &AuthContext) -> &'static str {
    match auth.auth_mode {
        AuthMode::ApiKey => "api_key",
        AuthMode::UiJwt => "ui_jwt",
    }
}

fn auth_user_id(auth: &AuthContext) -> Option<String> {
    if auth.is_dev_mode || !auth.is_ui_jwt() {
        return None;
    }
    auth.api_key
        .id
        .split_once(':')
        .map(|(_, value)| value.to_string())
}

fn audit_actor_ids(auth: &AuthContext) -> (Option<String>, Option<String>) {
    if auth.is_dev_mode {
        return (None, None);
    }

    if auth.is_ui_jwt() {
        (auth_user_id(auth), None)
    } else {
        (None, Some(auth.api_key.id.clone()))
    }
}

fn is_unique_project_name_error(err: &storage::SqliteError) -> bool {
    match err {
        storage::SqliteError::Database(db_err) => db_err
            .to_string()
            .contains("UNIQUE constraint failed: projects.name"),
        _ => false,
    }
}

fn should_enforce_scope(auth: &AuthContext, tier: EndpointRbacTier) -> bool {
    if auth.is_dev_mode {
        return false;
    }
    if !auth.is_ui_jwt() {
        return true;
    }

    env_flag_default(tier.env_flag_name(), true)
}

fn usize_to_i32_saturating(value: usize) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

fn usize_to_i64_saturating(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn i64_to_u64_or_zero(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

fn retry_after_seconds(wait: f64) -> u64 {
    if !wait.is_finite() || wait <= 0.0 {
        return 1;
    }
    std::time::Duration::try_from_secs_f64(wait.ceil())
        .map(|duration| duration.as_secs())
        .unwrap_or(u64::MAX)
}

#[allow(clippy::too_many_arguments)]
async fn emit_audit_event(
    state: &AppState,
    auth: Option<&AuthContext>,
    project_id: Option<&str>,
    run_id: Option<&str>,
    action: &str,
    resource_type: &str,
    resource_id: Option<&str>,
    outcome: &str,
    metadata: serde_json::Value,
) {
    let metadata_json = serde_json::to_string(&metadata).ok();
    let (actor_user_id, actor_key_id) = if let Some(auth) = auth {
        let (user_id, key_id) = audit_actor_ids(auth);
        (user_id, key_id)
    } else {
        (None, None)
    };

    let (request_id, client_ip, user_agent) = if let Some(auth) = auth {
        (
            Some(auth.request_id.clone()),
            auth.client_ip.clone(),
            auth.user_agent.clone(),
        )
    } else {
        (None, None, None)
    };

    if let Err(err) = state
        .sqlite_store
        .insert_audit_event(
            actor_user_id.as_deref(),
            actor_key_id.as_deref(),
            project_id,
            run_id,
            action,
            resource_type,
            resource_id,
            outcome,
            request_id.as_deref(),
            client_ip.as_deref(),
            user_agent.as_deref(),
            metadata_json.as_deref(),
        )
        .await
    {
        warn!(
            error = %err,
            action = %action,
            outcome = %outcome,
            "Failed to persist audit event"
        );
    }
}

#[allow(clippy::too_many_arguments)]
async fn require_endpoint_access(
    state: &AppState,
    auth: &AuthContext,
    tier: EndpointRbacTier,
    project_id: Option<&str>,
    run_id: Option<&str>,
    action: &str,
    resource_type: &str,
    resource_id: Option<&str>,
) -> Result<(), (StatusCode, String)> {
    let scope = tier.scope();
    let scope_enforced = should_enforce_scope(auth, tier);

    if scope_enforced {
        if let Err((status, message)) = auth.require_scope(scope) {
            emit_audit_event(
                state,
                Some(auth),
                project_id,
                run_id,
                action,
                resource_type,
                resource_id,
                "denied",
                serde_json::json!({
                    "reason": "scope_denied",
                    "required_scope": scope,
                    "auth_mode": auth_mode_label(auth),
                }),
            )
            .await;
            return Err((status, message));
        }
    }

    if let Some(project_id) = project_id {
        if let Err((status, message)) = auth.require_project_access(project_id) {
            emit_audit_event(
                state,
                Some(auth),
                Some(project_id),
                run_id,
                action,
                resource_type,
                resource_id,
                "denied",
                serde_json::json!({
                    "reason": "project_access_denied",
                    "required_scope": scope,
                    "scope_enforced": scope_enforced,
                    "auth_mode": auth_mode_label(auth),
                }),
            )
            .await;
            return Err((status, message));
        }
    }

    Ok(())
}

async fn require_platform_admin_access(
    state: &AppState,
    auth: &AuthContext,
    action: &str,
    resource_type: &str,
    resource_id: Option<&str>,
) -> Result<(), (StatusCode, String)> {
    require_endpoint_access(
        state,
        auth,
        EndpointRbacTier::Admin,
        None,
        None,
        action,
        resource_type,
        resource_id,
    )
    .await?;

    if auth.is_global() {
        return Ok(());
    }

    emit_audit_event(
        state,
        Some(auth),
        None,
        None,
        action,
        resource_type,
        resource_id,
        "denied",
        serde_json::json!({
            "reason": "platform_admin_required",
            "auth_mode": auth_mode_label(auth),
        }),
    )
    .await;

    Err((
        StatusCode::FORBIDDEN,
        "Platform admin access required.".to_string(),
    ))
}

fn require_ui_run_owner(auth: &AuthContext, run: &RunRow) -> Result<(), (StatusCode, String)> {
    match enforce_ui_run_owner(
        auth.is_dev_mode,
        auth.is_platform_admin,
        auth.is_ui_jwt(),
        auth_user_id(auth).as_deref(),
        run.created_by_user_id.as_deref(),
    ) {
        Ok(()) => Ok(()),
        Err(UiRunOwnerPolicyError::MissingUserIdentity) => Err((
            StatusCode::FORBIDDEN,
            "Unable to resolve user identity for run authorization.".to_string(),
        )),
        Err(UiRunOwnerPolicyError::OwnerMismatch) => Err((
            StatusCode::FORBIDDEN,
            "Access denied: this user cannot access that run.".to_string(),
        )),
    }
}

async fn require_ui_project_owner(
    state: &AppState,
    auth: &AuthContext,
    project_id: &str,
) -> Result<(), (StatusCode, String)> {
    if auth.is_dev_mode || !auth.is_ui_jwt() {
        return Ok(());
    }

    let user_id = auth_user_id(auth).ok_or_else(|| {
        (
            StatusCode::FORBIDDEN,
            "Unable to resolve user identity for project authorization.".to_string(),
        )
    })?;

    let memberships = state
        .sqlite_store
        .list_active_project_memberships(&user_id)
        .await
        .map_err(internal_error)?;

    let owns_project = memberships
        .iter()
        .any(|membership| membership.project_id == project_id && membership.role == "owner");

    if !owns_project {
        return Err((
            StatusCode::FORBIDDEN,
            "Access denied: only project owners can delete this project.".to_string(),
        ));
    }

    Ok(())
}

fn require_api_key_run_owner_for_mutation(
    auth: &AuthContext,
    run: &RunRow,
    action: &str,
) -> Result<(), (StatusCode, String)> {
    if auth.is_dev_mode || auth.is_ui_jwt() || auth.require_scope("admin").is_ok() {
        return Ok(());
    }

    if let Some(owner_key_id) = run.created_by_key_id.as_deref() {
        if owner_key_id != auth.api_key.id {
            return Err((
                StatusCode::FORBIDDEN,
                format!("Access denied: this API key can only {action} runs it created."),
            ));
        }
    }

    Ok(())
}

fn postgres_metadata_shadow_writes_enabled() -> bool {
    env_flag_default("MLRUNX_POSTGRES_METADATA_SHADOW_WRITES_ENABLED", false)
}

async fn maybe_shadow_write_project_to_postgres(project: &ProjectRow) {
    if !postgres_metadata_shadow_writes_enabled() {
        return;
    }

    let project_uuid = match uuid::Uuid::parse_str(&project.id) {
        Ok(value) => value,
        Err(err) => {
            warn!(
                project_id = %project.id,
                error = %err,
                "Skipped PostgreSQL shadow write for project with non-UUID id"
            );
            return;
        }
    };

    let result = ProjectRepository::create(CreateProjectInput {
        id: Some(project_uuid),
        name: project.name.clone(),
        description: project.description.clone(),
        owner_id: None,
        settings: None,
    })
    .await;

    if let Err(err) = result {
        warn!(
            project_id = %project.id,
            error = %err,
            "PostgreSQL project shadow write failed"
        );
    }
}

async fn maybe_shadow_write_run_to_postgres(
    run_id: &str,
    project_id: &str,
    name: Option<&str>,
    tags: Option<&std::collections::HashMap<String, String>>,
) {
    if !postgres_metadata_shadow_writes_enabled() {
        return;
    }

    let run_uuid = match uuid::Uuid::parse_str(run_id) {
        Ok(value) => value,
        Err(err) => {
            warn!(
                run_id = %run_id,
                error = %err,
                "Skipped PostgreSQL run shadow write because run_id is not UUID"
            );
            return;
        }
    };
    let project_uuid = match uuid::Uuid::parse_str(project_id) {
        Ok(value) => value,
        Err(err) => {
            warn!(
                project_id = %project_id,
                error = %err,
                "Skipped PostgreSQL run shadow write because project_id is not UUID"
            );
            return;
        }
    };

    let tags_json = tags.map_or_else(
        || serde_json::json!({}),
        |t| serde_json::to_value(t).unwrap_or_else(|_| serde_json::json!({})),
    );

    let result = RunRepository::create(CreateRunInput {
        id: Some(run_uuid),
        project_id: project_uuid,
        name: name.map(ToOwned::to_owned),
        description: None,
        parent_run_id: None,
        tags: Some(tags_json),
        system_info: None,
        git_info: None,
    })
    .await;

    if let Err(err) = result {
        warn!(
            run_id = %run_id,
            project_id = %project_id,
            error = %err,
            "PostgreSQL run shadow write failed"
        );
    }
}

fn postgres_run_status_from_http(status: &str) -> Option<PostgresRunStatus> {
    match status.trim().to_ascii_lowercase().as_str() {
        "pending" => Some(PostgresRunStatus::Pending),
        "running" => Some(PostgresRunStatus::Running),
        "finished" => Some(PostgresRunStatus::Finished),
        "failed" => Some(PostgresRunStatus::Failed),
        "killed" => Some(PostgresRunStatus::Killed),
        _ => None,
    }
}

async fn maybe_shadow_finish_run_in_postgres(run_id: &str, status: &str) {
    if !postgres_metadata_shadow_writes_enabled() {
        return;
    }

    let run_uuid = match uuid::Uuid::parse_str(run_id) {
        Ok(value) => value,
        Err(err) => {
            warn!(
                run_id = %run_id,
                error = %err,
                "Skipped PostgreSQL run status shadow write because run_id is not UUID"
            );
            return;
        }
    };
    let Some(status_value) = postgres_run_status_from_http(status) else {
        warn!(
            run_id = %run_id,
            status = %status,
            "Skipped PostgreSQL run status shadow write for unknown status"
        );
        return;
    };

    if let Err(err) = RunRepository::update_status(run_uuid, status_value, None).await {
        warn!(
            run_id = %run_id,
            status = %status,
            error = %err,
            "PostgreSQL run status shadow write failed"
        );
    }
}

fn extract_cookie(headers: &HeaderMap, cookie_name: &str) -> Option<String> {
    let cookie_header = headers.get(header::COOKIE)?.to_str().ok()?;
    cookie_header.split(';').find_map(|part| {
        let mut kv = part.trim().splitn(2, '=');
        let name = kv.next()?.trim();
        let value = kv.next()?.trim();
        if name == cookie_name && !value.is_empty() {
            Some(value.to_string())
        } else {
            None
        }
    })
}

fn header_string(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned)
}

fn trusted_forwarded_client_ip(headers: &HeaderMap) -> Option<String> {
    if let Some(forwarded) = header_string(headers, "x-forwarded-for") {
        let first = forwarded
            .split(',')
            .next()
            .map(str::trim)
            .unwrap_or_default();
        if !first.is_empty() {
            return Some(first.to_string());
        }
    }
    header_string(headers, "x-real-ip")
}

fn client_ip_from_socket_extensions(extensions: &axum::http::Extensions) -> Option<String> {
    extensions
        .get::<axum::extract::ConnectInfo<SocketAddr>>()
        .map(|info| info.0.ip().to_string())
        .or_else(|| {
            extensions
                .get::<SocketAddr>()
                .map(|addr| addr.ip().to_string())
        })
}

fn should_trust_forwarded_headers(socket_ip: Option<IpAddr>) -> bool {
    if !trust_proxy_headers() {
        return false;
    }

    let Some(peer_ip) = socket_ip else {
        return false;
    };
    let Ok(trusted_rules) = trusted_proxy_rules_from_env() else {
        return false;
    };
    if trusted_rules.is_empty() {
        return false;
    }

    trusted_rules.into_iter().any(|rule| rule.matches(peer_ip))
}

fn infer_client_ip(headers: &HeaderMap, socket_addr: Option<SocketAddr>) -> Option<String> {
    let socket_ip = socket_addr.map(|addr| addr.ip());
    if should_trust_forwarded_headers(socket_ip) {
        if let Some(forwarded_ip) = trusted_forwarded_client_ip(headers) {
            return Some(forwarded_ip);
        }
    }
    socket_ip.map(|ip| ip.to_string())
}

fn extract_bearer_token(headers: &HeaderMap) -> Option<String> {
    let auth = header_string(headers, "authorization")?;
    let token = auth.strip_prefix("Bearer ")?;
    let token = token.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

fn build_cookie(
    name: &str,
    value: &str,
    ttl_seconds: u64,
    http_only: bool,
    secure: bool,
    same_site: &str,
    domain: Option<&str>,
) -> String {
    let mut cookie = format!("{name}={value}; Path=/; Max-Age={ttl_seconds}; SameSite={same_site}");
    if let Some(domain) = domain {
        cookie.push_str("; Domain=");
        cookie.push_str(domain);
    }
    if http_only {
        cookie.push_str("; HttpOnly");
    }
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

fn build_clear_cookie(name: &str, secure: bool, same_site: &str, domain: Option<&str>) -> String {
    let mut cookie = format!(
        "{name}=; Path=/; Max-Age=0; Expires=Thu, 01 Jan 1970 00:00:00 GMT; SameSite={same_site}"
    );
    if let Some(domain) = domain {
        cookie.push_str("; Domain=");
        cookie.push_str(domain);
    }
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

#[allow(clippy::too_many_lines)]
async fn http_ui_auth_login(
    State(state): State<AppState>,
    headers: HeaderMap,
    connect_info: axum::extract::ConnectInfo<SocketAddr>,
    Json(req): Json<UiAuthLoginRequest>,
) -> Result<(HeaderMap, Json<UiAuthLoginResponse>), (StatusCode, String)> {
    let user_agent = header_string(&headers, "user-agent");
    let client_ip = infer_client_ip(&headers, Some(connect_info.0));

    if let Some(retry_after) = state
        .key_store
        .auth_rate_limit_retry_after_seconds(client_ip.as_deref())
        .await
    {
        state
            .key_store
            .record_auth_failure(
                client_ip.as_deref(),
                user_agent.as_deref(),
                "/api/v1/ui-auth/login",
                "POST",
                "rate_limited",
                "ui_auth_login",
            )
            .await;
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            format!("Too many failed authentication attempts. Retry in {retry_after}s."),
        ));
    }

    let jwt = req
        .jwt
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| extract_bearer_token(&headers))
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                "JWT is required (body `jwt` or Authorization: Bearer <token>).".to_string(),
            )
        })?;

    let issue = match state
        .key_store
        .create_ui_session_from_jwt(&jwt, user_agent.as_deref(), client_ip.as_deref())
        .await
    {
        Ok(issue) => issue,
        Err(e) => {
            let reason = if e.to_ascii_lowercase().contains("expired") {
                "expired_ui_jwt"
            } else {
                "invalid_ui_jwt"
            };
            state
                .key_store
                .record_auth_failure(
                    client_ip.as_deref(),
                    user_agent.as_deref(),
                    "/api/v1/ui-auth/login",
                    "POST",
                    reason,
                    "ui_auth_login",
                )
                .await;
            return Err((StatusCode::UNAUTHORIZED, e));
        }
    };

    state
        .key_store
        .record_auth_success(client_ip.as_deref())
        .await;

    let secure_cookie = state.key_store.ui_cookie_secure();
    let same_site = state.key_store.ui_cookie_same_site();
    let cookie_domain = state.key_store.ui_cookie_domain();
    let ttl = state.key_store.ui_session_ttl_seconds();

    let session_cookie = build_cookie(
        state.key_store.ui_session_cookie_name(),
        &issue.session_token,
        ttl,
        true,
        secure_cookie,
        same_site,
        cookie_domain,
    );
    let csrf_cookie = build_cookie(
        state.key_store.ui_csrf_cookie_name(),
        &issue.csrf_token,
        ttl,
        false,
        secure_cookie,
        same_site,
        cookie_domain,
    );

    let mut response_headers = HeaderMap::new();
    response_headers.append(
        header::SET_COOKIE,
        HeaderValue::from_str(&session_cookie).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to set session cookie: {e}"),
            )
        })?,
    );
    response_headers.append(
        header::SET_COOKIE,
        HeaderValue::from_str(&csrf_cookie).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to set csrf cookie: {e}"),
            )
        })?,
    );

    Ok((
        response_headers,
        Json(UiAuthLoginResponse {
            status: "ok".to_string(),
            user_id: issue.user_id,
            expires_at: issue.expires_at,
            project_count: issue.allowed_project_ids.len(),
        }),
    ))
}

async fn http_ui_auth_session(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
) -> Result<Json<UiAuthSessionResponse>, (StatusCode, String)> {
    let mut project_ids: Vec<String> = auth
        .allowed_project_ids()
        .map(|ids| ids.iter().cloned().collect())
        .unwrap_or_default();
    project_ids.sort();

    Ok(Json(UiAuthSessionResponse {
        authenticated: true,
        auth_mode: match auth.auth_mode {
            AuthMode::ApiKey => "api_key".to_string(),
            AuthMode::UiJwt => "ui_session".to_string(),
        },
        scopes: auth.api_key.scopes.clone(),
        project_ids,
        key_prefix: auth.api_key.key_prefix.clone(),
        is_dev_mode: auth.is_dev_mode,
        is_platform_admin: auth.is_platform_admin,
        ui_session_ttl_seconds: state.key_store.ui_session_ttl_seconds(),
        ui_key_max_ttl_seconds: ui_key_max_ttl_seconds(),
    }))
}

async fn http_ui_auth_logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(HeaderMap, Json<UiAuthLogoutResponse>), (StatusCode, String)> {
    let session_cookie_name = state.key_store.ui_session_cookie_name().to_string();
    let csrf_cookie_name = state.key_store.ui_csrf_cookie_name().to_string();
    let same_site = state.key_store.ui_cookie_same_site().to_string();
    let secure_cookie = state.key_store.ui_cookie_secure();
    let cookie_domain = state.key_store.ui_cookie_domain();

    let session_token = extract_cookie(&headers, &session_cookie_name);
    let csrf_token = header_string(&headers, "x-csrf-token");

    if let Some(token) = session_token {
        state
            .key_store
            .revoke_ui_session(&token, csrf_token.as_deref())
            .await
            .map_err(|e| {
                let status = if e.contains("CSRF") {
                    StatusCode::FORBIDDEN
                } else {
                    StatusCode::UNAUTHORIZED
                };
                (status, e)
            })?;
    }

    let clear_session_cookie = build_clear_cookie(
        &session_cookie_name,
        secure_cookie,
        &same_site,
        cookie_domain,
    );
    let clear_csrf_cookie =
        build_clear_cookie(&csrf_cookie_name, secure_cookie, &same_site, cookie_domain);

    let mut response_headers = HeaderMap::new();
    response_headers.append(
        header::SET_COOKIE,
        HeaderValue::from_str(&clear_session_cookie).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to clear session cookie: {e}"),
            )
        })?,
    );
    response_headers.append(
        header::SET_COOKIE,
        HeaderValue::from_str(&clear_csrf_cookie).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to clear csrf cookie: {e}"),
            )
        })?,
    );

    Ok((
        response_headers,
        Json(UiAuthLogoutResponse {
            status: "ok".to_string(),
        }),
    ))
}

/// Request to initialize a run via HTTP.
#[derive(Debug, Deserialize)]
struct InitRunHttpRequest {
    project: Option<String>,
    project_id: Option<String>,
    name: Option<String>,
    run_id: Option<String>,
    tags: Option<std::collections::HashMap<String, String>>,
    config: Option<std::collections::HashMap<String, serde_json::Value>>,
}

/// Response from init run.
#[derive(Debug, Serialize)]
struct InitRunHttpResponse {
    run_id: String,
    offline: bool,
}

#[derive(Debug, Deserialize)]
struct CreateProjectRequest {
    name: String,
    description: Option<String>,
}

#[derive(Debug, Serialize)]
struct ProjectResponse {
    project_id: String,
    name: String,
    description: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize)]
struct ListProjectsResponse {
    projects: Vec<ProjectResponse>,
}

fn project_response_from_row(row: ProjectRow) -> ProjectResponse {
    ProjectResponse {
        project_id: row.id,
        name: row.name,
        description: row.description,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

async fn http_list_projects(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
) -> Result<Json<ListProjectsResponse>, (StatusCode, String)> {
    require_endpoint_access(
        &state,
        &auth,
        EndpointRbacTier::Read,
        None,
        None,
        "projects.list",
        "project",
        None,
    )
    .await?;

    let rows = if auth.is_global() && (!auth.is_ui_jwt() || auth.is_platform_admin) {
        state
            .sqlite_store
            .list_projects()
            .await
            .map_err(internal_error)?
    } else if let Some(allowed_projects) = auth.allowed_project_ids() {
        let project_ids: Vec<String> = allowed_projects.iter().cloned().collect();
        state
            .sqlite_store
            .list_projects_by_ids(&project_ids)
            .await
            .map_err(internal_error)?
    } else if let Some(project_id) = auth.project_id() {
        state
            .sqlite_store
            .list_projects_by_ids(&[project_id.to_string()])
            .await
            .map_err(internal_error)?
    } else {
        Vec::new()
    };

    let projects = rows.into_iter().map(project_response_from_row).collect();
    Ok(Json(ListProjectsResponse { projects }))
}

#[allow(clippy::too_many_lines)]
async fn http_create_project(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(req): Json<CreateProjectRequest>,
) -> Result<Json<ProjectResponse>, (StatusCode, String)> {
    let name = req.name.trim();
    if name.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Project name is required.".to_string(),
        ));
    }

    if auth.is_ui_jwt() {
        auth.require_scope("write")?;
        let user_id = auth_user_id(&auth).ok_or_else(|| {
            (
                StatusCode::FORBIDDEN,
                "Unable to resolve user identity for project creation.".to_string(),
            )
        })?;

        let row = state
            .sqlite_store
            .create_project(name, req.description.as_deref())
            .await
            .map_err(|e| {
                if is_unique_project_name_error(&e) {
                    (
                        StatusCode::CONFLICT,
                        format!("Project '{name}' already exists."),
                    )
                } else {
                    internal_error(e)
                }
            })?;

        state
            .sqlite_store
            .grant_project_membership(&row.id, &user_id, "owner", Some(&user_id))
            .await
            .map_err(internal_error)?;
        maybe_shadow_write_project_to_postgres(&row).await;

        emit_audit_event(
            &state,
            Some(&auth),
            Some(&row.id),
            None,
            "project.create",
            "project",
            Some(&row.id),
            "success",
            serde_json::json!({
                "name": row.name,
                "auth_mode": auth_mode_label(&auth),
            }),
        )
        .await;

        return Ok(Json(project_response_from_row(row)));
    }

    require_endpoint_access(
        &state,
        &auth,
        EndpointRbacTier::Admin,
        None,
        None,
        "project.create",
        "project",
        None,
    )
    .await?;

    if !auth.is_global() {
        return Err((
            StatusCode::FORBIDDEN,
            "Project creation requires a global admin key or UI session.".to_string(),
        ));
    }

    let row = state
        .sqlite_store
        .create_project(name, req.description.as_deref())
        .await
        .map_err(|e| {
            if is_unique_project_name_error(&e) {
                (
                    StatusCode::CONFLICT,
                    format!("Project '{name}' already exists."),
                )
            } else {
                internal_error(e)
            }
        })?;
    maybe_shadow_write_project_to_postgres(&row).await;

    emit_audit_event(
        &state,
        Some(&auth),
        Some(&row.id),
        None,
        "project.create",
        "project",
        Some(&row.id),
        "success",
        serde_json::json!({
            "name": row.name,
            "auth_mode": auth_mode_label(&auth),
        }),
    )
    .await;

    Ok(Json(project_response_from_row(row)))
}

async fn http_delete_project(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    axum::extract::Path(project_id): axum::extract::Path<String>,
) -> Result<Json<AdminMutationResponse>, (StatusCode, String)> {
    validate_path_id(&project_id, "project_id")?;
    let project = state
        .sqlite_store
        .get_project_by_id(&project_id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!("Project not found: '{project_id}'"),
            )
        })?;

    require_endpoint_access(
        &state,
        &auth,
        EndpointRbacTier::Admin,
        Some(&project_id),
        None,
        "project.delete",
        "project",
        Some(&project_id),
    )
    .await?;

    if auth.is_ui_jwt() {
        require_ui_project_owner(&state, &auth, &project_id).await?;
    } else if !auth.is_global() {
        return Err((
            StatusCode::FORBIDDEN,
            "Project deletion requires a global admin key or owner UI session.".to_string(),
        ));
    }

    state
        .sqlite_store
        .delete_project(&project_id)
        .await
        .map_err(|e| match e {
            storage::SqliteError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            _ => internal_error(e),
        })?;

    emit_audit_event(
        &state,
        Some(&auth),
        Some(&project_id),
        None,
        "project.delete",
        "project",
        Some(&project_id),
        "success",
        serde_json::json!({
            "name": project.name,
            "auth_mode": auth_mode_label(&auth),
        }),
    )
    .await;

    Ok(Json(AdminMutationResponse {
        status: "ok".to_string(),
    }))
}

#[derive(Debug, Serialize)]
struct AdminUserResponse {
    user_id: String,
    email: Option<String>,
    display_name: Option<String>,
    auth_provider: String,
    external_subject: Option<String>,
    is_service_account: bool,
    disabled: bool,
    active_project_count: i64,
    active_session_count: i64,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Serialize)]
struct AdminListUsersResponse {
    users: Vec<AdminUserResponse>,
}

#[derive(Debug, Deserialize)]
struct AdminMembershipsQuery {
    #[serde(default)]
    include_revoked: bool,
}

#[derive(Debug, Serialize)]
struct AdminUserMembershipResponse {
    project_id: String,
    project_name: String,
    role: String,
    granted_by_user_id: Option<String>,
    created_at: String,
    revoked_at: Option<String>,
}

#[derive(Debug, Serialize)]
struct AdminListUserMembershipsResponse {
    user_id: String,
    memberships: Vec<AdminUserMembershipResponse>,
}

#[derive(Debug, Deserialize)]
struct AdminSessionsQuery {
    user_id: Option<String>,
    #[serde(default)]
    include_revoked: bool,
}

#[derive(Debug, Deserialize)]
struct AdminAuditEventsQuery {
    project_id: Option<String>,
    user_id: Option<String>,
    key_id: Option<String>,
    action: Option<String>,
    outcome: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Serialize)]
struct AdminSessionResponse {
    session_id: String,
    user_id: String,
    created_at: String,
    last_seen_at: Option<String>,
    expires_at: String,
    revoked_at: Option<String>,
    client_ip: Option<String>,
    user_agent: Option<String>,
}

#[derive(Debug, Serialize)]
struct AdminListSessionsResponse {
    sessions: Vec<AdminSessionResponse>,
}

#[derive(Debug, Serialize)]
struct AdminAuditEventResponse {
    id: i64,
    occurred_at: String,
    actor_user_id: Option<String>,
    actor_key_id: Option<String>,
    project_id: Option<String>,
    run_id: Option<String>,
    action: String,
    resource_type: String,
    resource_id: Option<String>,
    outcome: String,
    request_id: Option<String>,
    client_ip: Option<String>,
    user_agent: Option<String>,
    metadata: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct AdminListAuditEventsResponse {
    events: Vec<AdminAuditEventResponse>,
}

#[derive(Debug, Serialize)]
struct AdminMutationResponse {
    status: String,
}

fn admin_user_response_from_row(row: UserRow) -> AdminUserResponse {
    AdminUserResponse {
        user_id: row.id,
        email: row.email,
        display_name: row.display_name,
        auth_provider: row.auth_provider,
        external_subject: row.external_subject,
        is_service_account: row.is_service_account,
        disabled: row.disabled_at.is_some(),
        active_project_count: row.active_project_count,
        active_session_count: row.active_session_count,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

fn admin_membership_response_from_row(
    row: UserProjectMembershipRow,
) -> AdminUserMembershipResponse {
    AdminUserMembershipResponse {
        project_id: row.project_id,
        project_name: row.project_name,
        role: row.role,
        granted_by_user_id: row.granted_by_user_id,
        created_at: row.created_at,
        revoked_at: row.revoked_at,
    }
}

fn admin_session_response_from_row(row: AuthSessionAdminRow) -> AdminSessionResponse {
    AdminSessionResponse {
        session_id: row.id,
        user_id: row.user_id,
        created_at: row.created_at,
        last_seen_at: row.last_seen_at,
        expires_at: row.expires_at,
        revoked_at: row.revoked_at,
        client_ip: row.client_ip,
        user_agent: row.user_agent,
    }
}

fn admin_audit_event_response_from_row(row: AuditEventRow) -> AdminAuditEventResponse {
    let metadata = serde_json::from_str::<serde_json::Value>(&row.metadata)
        .unwrap_or_else(|_| serde_json::json!({ "raw": row.metadata }));

    AdminAuditEventResponse {
        id: row.id,
        occurred_at: row.occurred_at,
        actor_user_id: row.actor_user_id,
        actor_key_id: row.actor_key_id,
        project_id: row.project_id,
        run_id: row.run_id,
        action: row.action,
        resource_type: row.resource_type,
        resource_id: row.resource_id,
        outcome: row.outcome,
        request_id: row.request_id,
        client_ip: row.client_ip,
        user_agent: row.user_agent,
        metadata,
    }
}

async fn http_admin_list_users(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
) -> Result<Json<AdminListUsersResponse>, (StatusCode, String)> {
    require_platform_admin_access(&state, &auth, "admin.users.list", "user", None).await?;

    let users = state
        .sqlite_store
        .list_users()
        .await
        .map_err(internal_error)?
        .into_iter()
        .map(admin_user_response_from_row)
        .collect();

    emit_audit_event(
        &state,
        Some(&auth),
        None,
        None,
        "admin.users.list",
        "user",
        None,
        "success",
        serde_json::json!({}),
    )
    .await;

    Ok(Json(AdminListUsersResponse { users }))
}

async fn http_admin_list_user_memberships(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    axum::extract::Path(user_id): axum::extract::Path<String>,
    axum::extract::Query(query): axum::extract::Query<AdminMembershipsQuery>,
) -> Result<Json<AdminListUserMembershipsResponse>, (StatusCode, String)> {
    validate_path_id(&user_id, "user_id")?;
    require_platform_admin_access(
        &state,
        &auth,
        "admin.user.memberships.list",
        "user",
        Some(&user_id),
    )
    .await?;

    let user_exists = state
        .sqlite_store
        .get_user_by_id(&user_id)
        .await
        .map_err(internal_error)?
        .is_some();
    if !user_exists {
        return Err((
            StatusCode::NOT_FOUND,
            format!("User not found: '{user_id}'"),
        ));
    }

    let memberships = state
        .sqlite_store
        .list_user_project_memberships(&user_id, query.include_revoked)
        .await
        .map_err(internal_error)?
        .into_iter()
        .map(admin_membership_response_from_row)
        .collect();

    emit_audit_event(
        &state,
        Some(&auth),
        None,
        None,
        "admin.user.memberships.list",
        "user",
        Some(&user_id),
        "success",
        serde_json::json!({
            "include_revoked": query.include_revoked,
        }),
    )
    .await;

    Ok(Json(AdminListUserMembershipsResponse {
        user_id,
        memberships,
    }))
}

async fn http_admin_disable_user(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    axum::extract::Path(user_id): axum::extract::Path<String>,
) -> Result<Json<AdminUserResponse>, (StatusCode, String)> {
    validate_path_id(&user_id, "user_id")?;
    require_platform_admin_access(&state, &auth, "admin.user.disable", "user", Some(&user_id))
        .await?;

    let user_exists = state
        .sqlite_store
        .get_user_by_id(&user_id)
        .await
        .map_err(internal_error)?
        .is_some();
    if !user_exists {
        return Err((
            StatusCode::NOT_FOUND,
            format!("User not found: '{user_id}'"),
        ));
    }

    state
        .sqlite_store
        .set_user_disabled(&user_id, true)
        .await
        .map_err(internal_error)?;

    // Invalidate all active sessions for the disabled user so existing tokens
    // cannot be used after the account is disabled.
    state
        .sqlite_store
        .revoke_all_sessions_for_user(&user_id)
        .await
        .map_err(internal_error)?;

    let user = state
        .sqlite_store
        .get_user_by_id(&user_id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!("User not found: '{user_id}'"),
            )
        })?;

    emit_audit_event(
        &state,
        Some(&auth),
        None,
        None,
        "admin.user.disable",
        "user",
        Some(&user_id),
        "success",
        serde_json::json!({}),
    )
    .await;

    Ok(Json(admin_user_response_from_row(user)))
}

async fn http_admin_enable_user(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    axum::extract::Path(user_id): axum::extract::Path<String>,
) -> Result<Json<AdminUserResponse>, (StatusCode, String)> {
    validate_path_id(&user_id, "user_id")?;
    require_platform_admin_access(&state, &auth, "admin.user.enable", "user", Some(&user_id))
        .await?;

    let user_exists = state
        .sqlite_store
        .get_user_by_id(&user_id)
        .await
        .map_err(internal_error)?
        .is_some();
    if !user_exists {
        return Err((
            StatusCode::NOT_FOUND,
            format!("User not found: '{user_id}'"),
        ));
    }

    state
        .sqlite_store
        .set_user_disabled(&user_id, false)
        .await
        .map_err(internal_error)?;

    let user = state
        .sqlite_store
        .get_user_by_id(&user_id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!("User not found: '{user_id}'"),
            )
        })?;

    emit_audit_event(
        &state,
        Some(&auth),
        None,
        None,
        "admin.user.enable",
        "user",
        Some(&user_id),
        "success",
        serde_json::json!({}),
    )
    .await;

    Ok(Json(admin_user_response_from_row(user)))
}

async fn http_admin_list_sessions(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    axum::extract::Query(query): axum::extract::Query<AdminSessionsQuery>,
) -> Result<Json<AdminListSessionsResponse>, (StatusCode, String)> {
    require_platform_admin_access(&state, &auth, "admin.sessions.list", "auth_session", None)
        .await?;

    if let Some(ref user_id) = query.user_id {
        let user_exists = state
            .sqlite_store
            .get_user_by_id(user_id)
            .await
            .map_err(internal_error)?
            .is_some();
        if !user_exists {
            return Err((
                StatusCode::NOT_FOUND,
                format!("User not found: '{user_id}'"),
            ));
        }
    }

    let sessions = state
        .sqlite_store
        .list_auth_sessions_for_admin(query.user_id.as_deref(), query.include_revoked)
        .await
        .map_err(internal_error)?
        .into_iter()
        .map(admin_session_response_from_row)
        .collect();

    emit_audit_event(
        &state,
        Some(&auth),
        None,
        None,
        "admin.sessions.list",
        "auth_session",
        None,
        "success",
        serde_json::json!({
            "user_id": query.user_id,
            "include_revoked": query.include_revoked,
        }),
    )
    .await;

    Ok(Json(AdminListSessionsResponse { sessions }))
}

async fn http_admin_revoke_session(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> Result<Json<AdminMutationResponse>, (StatusCode, String)> {
    validate_path_id(&session_id, "session_id")?;
    require_platform_admin_access(
        &state,
        &auth,
        "admin.session.revoke",
        "auth_session",
        Some(&session_id),
    )
    .await?;

    let revoked = state
        .sqlite_store
        .revoke_auth_session_by_id(&session_id)
        .await
        .map_err(internal_error)?;
    if !revoked {
        return Err((
            StatusCode::NOT_FOUND,
            format!("Session not found or already revoked: '{session_id}'"),
        ));
    }

    emit_audit_event(
        &state,
        Some(&auth),
        None,
        None,
        "admin.session.revoke",
        "auth_session",
        Some(&session_id),
        "success",
        serde_json::json!({}),
    )
    .await;

    Ok(Json(AdminMutationResponse {
        status: "ok".to_string(),
    }))
}

async fn http_admin_list_audit_events(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    axum::extract::Query(query): axum::extract::Query<AdminAuditEventsQuery>,
) -> Result<Json<AdminListAuditEventsResponse>, (StatusCode, String)> {
    require_platform_admin_access(&state, &auth, "admin.audit.list", "audit_event", None).await?;

    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let events = state
        .sqlite_store
        .list_audit_events_for_admin(
            query.project_id.as_deref(),
            query.user_id.as_deref(),
            query.key_id.as_deref(),
            query.action.as_deref(),
            query.outcome.as_deref(),
            limit,
        )
        .await
        .map_err(internal_error)?
        .into_iter()
        .map(admin_audit_event_response_from_row)
        .collect();

    emit_audit_event(
        &state,
        Some(&auth),
        None,
        None,
        "admin.audit.list",
        "audit_event",
        None,
        "success",
        serde_json::json!({
            "project_id": query.project_id,
            "user_id": query.user_id,
            "key_id": query.key_id,
            "action": query.action,
            "outcome": query.outcome,
            "limit": limit,
        }),
    )
    .await;

    Ok(Json(AdminListAuditEventsResponse { events }))
}

/// Rotate the bootstrap API key without a server restart.
async fn http_admin_rotate_bootstrap_key(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    require_platform_admin_access(
        &state,
        &auth,
        "admin.bootstrap_key.rotate",
        "bootstrap_key",
        None,
    )
    .await?;

    let new_raw_key = state.key_store.rotate_bootstrap_key().await.map_err(|e| {
        warn!("Failed to rotate bootstrap key: {e}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to rotate bootstrap key".to_string(),
        )
    })?;

    emit_audit_event(
        &state,
        Some(&auth),
        None,
        None,
        "admin.bootstrap_key.rotate",
        "bootstrap_key",
        Some("bootstrap"),
        "success",
        serde_json::json!({}),
    )
    .await;

    Ok(Json(serde_json::json!({
        "status": "rotated",
        "new_api_key": new_raw_key,
        "expires_in_seconds": state.key_store.bootstrap_key_ttl_seconds(),
        "message": "Bootstrap key rotated with short TTL. Update MLRUNX_API_KEY immediately and restart SDK clients."
    })))
}

/// Initialize a run via HTTP (for SDK HTTP transport).
#[allow(clippy::too_many_lines)]
async fn http_init_run(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(req): Json<InitRunHttpRequest>,
) -> Result<Json<InitRunHttpResponse>, (StatusCode, String)> {
    let run_id = req
        .run_id
        .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());

    // Check if run exists in SQLite (idempotent)
    if state
        .sqlite_store
        .run_exists(&run_id)
        .await
        .unwrap_or(false)
    {
        // Verify the caller can access this existing run and mutate it.
        if let Ok(existing_run) = state.sqlite_store.get_run(&run_id).await {
            require_endpoint_access(
                &state,
                &auth,
                EndpointRbacTier::Write,
                Some(&existing_run.project_id),
                Some(&run_id),
                "run.init",
                "run",
                Some(&run_id),
            )
            .await?;
            require_ui_run_owner(&auth, &existing_run)?;
            require_api_key_run_owner_for_mutation(&auth, &existing_run, "reinitialize")?;
        }
        return Ok(Json(InitRunHttpResponse {
            run_id,
            offline: false,
        }));
    }

    // Phase 1: explicit project boundary.
    // Run init must target an existing project_id; project creation is handled via /api/v1/projects.
    let project_id = req.project_id.as_ref().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "project_id is required. Create a project first via POST /api/v1/projects.".to_string(),
        )
    })?;
    let resolved_project_name = state
        .sqlite_store
        .get_project_name_by_id(project_id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!("Project not found: '{project_id}'"),
            )
        })?;
    if let Some(ref project_name) = req.project {
        if project_name != &resolved_project_name {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("project_id '{project_id}' does not match project name '{project_name}'"),
            ));
        }
    }

    // Enforce project scope and write permission
    require_endpoint_access(
        &state,
        &auth,
        EndpointRbacTier::Write,
        Some(project_id),
        Some(&run_id),
        "run.init",
        "run",
        Some(&run_id),
    )
    .await?;

    let max_tag_value_len = state.cardinality_tracker.config().max_tag_value_length;
    let mut validated_init_tags: Vec<(String, String)> = Vec::new();
    if let Some(tags) = &req.tags {
        for (raw_key, raw_value) in tags {
            let canonical_key = canonicalize_metadata_key(raw_key, "tags").map_err(|err| {
                (
                    StatusCode::BAD_REQUEST,
                    format!("Invalid initial tag '{raw_key}': {err}"),
                )
            })?;
            validate_tag_value(raw_value, max_tag_value_len).map_err(|err| {
                (
                    StatusCode::BAD_REQUEST,
                    format!("Invalid initial tag '{raw_key}': {err}"),
                )
            })?;
            validated_init_tags.push((canonical_key, raw_value.clone()));
        }
    }

    let mut validated_init_params: Vec<(String, String)> = Vec::new();
    if let Some(config) = &req.config {
        for (raw_key, raw_value) in config {
            let canonical_key =
                canonicalize_metadata_key(raw_key, "parameters").map_err(|err| {
                    (
                        StatusCode::BAD_REQUEST,
                        format!("Invalid config key '{raw_key}': {err}"),
                    )
                })?;
            let value_text = stringify_config_param_value(raw_value).map_err(|err| {
                (
                    StatusCode::BAD_REQUEST,
                    format!("Invalid config value for '{raw_key}': {err}"),
                )
            })?;
            validated_init_params.push((canonical_key, value_text));
        }
    }
    let normalized_init_tags: Option<std::collections::HashMap<String, String>> =
        if validated_init_tags.is_empty() {
            None
        } else {
            Some(validated_init_tags.iter().cloned().collect())
        };

    let mut created_by_user_id = None;
    let mut created_by_key_id = None;
    if auth.is_ui_jwt() {
        created_by_user_id = auth_user_id(&auth);
    } else if !auth.is_dev_mode {
        created_by_key_id = Some(auth.api_key.id.clone());
        match state
            .sqlite_store
            .get_api_key_by_hash(&auth.api_key.key_hash)
            .await
        {
            Ok(Some(key_row)) => {
                created_by_user_id = key_row.created_by_user_id;
            }
            Ok(None) => {}
            Err(err) => {
                warn!(
                    error = %err,
                    key_id = %auth.api_key.id,
                    "Failed to resolve API key owner during run initialization"
                );
            }
        }
    }

    // Create run in SQLite
    state
        .sqlite_store
        .create_run(
            &run_id,
            project_id,
            req.name.as_deref(),
            created_by_key_id.as_deref(),
            created_by_user_id.as_deref(),
        )
        .await
        .map_err(internal_error)?;

    // Set initial tags if provided
    if !validated_init_tags.is_empty() {
        state
            .sqlite_store
            .set_tags(&run_id, &validated_init_tags)
            .await
            .map_err(internal_error)?;
    }

    if !validated_init_params.is_empty() {
        state
            .sqlite_store
            .insert_params(&run_id, &validated_init_params)
            .await
            .map_err(internal_error)?;
    }

    if let Err(e) = state
        .sqlite_store
        .insert_run_events(
            &run_id,
            &[RunEventInput {
                level: "info".to_string(),
                source: "system".to_string(),
                message: "Run initialized".to_string(),
                step: Some(0),
                timestamp: None,
            }],
        )
        .await
    {
        warn!(run_id = %run_id, error = %e, "Failed to persist run init event");
    }
    maybe_shadow_write_run_to_postgres(
        &run_id,
        project_id,
        req.name.as_deref(),
        normalized_init_tags.as_ref(),
    )
    .await;

    info!(
        run_id = %run_id,
        project_id = %project_id,
        project_name = %resolved_project_name,
        "HTTP: Initialized run (SQLite)"
    );

    emit_audit_event(
        &state,
        Some(&auth),
        Some(project_id),
        Some(&run_id),
        "run.init",
        "run",
        Some(&run_id),
        "success",
        serde_json::json!({
            "project_name": resolved_project_name,
            "project_id": project_id,
            "created_by_user_id": created_by_user_id,
            "initial_tag_count": validated_init_tags.len(),
            "initial_param_count": validated_init_params.len(),
            "source": "http_init_run",
        }),
    )
    .await;

    Ok(Json(InitRunHttpResponse {
        run_id,
        offline: false,
    }))
}

/// Request to ingest a batch via HTTP.
#[derive(Debug, Deserialize)]
struct IngestBatchHttpRequest {
    run_id: String,
    /// SDK-provided batch identifier for idempotency
    batch_id: Option<String>,
    /// Sequence number for ordering (optional)
    seq: Option<i64>,
    #[serde(default)]
    metrics: Vec<MetricData>,
    #[serde(default)]
    params: Vec<ParamData>,
    #[serde(default)]
    tags: Vec<TagData>,
    #[serde(default)]
    events: Vec<LogEventData>,
    #[allow(dead_code)]
    timestamp: Option<f64>,
    #[allow(dead_code)]
    stats: Option<BatchStats>,
}

#[derive(Debug, Deserialize)]
struct MetricData {
    name: String,
    value: MetricValue,
    step: i64,
    timestamp: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum MetricValue {
    Scalar(f64),
    Array(Vec<f64>),
}

#[derive(Debug, Deserialize)]
struct ParamData {
    name: String,
    value: String,
}

#[derive(Debug, Deserialize)]
struct TagData {
    key: String,
    value: String,
}

#[derive(Debug, Deserialize)]
struct LogEventData {
    level: Option<String>,
    source: Option<String>,
    message: String,
    step: Option<i64>,
    timestamp: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[allow(clippy::struct_field_names)]
struct BatchStats {
    metric_count: Option<i64>,
    param_count: Option<i64>,
    tag_count: Option<i64>,
    coalesced_count: Option<i64>,
}

#[derive(Debug, Serialize)]
struct IngestBatchHttpResponse {
    status: String,
    accepted: i64,
    /// Whether this was a duplicate batch
    duplicate: bool,
    /// Warnings about the batch (e.g., out of order)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,
}

fn normalize_run_event_level(raw: Option<&str>) -> String {
    match raw
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("info")
        .to_ascii_lowercase()
        .as_str()
    {
        "debug" => "debug".to_string(),
        "warn" | "warning" => "warn".to_string(),
        "error" => "error".to_string(),
        _ => "info".to_string(),
    }
}

fn normalize_run_event_source(raw: Option<&str>) -> String {
    let normalized = raw
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("sdk");
    let max_len = 64usize;
    if normalized.len() <= max_len {
        normalized.to_string()
    } else {
        normalized.chars().take(max_len).collect()
    }
}

fn sanitize_run_event_message(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let max_len = RUN_EVENT_MAX_LEN;
    if trimmed.len() <= max_len {
        Some(trimmed.to_string())
    } else {
        Some(trimmed.chars().take(max_len).collect())
    }
}

#[derive(Debug)]
struct ExpandedMetricData {
    name: String,
    value: f64,
    step: i64,
    timestamp: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StructuredEnvelope {
    kind: String,
    #[serde(default)]
    payload: serde_json::Value,
    #[serde(default)]
    truncated: bool,
}

fn json_serialized_size(value: &serde_json::Value) -> Result<usize, String> {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len())
        .map_err(|err| format!("failed to serialize JSON payload: {err}"))
}

fn validate_structured_json_tree(value: &serde_json::Value, depth: usize) -> Result<(), String> {
    if depth > STRUCTURED_JSON_MAX_DEPTH {
        return Err(format!(
            "structured payload exceeds max depth {STRUCTURED_JSON_MAX_DEPTH}"
        ));
    }

    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                let lowered = key.to_ascii_lowercase();
                if lowered == "__proto__" || lowered == "constructor" || lowered == "prototype" {
                    return Err("structured payload contains forbidden object key".to_string());
                }
                if key.len() > METADATA_MAX_SEGMENT_LEN {
                    return Err(format!(
                        "structured payload key '{key}' exceeds max length {METADATA_MAX_SEGMENT_LEN}"
                    ));
                }
                validate_structured_json_tree(child, depth + 1)?;
            }
            Ok(())
        }
        serde_json::Value::Array(items) => {
            for child in items {
                validate_structured_json_tree(child, depth + 1)?;
            }
            Ok(())
        }
        serde_json::Value::String(text) => {
            if text.len() > STRUCTURED_JSON_MAX_STRING_LEN {
                return Err(format!(
                    "structured payload string exceeds max length {STRUCTURED_JSON_MAX_STRING_LEN}"
                ));
            }
            let lowered = text.to_ascii_lowercase();
            if lowered.contains("<script") || lowered.contains("javascript:") {
                return Err("structured payload contains unsafe script-like content".to_string());
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn sanitize_structured_image_payload(
    payload: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let object = payload
        .as_object()
        .ok_or_else(|| "image payload must be a JSON object".to_string())?;

    let name = object
        .get("name")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "image payload.name is required".to_string())?;
    if name.len() > 256 {
        return Err("image payload.name exceeds max length 256".to_string());
    }

    let path = object
        .get("path")
        .or_else(|| object.get("uri"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "image payload.path is required".to_string())?;
    if path.len() > 4096 {
        return Err("image payload.path exceeds max length 4096".to_string());
    }

    let caption = object
        .get("caption")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(std::string::ToString::to_string);
    if let Some(text) = caption.as_deref()
        && text.len() > 1024
    {
        return Err("image payload.caption exceeds max length 1024".to_string());
    }

    let metadata = object
        .get("metadata")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    validate_structured_json_tree(&metadata, 1)?;
    if json_serialized_size(&metadata)? > IMAGE_METADATA_MAX_BYTES {
        return Err(format!(
            "image payload.metadata exceeds max size {IMAGE_METADATA_MAX_BYTES} bytes"
        ));
    }

    Ok(serde_json::json!({
        "name": name,
        "path": path,
        "caption": caption,
        "metadata": metadata,
    }))
}

fn sanitize_structured_chart_payload(
    payload: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let object = payload
        .as_object()
        .ok_or_else(|| "chart payload must be a JSON object".to_string())?;

    let name = object
        .get("name")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "chart payload.name is required".to_string())?;
    if name.len() > 256 {
        return Err("chart payload.name exceeds max length 256".to_string());
    }

    let chart_type = object
        .get("chart_type")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("custom");
    if chart_type.len() > 64 {
        return Err("chart payload.chart_type exceeds max length 64".to_string());
    }

    let renderer_hint = object
        .get("renderer_hint")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("auto");
    if renderer_hint.len() > 64 {
        return Err("chart payload.renderer_hint exceeds max length 64".to_string());
    }

    let data = object
        .get("data")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let layout = object
        .get("layout")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let options = object
        .get("options")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let metadata = object
        .get("metadata")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));

    validate_structured_json_tree(&data, 1)?;
    validate_structured_json_tree(&layout, 1)?;
    validate_structured_json_tree(&options, 1)?;
    validate_structured_json_tree(&metadata, 1)?;

    if json_serialized_size(&data)? > CHART_DATA_MAX_BYTES {
        return Err(format!(
            "chart payload.data exceeds max size {CHART_DATA_MAX_BYTES} bytes"
        ));
    }
    if json_serialized_size(&layout)? > CHART_LAYOUT_MAX_BYTES {
        return Err(format!(
            "chart payload.layout exceeds max size {CHART_LAYOUT_MAX_BYTES} bytes"
        ));
    }
    if json_serialized_size(&options)? > CHART_OPTIONS_MAX_BYTES {
        return Err(format!(
            "chart payload.options exceeds max size {CHART_OPTIONS_MAX_BYTES} bytes"
        ));
    }
    if json_serialized_size(&metadata)? > CHART_METADATA_MAX_BYTES {
        return Err(format!(
            "chart payload.metadata exceeds max size {CHART_METADATA_MAX_BYTES} bytes"
        ));
    }

    Ok(serde_json::json!({
        "name": name,
        "chart_type": chart_type,
        "renderer_hint": renderer_hint,
        "data": data,
        "layout": layout,
        "options": options,
        "metadata": metadata,
    }))
}

fn sanitize_structured_event_message(raw_message: &str) -> Result<Option<String>, String> {
    let parsed: serde_json::Value = match serde_json::from_str(raw_message) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };

    let envelope: StructuredEnvelope = match serde_json::from_value(parsed) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };

    match envelope.kind.as_str() {
        "image" => {
            if envelope.truncated {
                return Err("image payload was truncated and cannot be ingested".to_string());
            }
            let sanitized_payload = sanitize_structured_image_payload(&envelope.payload)?;
            let normalized = serde_json::json!({
                "kind": "image",
                "payload": sanitized_payload,
            });
            let encoded = serde_json::to_string(&normalized)
                .map_err(|err| format!("failed to encode image payload: {err}"))?;
            if encoded.len() > RUN_EVENT_MAX_LEN {
                return Err(format!(
                    "image structured message exceeds max length {RUN_EVENT_MAX_LEN}"
                ));
            }
            Ok(Some(encoded))
        }
        "chart" => {
            if envelope.truncated {
                return Err("chart payload was truncated and cannot be ingested".to_string());
            }
            let sanitized_payload = sanitize_structured_chart_payload(&envelope.payload)?;
            let normalized = serde_json::json!({
                "kind": "chart",
                "payload": sanitized_payload,
            });
            let encoded = serde_json::to_string(&normalized)
                .map_err(|err| format!("failed to encode chart payload: {err}"))?;
            if encoded.len() > RUN_EVENT_MAX_LEN {
                return Err(format!(
                    "chart structured message exceeds max length {RUN_EVENT_MAX_LEN}"
                ));
            }
            Ok(Some(encoded))
        }
        _ => Ok(None),
    }
}

fn parse_structured_payload_for_kind(
    message: &str,
    expected_kind: &str,
) -> Option<serde_json::Value> {
    let envelope: StructuredEnvelope = serde_json::from_str(message).ok()?;
    if envelope.kind == expected_kind && !envelope.truncated {
        Some(envelope.payload)
    } else {
        None
    }
}

/// Ingest a batch of events via HTTP (for SDK HTTP transport).
#[allow(clippy::too_many_lines)]
async fn http_ingest_batch(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(req): Json<IngestBatchHttpRequest>,
) -> Result<Json<IngestBatchHttpResponse>, (StatusCode, String)> {
    // Resolve run project first and fail closed when the run does not exist.
    let run = state
        .sqlite_store
        .get_run(&req.run_id)
        .await
        .map_err(|e| match e {
            storage::SqliteError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            _ => internal_error(e),
        })?;

    // Verify the caller can access the run's project and has write scope.
    require_endpoint_access(
        &state,
        &auth,
        EndpointRbacTier::Write,
        Some(&run.project_id),
        Some(&req.run_id),
        "run.ingest",
        "run",
        Some(&req.run_id),
    )
    .await?;
    require_ui_run_owner(&auth, &run)?;
    require_api_key_run_owner_for_mutation(&auth, &run, "ingest to")?;

    let max_tag_value_len = state.cardinality_tracker.config().max_tag_value_length;
    let mut validated_params: Vec<(String, String)> = Vec::with_capacity(req.params.len());
    for param in &req.params {
        let canonical_name =
            canonicalize_metadata_key(&param.name, "parameters").map_err(|err| {
                (
                    StatusCode::BAD_REQUEST,
                    format!("Invalid param key '{}': {err}", param.name),
                )
            })?;
        validate_param_value(&param.value).map_err(|err| {
            (
                StatusCode::BAD_REQUEST,
                format!("Invalid param value for '{}': {err}", param.name),
            )
        })?;
        validated_params.push((canonical_name, param.value.clone()));
    }

    let mut validated_tags: Vec<(String, String)> = Vec::with_capacity(req.tags.len());
    for tag in &req.tags {
        let canonical_key = canonicalize_metadata_key(&tag.key, "tags").map_err(|err| {
            (
                StatusCode::BAD_REQUEST,
                format!("Invalid tag key '{}': {err}", tag.key),
            )
        })?;
        validate_tag_value(&tag.value, max_tag_value_len).map_err(|err| {
            (
                StatusCode::BAD_REQUEST,
                format!("Invalid tag value for '{}': {err}", tag.key),
            )
        })?;
        validated_tags.push((canonical_key, tag.value.clone()));
    }

    let mut expanded_metrics: Vec<ExpandedMetricData> = Vec::new();
    for metric in &req.metrics {
        let base_name = metric.name.trim();
        if base_name.is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                "metric name cannot be empty".to_string(),
            ));
        }

        match &metric.value {
            MetricValue::Scalar(value) => {
                expanded_metrics.push(ExpandedMetricData {
                    name: base_name.to_string(),
                    value: *value,
                    step: metric.step,
                    timestamp: metric.timestamp,
                });
            }
            MetricValue::Array(values) => {
                if values.len() > METRIC_ARRAY_MAX_LEN {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        format!(
                            "metric array '{base_name}' exceeds max length {METRIC_ARRAY_MAX_LEN}"
                        ),
                    ));
                }
                for (idx, value) in values.iter().enumerate() {
                    expanded_metrics.push(ExpandedMetricData {
                        name: format!("{base_name}[{idx}]"),
                        value: *value,
                        step: metric.step,
                        timestamp: metric.timestamp,
                    });
                }
            }
        }
    }

    // Generate batch_id if not provided
    let batch_id = req
        .batch_id
        .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
    let seq = req.seq.unwrap_or(0);

    // Convert request data for hashing
    let metric_payloads: Vec<MetricPayload> = expanded_metrics
        .iter()
        .map(|metric| MetricPayload {
            name: metric.name.clone(),
            value: metric.value,
            step: metric.step,
        })
        .collect();

    let param_payloads: Vec<ParamPayload> = validated_params
        .iter()
        .map(|(name, value)| ParamPayload {
            name: name.clone(),
            value: value.clone(),
        })
        .collect();

    let tag_payloads: Vec<TagPayload> = validated_tags
        .iter()
        .map(|(key, value)| TagPayload {
            key: key.clone(),
            value: value.clone(),
        })
        .collect();

    let event_payloads: Vec<EventPayload> = req
        .events
        .iter()
        .map(|event| EventPayload {
            level: normalize_run_event_level(event.level.as_deref()),
            source: normalize_run_event_source(event.source.as_deref()),
            message: event.message.clone(),
            step: event.step,
            timestamp: event.timestamp,
        })
        .collect();

    // Compute payload hash for idempotency
    let payload_hash = compute_payload_hash(
        &metric_payloads,
        &param_payloads,
        &tag_payloads,
        &event_payloads,
    );

    // Check and record for idempotency
    let metric_count = expanded_metrics.len();
    let param_count = validated_params.len();
    let tag_count = validated_tags.len();
    let event_count = req.events.len();

    let project_id = run.project_id.clone();

    let idempotency_result = state
        .idempotency_store
        .check_and_record(
            &project_id,
            &req.run_id,
            &batch_id,
            seq,
            &payload_hash,
            usize_to_i32_saturating(metric_count),
            usize_to_i32_saturating(param_count),
            usize_to_i32_saturating(tag_count),
            usize_to_i32_saturating(event_count),
        )
        .await;

    // Handle idempotency results
    let mut warnings = Vec::new();

    match &idempotency_result {
        IdempotencyResult::Duplicate => {
            // Duplicate batch - return success without processing
            return Ok(Json(IngestBatchHttpResponse {
                status: "ok".to_string(),
                accepted: 0,
                duplicate: true,
                warnings: vec![],
            }));
        }
        IdempotencyResult::Conflict {
            expected_hash,
            actual_hash,
        } => {
            // Conflicting batch - error
            return Err((
                StatusCode::CONFLICT,
                format!(
                    "Batch {batch_id} conflicts with existing batch (expected hash {expected_hash}, got {actual_hash})"
                ),
            ));
        }
        IdempotencyResult::OutOfOrder {
            expected_seq,
            actual_seq,
        } => {
            warnings.push(format!(
                "Batch received out of order (expected seq >= {expected_seq}, got {actual_seq})"
            ));
        }
        IdempotencyResult::New => {
            // New batch - proceed normally
        }
    }

    // Validate cardinality limits
    let tags_for_validation: Vec<(String, String)> = validated_tags.clone();
    let metric_names: Vec<String> = expanded_metrics
        .iter()
        .map(|metric| metric.name.clone())
        .collect();

    let validation = state
        .cardinality_tracker
        .validate_batch(
            &project_id,
            &req.run_id,
            &tags_for_validation,
            &metric_names,
        )
        .await;

    // Add cardinality warnings
    warnings.extend(validation.warnings.clone());

    // Filter metrics and tags based on validation
    let accepted_tags = &validation.accepted_tags;
    let accepted_metrics: std::collections::HashSet<_> =
        validation.accepted_metrics.iter().collect();

    // Validate run status from SQLite (single source of truth for HTTP paths).
    let run = state
        .sqlite_store
        .get_run(&req.run_id)
        .await
        .map_err(|e| match e {
            storage::SqliteError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            _ => internal_error(e),
        })?;

    if !run.status.eq_ignore_ascii_case("running") {
        return Err((
            StatusCode::PRECONDITION_FAILED,
            format!("Run {} is not running", req.run_id),
        ));
    }

    // Filter metrics for cardinality acceptance plus finite numeric payloads.
    // Without this, a single NaN/inf can poison the whole insert batch.
    let mut non_finite_metric_count = 0usize;
    let sqlite_metrics: Vec<MetricRow> = expanded_metrics
        .iter()
        .filter(|metric| accepted_metrics.contains(&metric.name))
        .filter_map(|metric| {
            let finite_value = metric.value.is_finite();
            let finite_timestamp = metric.timestamp.is_none_or(f64::is_finite);
            if finite_value && finite_timestamp {
                Some(MetricRow {
                    name: metric.name.clone(),
                    step: metric.step,
                    value: metric.value,
                    timestamp: metric.timestamp,
                })
            } else {
                non_finite_metric_count += 1;
                None
            }
        })
        .collect();

    if non_finite_metric_count > 0 {
        warnings.push(format!(
            "Dropped {non_finite_metric_count} metrics with non-finite values/timestamps"
        ));
    }

    let accepted_metric_count = sqlite_metrics.len();
    let accepted_tag_count = accepted_tags.len();

    // Persist metrics to SQLite.
    // Fail closed on storage errors to avoid reporting successful ingestion when metrics are missing.
    if accepted_metric_count > 0 {
        state
            .sqlite_store
            .insert_metrics(&req.run_id, &sqlite_metrics)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to persist metrics to SQLite: {e}"),
                )
            })?;

        state
            .sqlite_store
            .increment_metrics_count(&req.run_id, usize_to_i64_saturating(accepted_metric_count))
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to update metrics count in SQLite: {e}"),
                )
            })?;
    }

    // Persist tags to SQLite
    if !accepted_tags.is_empty() {
        let tag_pairs: Vec<(String, String)> = accepted_tags
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if let Err(e) = state.sqlite_store.set_tags(&req.run_id, &tag_pairs).await {
            warn!(error = %e, "Failed to persist tags to SQLite");
        }
    }

    // Persist params to SQLite
    if param_count > 0 {
        if let Err(e) = state
            .sqlite_store
            .insert_params(&req.run_id, &validated_params)
            .await
        {
            warn!(error = %e, "Failed to persist params to SQLite");
        }
    }

    let mut dropped_event_count = 0usize;
    let sqlite_events: Vec<RunEventInput> = req
        .events
        .iter()
        .filter_map(|event| {
            let Some(mut message) = sanitize_run_event_message(&event.message) else {
                dropped_event_count += 1;
                return None;
            };
            let timestamp = match event.timestamp {
                Some(value) if !value.is_finite() => {
                    dropped_event_count += 1;
                    return None;
                }
                value => value,
            };

            match sanitize_structured_event_message(&message) {
                Ok(Some(sanitized_message)) => {
                    message = sanitized_message;
                }
                Ok(None) => {}
                Err(err) => {
                    dropped_event_count += 1;
                    warnings.push(format!("Dropped unsafe structured event: {err}"));
                    return None;
                }
            }

            Some(RunEventInput {
                level: normalize_run_event_level(event.level.as_deref()),
                source: normalize_run_event_source(event.source.as_deref()),
                message,
                step: event.step,
                timestamp,
            })
        })
        .collect();

    if dropped_event_count > 0 {
        warnings.push(format!(
            "Dropped {dropped_event_count} events with empty messages or non-finite timestamps"
        ));
    }

    let mut accepted_event_count = if sqlite_events.is_empty() {
        0usize
    } else {
        state
            .sqlite_store
            .insert_run_events(&req.run_id, &sqlite_events)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to persist run events to SQLite: {e}"),
                )
            })?
    };

    if !warnings.is_empty() {
        let warning_events: Vec<RunEventInput> = warnings
            .iter()
            .map(|warning| RunEventInput {
                level: "warn".to_string(),
                source: "ingest".to_string(),
                message: warning.clone(),
                step: None,
                timestamp: None,
            })
            .collect();

        match state
            .sqlite_store
            .insert_run_events(&req.run_id, &warning_events)
            .await
        {
            Ok(inserted) => {
                accepted_event_count += inserted;
            }
            Err(e) => {
                warn!(run_id = %req.run_id, error = %e, "Failed to persist warning events");
            }
        }
    }

    if param_count > 0 || accepted_tag_count > 0 {
        let updated_param_names: Vec<String> = validated_params
            .iter()
            .take(20)
            .map(|(name, _)| name.clone())
            .collect();
        let updated_tag_keys: Vec<String> = accepted_tags
            .iter()
            .take(20)
            .map(|(key, _)| key.clone())
            .collect();

        emit_audit_event(
            &state,
            Some(&auth),
            Some(&project_id),
            Some(&req.run_id),
            "run.metadata.update",
            "run",
            Some(&req.run_id),
            "success",
            serde_json::json!({
                "param_count": param_count,
                "tag_count": accepted_tag_count,
                "updated_param_names": updated_param_names,
                "updated_tag_keys": updated_tag_keys,
                "source": "http_ingest_batch",
            }),
        )
        .await;
    }

    let total = accepted_metric_count + param_count + accepted_tag_count + accepted_event_count;
    let dropped = validation.dropped_tags.len() + validation.dropped_metrics.len();

    tracing::debug!(
        run_id = %req.run_id,
        batch_id = %batch_id,
        seq = seq,
        metrics = accepted_metric_count,
        params = param_count,
        tags = accepted_tag_count,
        events = accepted_event_count,
        dropped = dropped,
        "HTTP: Ingested batch (SQLite)"
    );

    Ok(Json(IngestBatchHttpResponse {
        status: "ok".to_string(),
        accepted: usize_to_i64_saturating(total),
        duplicate: false,
        warnings,
    }))
}

/// Request to finish a run via HTTP.
#[derive(Debug, Deserialize)]
struct FinishRunHttpRequest {
    status: String,
}

#[derive(Debug, Serialize)]
struct FinishRunHttpResponse {
    status: String,
}

/// Finish a run via HTTP.
async fn http_finish_run(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    axum::extract::Path(run_id): axum::extract::Path<String>,
    Json(req): Json<FinishRunHttpRequest>,
) -> Result<Json<FinishRunHttpResponse>, (StatusCode, String)> {
    validate_path_id(&run_id, "run_id")?;
    // Verify the caller can access the run's project and has write scope
    let run = state
        .sqlite_store
        .get_run(&run_id)
        .await
        .map_err(|e| match e {
            storage::SqliteError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            _ => internal_error(e),
        })?;
    require_endpoint_access(
        &state,
        &auth,
        EndpointRbacTier::Write,
        Some(&run.project_id),
        Some(&run_id),
        "run.finish",
        "run",
        Some(&run_id),
    )
    .await?;
    require_ui_run_owner(&auth, &run)?;
    require_api_key_run_owner_for_mutation(&auth, &run, "finish")?;

    // Update in SQLite
    state
        .sqlite_store
        .finish_run(&run_id, &req.status)
        .await
        .map_err(internal_error)?;
    if let Err(e) = state
        .sqlite_store
        .insert_run_events(
            &run_id,
            &[RunEventInput {
                level: if req.status.eq_ignore_ascii_case("failed") {
                    "error".to_string()
                } else {
                    "info".to_string()
                },
                source: "system".to_string(),
                message: format!("Run marked as {}", req.status),
                step: None,
                timestamp: None,
            }],
        )
        .await
    {
        warn!(run_id = %run_id, error = %e, "Failed to persist run finish event");
    }
    maybe_shadow_finish_run_in_postgres(&run_id, &req.status).await;

    info!(run_id = %run_id, status = %req.status, "HTTP: Finished run (SQLite)");

    emit_audit_event(
        &state,
        Some(&auth),
        Some(&run.project_id),
        Some(&run_id),
        "run.finish",
        "run",
        Some(&run_id),
        "success",
        serde_json::json!({
            "status": req.status,
        }),
    )
    .await;

    Ok(Json(FinishRunHttpResponse {
        status: "ok".to_string(),
    }))
}

/// Response for deleting a run.
#[derive(Debug, Serialize)]
struct DeleteRunHttpResponse {
    status: String,
}

/// Delete a run and all its associated data.
async fn http_delete_run(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    axum::extract::Path(run_id): axum::extract::Path<String>,
) -> Result<Json<DeleteRunHttpResponse>, (StatusCode, String)> {
    validate_path_id(&run_id, "run_id")?;
    // Verify the caller can access the run's project and has write scope.
    // Ownership checks below prevent cross-user run deletion.
    let run = state
        .sqlite_store
        .get_run(&run_id)
        .await
        .map_err(|e| match e {
            storage::SqliteError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            _ => internal_error(e),
        })?;
    require_endpoint_access(
        &state,
        &auth,
        EndpointRbacTier::Write,
        Some(&run.project_id),
        Some(&run_id),
        "run.delete",
        "run",
        Some(&run_id),
    )
    .await?;
    require_ui_run_owner(&auth, &run)?;
    require_api_key_run_owner_for_mutation(&auth, &run, "delete")?;

    // Delete from SQLite (cascades to metrics, tags, params, batches)
    state
        .sqlite_store
        .delete_run(&run_id)
        .await
        .map_err(|e| match e {
            storage::SqliteError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            _ => internal_error(e),
        })?;

    info!(run_id = %run_id, "HTTP: Deleted run");

    emit_audit_event(
        &state,
        Some(&auth),
        Some(&run.project_id),
        Some(&run_id),
        "run.delete",
        "run",
        Some(&run_id),
        "success",
        serde_json::json!({}),
    )
    .await;

    Ok(Json(DeleteRunHttpResponse {
        status: "ok".to_string(),
    }))
}

// =============================================================================
// Key Management API Handlers
// =============================================================================

/// Request to create a new API key.
#[derive(Debug, Deserialize)]
struct CreateKeyRequest {
    /// Project to scope the key to (None = admin/global key)
    project_id: Option<String>,
    /// Human-readable name for the key
    name: Option<String>,
    /// Scopes: "admin", "write", "read"
    scopes: Vec<String>,
    /// Optional TTL in seconds (None = never expires)
    expires_in_seconds: Option<u64>,
}

/// Response from creating a key (raw key shown ONCE).
#[derive(Debug, Serialize)]
struct CreateKeyResponse {
    /// The raw API key — shown only once, store it safely
    api_key: String,
    /// Key identifier
    key_id: String,
    /// First 8 chars for identification
    key_prefix: String,
    /// Project scope (null = global admin)
    project_id: Option<String>,
    /// Human-readable name
    name: Option<String>,
    /// Granted scopes
    scopes: Vec<String>,
    /// When the key expires (null = never)
    expires_at: Option<String>,
}

/// A key in the list response (no raw key exposed).
#[derive(Debug, Serialize)]
struct KeyInfoResponse {
    key_id: String,
    key_prefix: String,
    project_id: Option<String>,
    name: Option<String>,
    scopes: Vec<String>,
    created_at: String,
    last_used_at: Option<String>,
    is_revoked: bool,
}

/// Response for listing keys.
#[derive(Debug, Serialize)]
struct ListKeysResponse {
    keys: Vec<KeyInfoResponse>,
}

const DEFAULT_UI_KEY_MAX_TTL_SECONDS: u64 = 90 * 24 * 60 * 60;
const UI_KEY_NAME_MIN_LEN: usize = 3;
const UI_KEY_NAME_MAX_LEN: usize = 64;

fn ui_key_max_ttl_seconds() -> u64 {
    std::env::var("MLRUNX_UI_KEY_MAX_TTL_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_UI_KEY_MAX_TTL_SECONDS)
}

fn validate_ui_key_name(name: Option<&str>) -> Result<String, (StatusCode, String)> {
    let Some(trimmed) = name.map(str::trim).filter(|value| !value.is_empty()) else {
        return Err((
            StatusCode::BAD_REQUEST,
            "name is required for UI key creation.".to_string(),
        ));
    };

    if trimmed.len() < UI_KEY_NAME_MIN_LEN || trimmed.len() > UI_KEY_NAME_MAX_LEN {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "name must be between {UI_KEY_NAME_MIN_LEN} and {UI_KEY_NAME_MAX_LEN} characters."
            ),
        ));
    }

    for segment in trimmed.split('/') {
        if segment.is_empty() || segment.starts_with('-') || segment.ends_with('-') {
            return Err((
                StatusCode::BAD_REQUEST,
                "name must use slash-delimited segments with lowercase letters, digits, and hyphens.".to_string(),
            ));
        }
        if !segment
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
        {
            return Err((
                StatusCode::BAD_REQUEST,
                "name may only contain lowercase letters, digits, '/', and '-'.".to_string(),
            ));
        }
    }

    Ok(trimmed.to_string())
}

fn resolve_ui_key_ttl_seconds(
    expires_in_seconds: Option<u64>,
) -> Result<u64, (StatusCode, String)> {
    let Some(ttl) = expires_in_seconds else {
        return Err((
            StatusCode::BAD_REQUEST,
            "expires_in_seconds is required for UI key creation.".to_string(),
        ));
    };

    if ttl == 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            "expires_in_seconds must be greater than zero.".to_string(),
        ));
    }

    let max_ttl = ui_key_max_ttl_seconds();
    if ttl > max_ttl {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("expires_in_seconds exceeds policy maximum of {max_ttl} seconds."),
        ));
    }

    Ok(ttl)
}

fn normalize_requested_scopes(scopes: &[String]) -> Result<Vec<String>, (StatusCode, String)> {
    if scopes.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "At least one scope is required.".to_string(),
        ));
    }

    let valid_scopes = ["admin", "write", "read"];
    let mut normalized: Vec<String> = Vec::new();

    for scope in scopes {
        let value = scope.trim().to_ascii_lowercase();
        if !valid_scopes.contains(&value.as_str()) {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("Invalid scope '{scope}'. Valid scopes: admin, write, read"),
            ));
        }
        if !normalized.iter().any(|existing| existing == &value) {
            normalized.push(value);
        }
    }

    Ok(normalized)
}

fn resolve_ui_key_project_id(
    auth: &AuthContext,
    requested_project_id: Option<&str>,
) -> Result<String, (StatusCode, String)> {
    let allowed_projects = auth.allowed_project_ids().ok_or_else(|| {
        (
            StatusCode::FORBIDDEN,
            "UI session is missing project memberships.".to_string(),
        )
    })?;

    let Some(project_id) = requested_project_id else {
        return Err((
            StatusCode::BAD_REQUEST,
            "project_id is required for UI key creation.".to_string(),
        ));
    };

    if allowed_projects.contains(project_id) {
        return Ok(project_id.to_string());
    }

    Err((
        StatusCode::FORBIDDEN,
        format!("Access denied: cannot manage API keys for project '{project_id}'."),
    ))
}

fn ensure_requested_scopes_within_caller(
    auth: &AuthContext,
    requested_scopes: &[String],
) -> Result<(), (StatusCode, String)> {
    for scope in requested_scopes {
        if !auth.api_key.has_scope(scope) {
            return Err((
                StatusCode::FORBIDDEN,
                format!("Insufficient permissions: your UI session cannot grant '{scope}' scope."),
            ));
        }
    }
    Ok(())
}

fn filter_keys_to_ui_memberships(auth: &AuthContext, keys: Vec<auth::ApiKey>) -> Vec<auth::ApiKey> {
    let Some(allowed_projects) = auth.allowed_project_ids() else {
        return Vec::new();
    };
    let Some(user_id) = auth_user_id(auth) else {
        return Vec::new();
    };

    keys.into_iter()
        .filter(|key| {
            key.project_id
                .as_ref()
                .is_some_and(|project_id| allowed_projects.contains(project_id))
                && key.created_by_user_id.as_deref() == Some(user_id.as_str())
        })
        .collect()
}

/// Create a new API key.
#[allow(clippy::too_many_lines)]
async fn http_create_key(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(req): Json<CreateKeyRequest>,
) -> Result<Json<CreateKeyResponse>, (StatusCode, String)> {
    let normalized_scopes = normalize_requested_scopes(&req.scopes)?;
    let mut target_project_id = req.project_id.clone();
    let mut target_name = req
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let mut target_expires_in_seconds = req.expires_in_seconds;

    if auth.is_ui_jwt() {
        if let Err((status, message)) = auth.require_scope("write") {
            emit_audit_event(
                &state,
                Some(&auth),
                req.project_id.as_deref(),
                None,
                "api_key.create",
                "api_key",
                None,
                "denied",
                serde_json::json!({
                    "reason": "scope_denied",
                    "required_scope": "write",
                    "auth_mode": auth_mode_label(&auth),
                }),
            )
            .await;
            return Err((status, message));
        }

        if normalized_scopes.iter().any(|scope| scope == "admin") {
            emit_audit_event(
                &state,
                Some(&auth),
                req.project_id.as_deref(),
                None,
                "api_key.create",
                "api_key",
                None,
                "denied",
                serde_json::json!({
                    "reason": "admin_scope_not_allowed_for_ui_session",
                    "auth_mode": auth_mode_label(&auth),
                }),
            )
            .await;
            return Err((
                StatusCode::FORBIDDEN,
                "UI session key creation cannot grant admin scope. Use read/write scopes."
                    .to_string(),
            ));
        }

        if let Err((status, message)) =
            ensure_requested_scopes_within_caller(&auth, &normalized_scopes)
        {
            emit_audit_event(
                &state,
                Some(&auth),
                req.project_id.as_deref(),
                None,
                "api_key.create",
                "api_key",
                None,
                "denied",
                serde_json::json!({
                    "reason": "requested_scope_not_granted_to_ui_session",
                    "requested_scopes": normalized_scopes.clone(),
                    "caller_scopes": auth.api_key.scopes.clone(),
                    "auth_mode": auth_mode_label(&auth),
                }),
            )
            .await;
            return Err((status, message));
        }

        let validated_name = match validate_ui_key_name(req.name.as_deref()) {
            Ok(name) => name,
            Err((status, message)) => {
                emit_audit_event(
                    &state,
                    Some(&auth),
                    req.project_id.as_deref(),
                    None,
                    "api_key.create",
                    "api_key",
                    None,
                    "denied",
                    serde_json::json!({
                        "reason": "invalid_key_name",
                        "provided_name": req.name.clone(),
                        "auth_mode": auth_mode_label(&auth),
                    }),
                )
                .await;
                return Err((status, message));
            }
        };
        target_name = Some(validated_name);

        let validated_ttl_seconds = match resolve_ui_key_ttl_seconds(req.expires_in_seconds) {
            Ok(ttl) => ttl,
            Err((status, message)) => {
                emit_audit_event(
                    &state,
                    Some(&auth),
                    req.project_id.as_deref(),
                    None,
                    "api_key.create",
                    "api_key",
                    None,
                    "denied",
                    serde_json::json!({
                        "reason": "invalid_ttl",
                        "requested_ttl_seconds": req.expires_in_seconds,
                        "max_ttl_seconds": ui_key_max_ttl_seconds(),
                        "auth_mode": auth_mode_label(&auth),
                    }),
                )
                .await;
                return Err((status, message));
            }
        };
        target_expires_in_seconds = Some(validated_ttl_seconds);

        let resolved_project_id = match resolve_ui_key_project_id(&auth, req.project_id.as_deref())
        {
            Ok(project_id) => project_id,
            Err((status, message)) => {
                emit_audit_event(
                    &state,
                    Some(&auth),
                    req.project_id.as_deref(),
                    None,
                    "api_key.create",
                    "api_key",
                    None,
                    "denied",
                    serde_json::json!({
                        "reason": "project_access_denied",
                        "requested_project_id": req.project_id.clone(),
                        "auth_mode": auth_mode_label(&auth),
                    }),
                )
                .await;
                return Err((status, message));
            }
        };
        target_project_id = Some(resolved_project_id);
    } else {
        require_endpoint_access(
            &state,
            &auth,
            EndpointRbacTier::Admin,
            None,
            None,
            "api_key.create",
            "api_key",
            None,
        )
        .await?;
    }

    let key_owner_user_id = if auth.is_ui_jwt() {
        auth_user_id(&auth)
    } else {
        None
    };

    let (raw_key, key) = state
        .key_store
        .create_key_with_owner(
            target_project_id.clone(),
            target_name.clone(),
            normalized_scopes.clone(),
            target_expires_in_seconds,
            key_owner_user_id.clone(),
        )
        .await;

    info!(
        key_prefix = %key.key_prefix,
        project_id = ?target_project_id,
        scopes = ?normalized_scopes,
        expires_in_seconds = ?target_expires_in_seconds,
        "Created new API key"
    );

    emit_audit_event(
        &state,
        Some(&auth),
        key.project_id.as_deref(),
        None,
        "api_key.create",
        "api_key",
        Some(&key.id),
        "success",
        serde_json::json!({
            "scopes": normalized_scopes,
            "expires_in_seconds": target_expires_in_seconds,
            "created_by_user_id": key_owner_user_id,
        }),
    )
    .await;

    let expires_at_str = key.expires_at.map(|t| {
        chrono::DateTime::<chrono::Utc>::from(t)
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string()
    });

    Ok(Json(CreateKeyResponse {
        api_key: raw_key,
        key_id: key.id,
        key_prefix: key.key_prefix,
        project_id: key.project_id,
        name: key.name,
        scopes: key.scopes,
        expires_at: expires_at_str,
    }))
}

/// List API keys visible to the caller.
async fn http_list_keys(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
) -> Result<Json<ListKeysResponse>, (StatusCode, String)> {
    let keys = if auth.is_ui_jwt() {
        if let Err((status, message)) = auth.require_scope("read") {
            emit_audit_event(
                &state,
                Some(&auth),
                None,
                None,
                "api_key.list",
                "api_key",
                None,
                "denied",
                serde_json::json!({
                    "reason": "scope_denied",
                    "required_scope": "read",
                    "auth_mode": auth_mode_label(&auth),
                }),
            )
            .await;
            return Err((status, message));
        }
        let all_keys = state.key_store.list_keys(None).await;
        filter_keys_to_ui_memberships(&auth, all_keys)
    } else {
        require_endpoint_access(
            &state,
            &auth,
            EndpointRbacTier::Admin,
            None,
            None,
            "api_key.list",
            "api_key",
            None,
        )
        .await?;
        let project_filter = auth.project_id();
        state.key_store.list_keys(project_filter).await
    };

    let keys_response: Vec<KeyInfoResponse> = keys
        .into_iter()
        .map(|k| {
            let created_at = k
                .created_at
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let last_used_at = k.last_used_at.map(|t| {
                t.duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
                    .to_string()
            });

            KeyInfoResponse {
                key_id: k.id,
                key_prefix: k.key_prefix,
                project_id: k.project_id,
                name: k.name,
                scopes: k.scopes,
                created_at: created_at.to_string(),
                last_used_at,
                is_revoked: k.revoked_at.is_some(),
            }
        })
        .collect();

    Ok(Json(ListKeysResponse {
        keys: keys_response,
    }))
}

/// Revoke an API key by its `key_id`.
async fn http_revoke_key(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    axum::extract::Path(key_id): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    validate_path_id(&key_id, "key_id")?;
    let keys = if auth.is_ui_jwt() {
        if let Err((status, message)) = auth.require_scope("write") {
            emit_audit_event(
                &state,
                Some(&auth),
                None,
                None,
                "api_key.revoke",
                "api_key",
                Some(&key_id),
                "denied",
                serde_json::json!({
                    "reason": "scope_denied",
                    "required_scope": "write",
                    "auth_mode": auth_mode_label(&auth),
                }),
            )
            .await;
            return Err((status, message));
        }
        let all_keys = state.key_store.list_keys(None).await;
        filter_keys_to_ui_memberships(&auth, all_keys)
    } else {
        require_endpoint_access(
            &state,
            &auth,
            EndpointRbacTier::Admin,
            None,
            None,
            "api_key.revoke",
            "api_key",
            Some(&key_id),
        )
        .await?;
        let project_filter = auth.project_id();
        state.key_store.list_keys(project_filter).await
    };

    // Find the key by id and get its hash for revocation
    let target = keys.iter().find(|k| k.id == key_id);

    match target {
        Some(key) => {
            state.key_store.revoke_key(&key.key_hash).await;
            info!(key_id = %key_id, key_prefix = %key.key_prefix, "Revoked API key");
            emit_audit_event(
                &state,
                Some(&auth),
                key.project_id.as_deref(),
                None,
                "api_key.revoke",
                "api_key",
                Some(&key_id),
                "success",
                serde_json::json!({}),
            )
            .await;
            Ok(Json(
                serde_json::json!({ "status": "ok", "revoked": key_id }),
            ))
        }
        None => Err((StatusCode::NOT_FOUND, format!("Key not found: {key_id}"))),
    }
}

// =============================================================================
// Share Token API Handlers
// =============================================================================

/// Request to create a share link.
#[derive(Debug, Deserialize)]
struct CreateShareRequest {
    /// Number of days until the link expires (required; bounded by policy).
    expires_in_days: Option<i64>,
}

const DEFAULT_SHARE_LINK_MAX_TTL_DAYS: i64 = 1;

#[cfg(test)]
static SHARE_LINK_POLICY_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
#[cfg(test)]
static SHARE_LINK_POLICY_TEST_OVERRIDE: std::sync::Mutex<Option<(bool, i64)>> =
    std::sync::Mutex::new(None);

#[cfg(test)]
struct ShareLinkPolicyOverrideGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    previous: Option<(bool, i64)>,
}

#[cfg(test)]
impl Drop for ShareLinkPolicyOverrideGuard {
    fn drop(&mut self) {
        let mut slot = SHARE_LINK_POLICY_TEST_OVERRIDE
            .lock()
            .expect("Failed to lock share-link policy override");
        *slot = self.previous;
    }
}

#[cfg(test)]
fn set_share_link_policy_override_for_tests(
    override_value: Option<(bool, i64)>,
) -> ShareLinkPolicyOverrideGuard {
    let lock = SHARE_LINK_POLICY_TEST_LOCK
        .lock()
        .expect("Failed to lock share-link policy test mutex");
    let mut slot = SHARE_LINK_POLICY_TEST_OVERRIDE
        .lock()
        .expect("Failed to lock share-link policy override");
    let previous = *slot;
    *slot = override_value;
    drop(slot);
    ShareLinkPolicyOverrideGuard {
        _lock: lock,
        previous,
    }
}

fn share_links_enabled() -> bool {
    #[cfg(test)]
    {
        let slot = SHARE_LINK_POLICY_TEST_OVERRIDE
            .lock()
            .expect("Failed to lock share-link policy override");
        if let Some((enabled, _)) = *slot {
            return enabled;
        }
    }
    env_flag_default("MLRUNX_SHARE_LINKS_ENABLED", false)
}

fn share_link_max_ttl_days() -> i64 {
    #[cfg(test)]
    {
        let slot = SHARE_LINK_POLICY_TEST_OVERRIDE
            .lock()
            .expect("Failed to lock share-link policy override");
        if let Some((_, max_ttl_days)) = *slot {
            return max_ttl_days.max(1);
        }
    }
    std::env::var("MLRUNX_SHARE_LINK_MAX_TTL_DAYS")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_SHARE_LINK_MAX_TTL_DAYS)
}

fn resolve_share_expiry(expires_in_days: Option<i64>) -> Result<String, (StatusCode, String)> {
    let Some(days) = expires_in_days else {
        return Err((
            StatusCode::BAD_REQUEST,
            "expires_in_days is required for share links.".to_string(),
        ));
    };
    if days <= 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            "expires_in_days must be greater than zero.".to_string(),
        ));
    }
    let max_days = share_link_max_ttl_days();
    if days > max_days {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("expires_in_days exceeds policy maximum of {max_days} days."),
        ));
    }

    let expires = chrono::Utc::now() + chrono::Duration::days(days);
    Ok(expires.format("%Y-%m-%d %H:%M:%S").to_string())
}

/// Response from creating a share link.
#[derive(Debug, Serialize)]
struct CreateShareResponse {
    token: String,
    share_url: String,
    run_id: String,
    expires_at: Option<String>,
}

/// Response for a shared run (public, no auth).
#[derive(Debug, Serialize)]
struct SharedRunResponse {
    run_id: String,
    project_id: String,
    name: Option<String>,
    status: String,
    metrics_count: u64,
    params_count: u64,
    tags: std::collections::HashMap<String, String>,
    created_at: String,
    updated_at: String,
    duration_seconds: Option<f64>,
    available_metrics: Vec<String>,
}

fn share_token_hash(key_store: &ApiKeyStore, token: &str) -> String {
    key_store.hmac_fingerprint(token)
}

fn share_token_resource_id(token_hash: &str) -> String {
    format!("share_token:{}", &token_hash[..token_hash.len().min(16)])
}

/// Create a share token for a run.
async fn http_create_share_token(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    axum::extract::Path(run_id): axum::extract::Path<String>,
    Json(req): Json<CreateShareRequest>,
) -> Result<Json<CreateShareResponse>, (StatusCode, String)> {
    if !share_links_enabled() {
        return Err((
            StatusCode::FORBIDDEN,
            "Run share links are disabled by server policy.".to_string(),
        ));
    }
    validate_path_id(&run_id, "run_id")?;
    // Verify the caller can access the run
    let run = state
        .sqlite_store
        .get_run(&run_id)
        .await
        .map_err(|e| match e {
            storage::SqliteError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            _ => internal_error(e),
        })?;
    require_endpoint_access(
        &state,
        &auth,
        EndpointRbacTier::Read,
        Some(&run.project_id),
        Some(&run_id),
        "share_token.create",
        "run",
        Some(&run_id),
    )
    .await?;
    require_ui_run_owner(&auth, &run)?;
    require_api_key_run_owner_for_mutation(&auth, &run, "share")?;

    // Generate a short, URL-safe token
    let token = generate_share_token();
    let token_hash = share_token_hash(state.key_store.as_ref(), &token);
    let token_resource_id = share_token_resource_id(&token_hash);

    let expires_at = resolve_share_expiry(req.expires_in_days)?;
    let expires_at_opt = Some(expires_at.clone());

    state
        .sqlite_store
        .create_share_token(
            &token_hash,
            &run_id,
            Some(&auth.api_key.key_prefix),
            Some(expires_at.as_str()),
        )
        .await
        .map_err(internal_error)?;

    info!(run_id = %run_id, "Created share token");

    emit_audit_event(
        &state,
        Some(&auth),
        Some(&run.project_id),
        Some(&run_id),
        "share_token.create",
        "share_token",
        Some(&token_resource_id),
        "success",
        serde_json::json!({
            "expires_at": expires_at_opt.clone(),
        }),
    )
    .await;

    Ok(Json(CreateShareResponse {
        share_url: format!("/api/v1/shared/{token}"),
        token,
        run_id,
        expires_at: expires_at_opt,
    }))
}

/// Get a shared run via token (PUBLIC — no auth required).
async fn http_get_shared_run(
    State(state): State<AppState>,
    axum::extract::Path(token): axum::extract::Path<String>,
) -> Result<Json<SharedRunResponse>, (StatusCode, String)> {
    if !share_links_enabled() {
        return Err((StatusCode::NOT_FOUND, "Not found".to_string()));
    }
    let token_hash = share_token_hash(state.key_store.as_ref(), &token);
    // Validate the share token
    let share = state
        .sqlite_store
        .validate_share_token(&token_hash, Some(&token))
        .await
        .map_err(|e| match e {
            storage::SqliteError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            _ => internal_error(e),
        })?;

    // Fetch the run
    let run = state
        .sqlite_store
        .get_run(&share.run_id)
        .await
        .map_err(internal_error)?;

    let tags = state
        .sqlite_store
        .get_tags(&share.run_id)
        .await
        .unwrap_or_default()
        .into_iter()
        .collect::<std::collections::HashMap<String, String>>();

    let available_metrics = state
        .sqlite_store
        .get_metric_names(&share.run_id)
        .await
        .unwrap_or_default();

    Ok(Json(SharedRunResponse {
        run_id: run.id,
        project_id: run.project_id,
        name: run.name,
        status: run.status,
        metrics_count: i64_to_u64_or_zero(run.metrics_count),
        params_count: i64_to_u64_or_zero(run.params_count),
        tags,
        created_at: run.created_at,
        updated_at: run.updated_at,
        duration_seconds: run.duration_seconds,
        available_metrics,
    }))
}

/// Get metrics for a shared run via token (PUBLIC — no auth required).
async fn http_get_shared_metrics(
    State(state): State<AppState>,
    axum::extract::Path(token): axum::extract::Path<String>,
    axum::extract::Query(query): axum::extract::Query<MetricsQuery>,
) -> Result<Json<services::MetricsQueryResponse>, (StatusCode, String)> {
    if !share_links_enabled() {
        return Err((StatusCode::NOT_FOUND, "Not found".to_string()));
    }
    let token_hash = share_token_hash(state.key_store.as_ref(), &token);
    // Validate the share token
    let share = state
        .sqlite_store
        .validate_share_token(&token_hash, Some(&token))
        .await
        .map_err(|e| match e {
            storage::SqliteError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            _ => internal_error(e),
        })?;

    // Parse metric names
    let names: Vec<String> = if query.names.is_empty() {
        vec![]
    } else {
        query
            .names
            .split(',')
            .map(|s| s.trim().to_string())
            .collect()
    };

    // Query metrics
    let sqlite_series = state
        .sqlite_store
        .get_metrics(&share.run_id, &names, query.max_points)
        .await
        .map_err(internal_error)?;

    let available_metrics = state
        .sqlite_store
        .get_metric_names(&share.run_id)
        .await
        .unwrap_or_default();

    let series: Vec<services::MetricSeries> = sqlite_series
        .into_iter()
        .map(|s| services::MetricSeries {
            name: s.name,
            points: s
                .points
                .into_iter()
                .map(|p| services::AggregatedPoint {
                    step: p.step,
                    mean: p.mean,
                    min: p.min,
                    max: p.max,
                    count: p.count,
                })
                .collect(),
            total_points: s.total_points,
            downsampled: s.downsampled,
        })
        .collect();

    Ok(Json(services::MetricsQueryResponse {
        run_id: share.run_id,
        series,
        available_metrics,
    }))
}

/// Revoke a share token for a run.
async fn http_revoke_share_token(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    axum::extract::Path((run_id, token)): axum::extract::Path<(String, String)>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let token_hash = share_token_hash(state.key_store.as_ref(), &token);
    let token_resource_id = share_token_resource_id(&token_hash);

    // Verify the caller can access the run
    let run = state
        .sqlite_store
        .get_run(&run_id)
        .await
        .map_err(|e| match e {
            storage::SqliteError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            _ => internal_error(e),
        })?;
    require_endpoint_access(
        &state,
        &auth,
        EndpointRbacTier::Read,
        Some(&run.project_id),
        Some(&run_id),
        "share_token.revoke",
        "share_token",
        Some(&token_resource_id),
    )
    .await?;
    require_ui_run_owner(&auth, &run)?;
    require_api_key_run_owner_for_mutation(&auth, &run, "revoke share links for")?;

    state
        .sqlite_store
        .revoke_share_token(&token_hash, Some(&token))
        .await
        .map_err(|e| match e {
            storage::SqliteError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            _ => internal_error(e),
        })?;

    info!(run_id = %run_id, "Revoked share token");

    emit_audit_event(
        &state,
        Some(&auth),
        Some(&run.project_id),
        Some(&run_id),
        "share_token.revoke",
        "share_token",
        Some(&token_resource_id),
        "success",
        serde_json::json!({}),
    )
    .await;

    Ok(Json(
        serde_json::json!({ "status": "ok", "revoked": token_resource_id }),
    ))
}

/// Generate a URL-safe share token (24 chars).
fn generate_share_token() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    let bytes: Vec<u8> = (0..18).map(|_| rng.random()).collect();
    base64_url_encode(&bytes)
}

/// Base64 URL-safe encoding (no padding).
fn base64_url_encode(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)
}

// =============================================================================
// Query API Handlers
// =============================================================================

/// Query parameters for listing runs.
#[derive(Debug, Default, Deserialize)]
struct ListRunsQuery {
    /// Filter by project ID
    project: Option<String>,
    /// Filter by owner user ID
    owner: Option<String>,
    /// Filter by run status
    status: Option<String>,
    /// Filter by exact run name
    name: Option<String>,
    /// Free-text search query
    q: Option<String>,
    /// Comma-separated tag filters (key or key=value)
    tags: Option<String>,
    /// Comma-separated param filters (key or key=value)
    params: Option<String>,
    /// Structured filter expression with AND/OR and comparisons
    filter: Option<String>,
    /// Sort field for stable pagination ordering
    sort_by: Option<String>,
    /// Sort order ("asc" or "desc")
    sort_order: Option<String>,
    /// Include runs created at or after this timestamp
    created_after: Option<String>,
    /// Include runs created at or before this timestamp
    created_before: Option<String>,
    /// Maximum number of runs to return
    limit: Option<usize>,
    /// Number of runs to skip
    offset: Option<usize>,
}

/// A run in the response.
#[derive(Debug, Serialize)]
struct RunResponse {
    run_id: String,
    project_id: String,
    name: Option<String>,
    status: String,
    metrics_count: u64,
    params_count: u64,
    tags: std::collections::HashMap<String, String>,
    created_at: String,
    updated_at: String,
    duration_seconds: Option<f64>,
}

/// Response for listing runs.
#[derive(Debug, Serialize)]
struct ListRunsResponse {
    runs: Vec<RunResponse>,
    total: usize,
    limit: usize,
    offset: usize,
}

/// List runs with optional filtering (queries from `SQLite`).
#[allow(clippy::too_many_lines)]
async fn http_list_runs(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    axum::extract::Query(query): axum::extract::Query<ListRunsQuery>,
) -> Result<Json<ListRunsResponse>, (StatusCode, String)> {
    // Require at least read scope (feature-flagged for UI JWT mode).
    require_endpoint_access(
        &state,
        &auth,
        EndpointRbacTier::Read,
        None,
        None,
        "runs.list",
        "run",
        None,
    )
    .await?;

    let limit = query.limit.unwrap_or(100).min(1000);
    let offset = query.offset.unwrap_or(0);
    let tag_filters = parse_metadata_filters(query.tags.as_deref(), "tags", "tags")?;
    let param_filters = parse_metadata_filters(query.params.as_deref(), "parameters", "params")?;
    let filter_expr = parse_run_filter_expr(query.filter.as_deref())?;
    let sort_field = parse_run_list_sort_field(query.sort_by.as_deref())?;
    let sort_order = parse_run_list_sort_order(query.sort_order.as_deref())?;

    let created_after = query
        .created_after
        .as_deref()
        .map(|raw| normalize_created_at_filter(raw, "created_after"))
        .transpose()?;
    let created_before = query
        .created_before
        .as_deref()
        .map(|raw| normalize_created_at_filter(raw, "created_before"))
        .transpose()?;

    // Enforce project scope:
    // - API key scoped callers: existing behavior.
    // - UI JWT callers: project must be one of the user's active memberships.
    let effective_project = if auth.is_platform_admin {
        query.project.clone()
    } else if let Some(allowed_projects) = auth.allowed_project_ids() {
        if let Some(ref requested) = query.project {
            if !allowed_projects.contains(requested) {
                emit_audit_event(
                    &state,
                    Some(&auth),
                    Some(requested),
                    None,
                    "runs.list",
                    "run",
                    None,
                    "denied",
                    serde_json::json!({
                        "reason": "project_membership_mismatch",
                    }),
                )
                .await;
                return Err((
                    StatusCode::FORBIDDEN,
                    format!("Access denied: this user is not a member of project '{requested}'."),
                ));
            }
            Some(requested.clone())
        } else if allowed_projects.len() == 1 {
            allowed_projects.iter().next().cloned()
        } else {
            return Err((
                StatusCode::BAD_REQUEST,
                "Multiple project memberships found. Provide ?project=<project_id>.".to_string(),
            ));
        }
    } else {
        match auth.project_id() {
            Some(scoped_project) => {
                // If caller also passed a ?project= filter, verify it matches their scope.
                if let Some(ref requested) = query.project {
                    if requested != scoped_project {
                        emit_audit_event(
                            &state,
                            Some(&auth),
                            Some(requested),
                            None,
                            "runs.list",
                            "run",
                            None,
                            "denied",
                            serde_json::json!({
                                "reason": "api_key_project_scope_mismatch",
                                "scoped_project": scoped_project,
                            }),
                        )
                        .await;
                        return Err((
                            StatusCode::FORBIDDEN,
                            format!(
                                "Access denied: your key is scoped to project '{scoped_project}', cannot query '{requested}'."
                            ),
                        ));
                    }
                }
                Some(scoped_project.to_string())
            }
            None => query.project.clone(), // Admin/dev: use whatever was requested.
        }
    };

    let owner_user_filter = if auth.is_ui_jwt() && !auth.is_platform_admin {
        let current_user_id = auth_user_id(&auth).ok_or_else(|| {
            (
                StatusCode::FORBIDDEN,
                "Unable to resolve user identity for run listing.".to_string(),
            )
        })?;
        if let Some(requested_owner) = query.owner.as_deref()
            && requested_owner != current_user_id
        {
            return Err((
                StatusCode::FORBIDDEN,
                "Access denied: non-admin users can only query their own runs.".to_string(),
            ));
        }
        Some(current_user_id)
    } else {
        query.owner.clone()
    };

    if let Some(owner) = owner_user_filter.as_deref() {
        validate_path_id(owner, "owner")?;
    }

    // Query from SQLite
    let (sqlite_runs, total) = state
        .sqlite_store
        .list_runs(
            effective_project.as_deref(),
            owner_user_filter.as_deref(),
            query.status.as_deref(),
            query.q.as_deref(),
            query.name.as_deref(),
            created_after.as_deref(),
            created_before.as_deref(),
            &tag_filters,
            &param_filters,
            filter_expr.as_ref(),
            sort_field,
            sort_order,
            limit,
            offset,
        )
        .await
        .map_err(internal_error)?;

    // Convert to response format
    let mut runs_response = Vec::new();
    for run in sqlite_runs {
        // Get tags for this run
        let tags = state
            .sqlite_store
            .get_tags(&run.id)
            .await
            .unwrap_or_default()
            .into_iter()
            .collect::<std::collections::HashMap<String, String>>();

        runs_response.push(RunResponse {
            run_id: run.id,
            project_id: run.project_id,
            name: run.name,
            status: run.status,
            metrics_count: i64_to_u64_or_zero(run.metrics_count),
            params_count: i64_to_u64_or_zero(run.params_count),
            tags,
            created_at: run.created_at,
            updated_at: run.updated_at,
            duration_seconds: run.duration_seconds,
        });
    }

    Ok(Json(ListRunsResponse {
        runs: runs_response,
        total,
        limit,
        offset,
    }))
}

/// Detailed run response including metrics summary.
#[derive(Debug, Serialize)]
struct RunDetailResponse {
    run_id: String,
    project_id: String,
    name: Option<String>,
    status: String,
    metrics_count: u64,
    params_count: u64,
    tags: std::collections::HashMap<String, String>,
    created_at: String,
    updated_at: String,
    duration_seconds: Option<f64>,
    // Additional detail fields
    metrics_summary: Vec<MetricSummaryResponse>,
}

#[derive(Debug, Serialize)]
struct MetricSummaryResponse {
    name: String,
    last_value: f64,
    last_step: i64,
}

/// Get run detail by ID.
async fn http_get_run(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    axum::extract::Path(run_id): axum::extract::Path<String>,
) -> Result<Json<RunDetailResponse>, (StatusCode, String)> {
    validate_path_id(&run_id, "run_id")?;
    // Run list is sourced from SQLite; run detail must use the same source.
    let run = state
        .sqlite_store
        .get_run(&run_id)
        .await
        .map_err(|e| match e {
            storage::SqliteError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            _ => internal_error(e),
        })?;

    // Verify the caller can access this run's project and has read scope
    require_endpoint_access(
        &state,
        &auth,
        EndpointRbacTier::Read,
        Some(&run.project_id),
        Some(&run_id),
        "run.read",
        "run",
        Some(&run_id),
    )
    .await?;
    require_ui_run_owner(&auth, &run)?;

    let tags = state
        .sqlite_store
        .get_tags(&run_id)
        .await
        .unwrap_or_default()
        .into_iter()
        .collect::<std::collections::HashMap<String, String>>();

    // Keep detail summary lightweight for now.
    let metrics_summary = vec![];

    Ok(Json(RunDetailResponse {
        run_id: run.id.clone(),
        project_id: run.project_id.clone(),
        name: run.name.clone(),
        status: run.status.clone(),
        metrics_count: i64_to_u64_or_zero(run.metrics_count),
        params_count: i64_to_u64_or_zero(run.params_count),
        tags,
        created_at: run.created_at.clone(),
        updated_at: run.updated_at.clone(),
        duration_seconds: run.duration_seconds,
        metrics_summary,
    }))
}

// =============================================================================
// Metrics Query API
// =============================================================================

/// Query parameters for metrics endpoint.
#[derive(Debug, Deserialize)]
struct MetricsQuery {
    /// Comma-separated metric names (empty = all)
    #[serde(default)]
    names: String,
    /// Maximum points per metric (triggers downsampling)
    #[serde(default = "default_max_points")]
    max_points: usize,
    /// Start step (inclusive)
    start_step: Option<i64>,
    /// End step (inclusive)
    end_step: Option<i64>,
}

const fn default_max_points() -> usize {
    1000
}

#[derive(Debug, Deserialize)]
struct RunEventsQuery {
    after_id: Option<i64>,
    #[serde(default = "default_run_events_limit")]
    limit: usize,
}

const fn default_run_events_limit() -> usize {
    200
}

#[derive(Debug, Serialize)]
struct RunEventResponse {
    id: i64,
    run_id: String,
    level: String,
    source: String,
    message: String,
    step: Option<i64>,
    timestamp: Option<f64>,
    created_at: String,
}

#[derive(Debug, Serialize)]
struct ListRunEventsResponse {
    run_id: String,
    events: Vec<RunEventResponse>,
    next_after_id: Option<i64>,
    has_more: bool,
}

#[derive(Debug, Deserialize)]
struct StructuredRunLogsQuery {
    after_id: Option<i64>,
    #[serde(default = "default_run_events_limit")]
    limit: usize,
    names: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
struct RunChartLogResponse {
    id: i64,
    run_id: String,
    name: String,
    chart_type: String,
    renderer_hint: Option<String>,
    data: serde_json::Value,
    layout: serde_json::Value,
    options: serde_json::Value,
    metadata: serde_json::Value,
    step: Option<i64>,
    timestamp: Option<f64>,
    created_at: String,
}

#[derive(Debug, Serialize, Clone)]
struct RunImageLogResponse {
    id: i64,
    run_id: String,
    name: String,
    path: String,
    caption: Option<String>,
    metadata: serde_json::Value,
    step: Option<i64>,
    timestamp: Option<f64>,
    created_at: String,
}

#[derive(Debug, Serialize)]
struct ListRunChartsResponse {
    run_id: String,
    charts: Vec<RunChartLogResponse>,
    next_after_id: Option<i64>,
    has_more: bool,
}

#[derive(Debug, Serialize)]
struct ListRunImagesResponse {
    run_id: String,
    images: Vec<RunImageLogResponse>,
    next_after_id: Option<i64>,
    has_more: bool,
}

fn parse_name_filter(raw: Option<&str>) -> std::collections::HashSet<String> {
    raw.map(|value| {
        value
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(std::string::ToString::to_string)
            .collect()
    })
    .unwrap_or_default()
}

fn chart_log_from_event_row(row: &RunEventRow) -> Option<RunChartLogResponse> {
    let payload = parse_structured_payload_for_kind(&row.message, "chart")?;
    let object = payload.as_object()?;
    let name = object.get("name")?.as_str()?.to_string();
    let chart_type = object
        .get("chart_type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("custom")
        .to_string();
    let renderer_hint = object
        .get("renderer_hint")
        .and_then(serde_json::Value::as_str)
        .map(std::string::ToString::to_string);
    let data = object
        .get("data")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let layout = object
        .get("layout")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let options = object
        .get("options")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let metadata = object
        .get("metadata")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));

    Some(RunChartLogResponse {
        id: row.id,
        run_id: row.run_id.clone(),
        name,
        chart_type,
        renderer_hint,
        data,
        layout,
        options,
        metadata,
        step: row.step,
        timestamp: row.timestamp,
        created_at: row.created_at.clone(),
    })
}

fn image_log_from_event_row(row: &RunEventRow) -> Option<RunImageLogResponse> {
    let payload = parse_structured_payload_for_kind(&row.message, "image")?;
    let object = payload.as_object()?;
    let name = object.get("name")?.as_str()?.to_string();
    let path = object
        .get("path")
        .or_else(|| object.get("uri"))
        .and_then(serde_json::Value::as_str)?
        .to_string();
    let caption = object
        .get("caption")
        .and_then(serde_json::Value::as_str)
        .map(std::string::ToString::to_string);
    let metadata = object
        .get("metadata")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));

    Some(RunImageLogResponse {
        id: row.id,
        run_id: row.run_id.clone(),
        name,
        path,
        caption,
        metadata,
        step: row.step,
        timestamp: row.timestamp,
        created_at: row.created_at.clone(),
    })
}

/// Get metrics for a run with optional downsampling.
async fn http_get_metrics(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    axum::extract::Path(run_id): axum::extract::Path<String>,
    axum::extract::Query(query): axum::extract::Query<MetricsQuery>,
) -> Result<Json<services::MetricsQueryResponse>, (StatusCode, String)> {
    validate_path_id(&run_id, "run_id")?;
    // Verify run exists, check project access, and require read scope
    let run = state
        .sqlite_store
        .get_run(&run_id)
        .await
        .map_err(|_| (StatusCode::NOT_FOUND, format!("Run not found: {run_id}")))?;
    require_endpoint_access(
        &state,
        &auth,
        EndpointRbacTier::Read,
        Some(&run.project_id),
        Some(&run_id),
        "run.metrics.read",
        "run",
        Some(&run_id),
    )
    .await?;
    require_ui_run_owner(&auth, &run)?;

    // Parse metric names
    let names: Vec<String> = if query.names.is_empty() {
        vec![]
    } else {
        query
            .names
            .split(',')
            .map(|s| s.trim().to_string())
            .collect()
    };

    // Query metrics from SQLite
    let sqlite_series = state
        .sqlite_store
        .get_metrics(&run_id, &names, query.max_points)
        .await
        .map_err(internal_error)?;

    // Get available metric names
    let available_metrics = state
        .sqlite_store
        .get_metric_names(&run_id)
        .await
        .unwrap_or_default();

    // Convert SQLite format to API format
    let series: Vec<services::MetricSeries> = sqlite_series
        .into_iter()
        .map(|s| services::MetricSeries {
            name: s.name,
            points: s
                .points
                .into_iter()
                .map(|p| services::AggregatedPoint {
                    step: p.step,
                    mean: p.mean,
                    min: p.min,
                    max: p.max,
                    count: p.count,
                })
                .collect(),
            total_points: s.total_points,
            downsampled: s.downsampled,
        })
        .collect();

    Ok(Json(services::MetricsQueryResponse {
        run_id,
        series,
        available_metrics,
    }))
}

/// Get structured run events for log/timeline display.
async fn http_get_run_events(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    axum::extract::Path(run_id): axum::extract::Path<String>,
    axum::extract::Query(query): axum::extract::Query<RunEventsQuery>,
) -> Result<Json<ListRunEventsResponse>, (StatusCode, String)> {
    validate_path_id(&run_id, "run_id")?;
    let run = state
        .sqlite_store
        .get_run(&run_id)
        .await
        .map_err(|_| (StatusCode::NOT_FOUND, format!("Run not found: {run_id}")))?;

    require_endpoint_access(
        &state,
        &auth,
        EndpointRbacTier::Read,
        Some(&run.project_id),
        Some(&run_id),
        "run.events.read",
        "run",
        Some(&run_id),
    )
    .await?;
    require_ui_run_owner(&auth, &run)?;

    let limit = query.limit.clamp(1, 500);
    let fetch_limit = limit + 1;
    let mut rows = state
        .sqlite_store
        .list_run_events(&run_id, query.after_id, fetch_limit)
        .await
        .map_err(internal_error)?;

    let has_more = rows.len() > limit;
    if has_more {
        rows.truncate(limit);
    }

    let next_after_id = rows.last().map(|row| row.id).or(query.after_id);

    let events = rows
        .into_iter()
        .map(|row| RunEventResponse {
            id: row.id,
            run_id: row.run_id,
            level: row.level,
            source: row.source,
            message: row.message,
            step: row.step,
            timestamp: row.timestamp,
            created_at: row.created_at,
        })
        .collect();

    Ok(Json(ListRunEventsResponse {
        run_id,
        events,
        next_after_id,
        has_more,
    }))
}

/// Get structured chart events for a run.
async fn http_get_run_charts(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    axum::extract::Path(run_id): axum::extract::Path<String>,
    axum::extract::Query(query): axum::extract::Query<StructuredRunLogsQuery>,
) -> Result<Json<ListRunChartsResponse>, (StatusCode, String)> {
    validate_path_id(&run_id, "run_id")?;
    let run = state
        .sqlite_store
        .get_run(&run_id)
        .await
        .map_err(|_| (StatusCode::NOT_FOUND, format!("Run not found: {run_id}")))?;

    require_endpoint_access(
        &state,
        &auth,
        EndpointRbacTier::Read,
        Some(&run.project_id),
        Some(&run_id),
        "run.charts.read",
        "run",
        Some(&run_id),
    )
    .await?;
    require_ui_run_owner(&auth, &run)?;

    let limit = query.limit.clamp(1, 200);
    let fetch_limit = (limit.saturating_mul(10)).clamp(20, 1000);
    let names = parse_name_filter(query.names.as_deref());

    let rows = state
        .sqlite_store
        .list_run_events(&run_id, query.after_id, fetch_limit)
        .await
        .map_err(internal_error)?;

    let mut charts: Vec<RunChartLogResponse> = rows
        .iter()
        .filter_map(chart_log_from_event_row)
        .filter(|chart| names.is_empty() || names.contains(&chart.name))
        .collect();

    let has_more = charts.len() > limit;
    if has_more {
        charts.truncate(limit);
    }

    let next_after_id = charts.last().map(|chart| chart.id).or(query.after_id);

    Ok(Json(ListRunChartsResponse {
        run_id,
        charts,
        next_after_id,
        has_more,
    }))
}

/// Get structured image events for a run.
async fn http_get_run_images(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    axum::extract::Path(run_id): axum::extract::Path<String>,
    axum::extract::Query(query): axum::extract::Query<StructuredRunLogsQuery>,
) -> Result<Json<ListRunImagesResponse>, (StatusCode, String)> {
    validate_path_id(&run_id, "run_id")?;
    let run = state
        .sqlite_store
        .get_run(&run_id)
        .await
        .map_err(|_| (StatusCode::NOT_FOUND, format!("Run not found: {run_id}")))?;

    require_endpoint_access(
        &state,
        &auth,
        EndpointRbacTier::Read,
        Some(&run.project_id),
        Some(&run_id),
        "run.images.read",
        "run",
        Some(&run_id),
    )
    .await?;
    require_ui_run_owner(&auth, &run)?;

    let limit = query.limit.clamp(1, 200);
    let fetch_limit = (limit.saturating_mul(10)).clamp(20, 1000);
    let names = parse_name_filter(query.names.as_deref());

    let rows = state
        .sqlite_store
        .list_run_events(&run_id, query.after_id, fetch_limit)
        .await
        .map_err(internal_error)?;

    let mut images: Vec<RunImageLogResponse> = rows
        .iter()
        .filter_map(image_log_from_event_row)
        .filter(|image| names.is_empty() || names.contains(&image.name))
        .collect();

    let has_more = images.len() > limit;
    if has_more {
        images.truncate(limit);
    }

    let next_after_id = images.last().map(|image| image.id).or(query.after_id);

    Ok(Json(ListRunImagesResponse {
        run_id,
        images,
        next_after_id,
        has_more,
    }))
}

// =============================================================================
// Compare Runs API
// =============================================================================

/// Request body for comparing runs.
#[derive(Debug, Deserialize)]
struct CompareRunsRequest {
    /// Run IDs to compare
    run_ids: Vec<String>,
    /// Metric names to compare (empty = all common metrics)
    #[serde(default)]
    metric_names: Vec<String>,
    /// Maximum points per metric per run
    #[serde(default = "default_max_points")]
    max_points: usize,
    /// Alignment mode: "step" (default) or "time"
    #[serde(default = "default_alignment")]
    alignment: String,
    /// Max runs to include in this response page
    #[serde(default = "default_compare_page_limit")]
    limit: usize,
    /// Number of deduplicated run IDs to skip
    #[serde(default)]
    offset: usize,
}

fn default_alignment() -> String {
    "step".to_string()
}

const MAX_COMPARE_RUN_IDS: usize = 5000;
const MAX_COMPARE_PAGE_LIMIT: usize = 1000;

const fn default_compare_page_limit() -> usize {
    100
}

const fn default_chart_limit_per_run() -> usize {
    20
}

/// Metrics data for a single run in comparison.
#[derive(Debug, Serialize)]
struct RunCompareData {
    run_id: String,
    run_name: Option<String>,
    status: String,
    series: Vec<services::MetricSeries>,
}

/// Response for comparing runs.
#[derive(Debug, Serialize)]
struct CompareRunsResponse {
    /// Data for each run
    runs: Vec<RunCompareData>,
    /// Metric names common to all runs
    common_metrics: Vec<String>,
    /// Alignment mode used
    alignment: String,
    /// Total unique run IDs in the request
    total: usize,
    /// Page size after clamping
    limit: usize,
    /// Offset applied to the deduplicated run IDs
    offset: usize,
}

/// Request body for comparing chart payloads across runs.
#[derive(Debug, Deserialize)]
struct CompareRunChartsRequest {
    run_ids: Vec<String>,
    #[serde(default)]
    chart_names: Vec<String>,
    #[serde(default = "default_chart_limit_per_run")]
    limit_per_run: usize,
}

#[derive(Debug, Serialize)]
struct RunChartsCompareData {
    run_id: String,
    run_name: Option<String>,
    status: String,
    charts: Vec<RunChartLogResponse>,
}

#[derive(Debug, Serialize)]
struct CompareRunChartsResponse {
    runs: Vec<RunChartsCompareData>,
    common_chart_names: Vec<String>,
}

/// Compare metrics across multiple runs.
async fn http_compare_runs(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(req): Json<CompareRunsRequest>,
) -> Result<Json<CompareRunsResponse>, (StatusCode, String)> {
    if req.run_ids.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "run_ids cannot be empty".to_string(),
        ));
    }

    if req.run_ids.len() > MAX_COMPARE_RUN_IDS {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Maximum {MAX_COMPARE_RUN_IDS} runs can be compared"),
        ));
    }

    if req.limit == 0 {
        return Err((StatusCode::BAD_REQUEST, "limit must be >= 1".to_string()));
    }

    let page_limit = req.limit.min(MAX_COMPARE_PAGE_LIMIT);

    let mut seen = std::collections::HashSet::new();
    let unique_run_ids: Vec<String> = req
        .run_ids
        .iter()
        .filter_map(|run_id| {
            if seen.insert(run_id.clone()) {
                Some(run_id.clone())
            } else {
                None
            }
        })
        .collect();
    let total = unique_run_ids.len();
    let paged_run_ids: Vec<String> = unique_run_ids
        .iter()
        .skip(req.offset)
        .take(page_limit)
        .cloned()
        .collect();

    // Collect data for each run
    let mut runs_data = Vec::new();
    let mut all_metric_sets: Vec<std::collections::HashSet<String>> = Vec::new();

    for run_id in &paged_run_ids {
        let run = state
            .sqlite_store
            .get_run(run_id)
            .await
            .map_err(|e| match e {
                storage::SqliteError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
                _ => internal_error(e),
            })?;

        // Verify the caller can access this run's project and has read scope
        require_endpoint_access(
            &state,
            &auth,
            EndpointRbacTier::Read,
            Some(&run.project_id),
            Some(run_id),
            "runs.compare",
            "run",
            Some(run_id),
        )
        .await?;
        require_ui_run_owner(&auth, &run)?;

        let names = if req.metric_names.is_empty() {
            vec![]
        } else {
            req.metric_names.clone()
        };

        let sqlite_series = state
            .sqlite_store
            .get_metrics(run_id, &names, req.max_points)
            .await
            .map_err(internal_error)?;

        let series: Vec<services::MetricSeries> = sqlite_series
            .into_iter()
            .map(|s| services::MetricSeries {
                name: s.name,
                points: s
                    .points
                    .into_iter()
                    .map(|p| services::AggregatedPoint {
                        step: p.step,
                        mean: p.mean,
                        min: p.min,
                        max: p.max,
                        count: p.count,
                    })
                    .collect(),
                total_points: s.total_points,
                downsampled: s.downsampled,
            })
            .collect();

        let available = state
            .sqlite_store
            .get_metric_names(run_id)
            .await
            .unwrap_or_default();

        // Track metric names for finding common ones
        let metric_set: std::collections::HashSet<String> = available.into_iter().collect();
        all_metric_sets.push(metric_set);

        runs_data.push(RunCompareData {
            run_id: run_id.clone(),
            run_name: run.name.clone(),
            status: run.status.clone(),
            series,
        });
    }

    // Find common metrics (intersection of all sets)
    let common_metrics: Vec<String> = if all_metric_sets.is_empty() {
        vec![]
    } else {
        let mut common = all_metric_sets[0].clone();
        for set in all_metric_sets.iter().skip(1) {
            common = common.intersection(set).cloned().collect();
        }
        let mut common_vec: Vec<_> = common.into_iter().collect();
        common_vec.sort();
        common_vec
    };

    Ok(Json(CompareRunsResponse {
        runs: runs_data,
        common_metrics,
        alignment: req.alignment,
        total,
        limit: page_limit,
        offset: req.offset,
    }))
}

/// Compare structured chart logs across multiple runs.
async fn http_compare_run_charts(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(req): Json<CompareRunChartsRequest>,
) -> Result<Json<CompareRunChartsResponse>, (StatusCode, String)> {
    if req.run_ids.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "run_ids cannot be empty".to_string(),
        ));
    }

    if req.run_ids.len() > 100 {
        return Err((
            StatusCode::BAD_REQUEST,
            "Maximum 100 runs can be compared".to_string(),
        ));
    }

    let requested_names: std::collections::HashSet<String> =
        req.chart_names.iter().cloned().collect();
    let limit_per_run = req.limit_per_run.clamp(1, 200);
    let mut runs_data = Vec::new();
    let mut per_run_name_sets: Vec<std::collections::HashSet<String>> = Vec::new();

    for run_id in &req.run_ids {
        let run = state
            .sqlite_store
            .get_run(run_id)
            .await
            .map_err(|e| match e {
                storage::SqliteError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
                _ => internal_error(e),
            })?;

        require_endpoint_access(
            &state,
            &auth,
            EndpointRbacTier::Read,
            Some(&run.project_id),
            Some(run_id),
            "runs.compare.charts",
            "run",
            Some(run_id),
        )
        .await?;
        require_ui_run_owner(&auth, &run)?;

        let rows = state
            .sqlite_store
            .list_run_events(run_id, None, 1000)
            .await
            .map_err(internal_error)?;

        let mut charts: Vec<RunChartLogResponse> = rows
            .iter()
            .filter_map(chart_log_from_event_row)
            .filter(|chart| requested_names.is_empty() || requested_names.contains(&chart.name))
            .collect();

        if charts.len() > limit_per_run {
            charts.truncate(limit_per_run);
        }

        let chart_name_set = charts.iter().map(|chart| chart.name.clone()).collect();
        per_run_name_sets.push(chart_name_set);

        runs_data.push(RunChartsCompareData {
            run_id: run_id.clone(),
            run_name: run.name.clone(),
            status: run.status.clone(),
            charts,
        });
    }

    let common_chart_names: Vec<String> = if per_run_name_sets.is_empty() {
        vec![]
    } else {
        let mut common = per_run_name_sets[0].clone();
        for set in per_run_name_sets.iter().skip(1) {
            common = common.intersection(set).cloned().collect();
        }
        let mut names: Vec<_> = common.into_iter().collect();
        names.sort();
        names
    };

    Ok(Json(CompareRunChartsResponse {
        runs: runs_data,
        common_chart_names,
    }))
}

// =============================================================================
// Per-IP HTTP Rate Limiter
// =============================================================================

/// Simple in-memory token-bucket rate limiter keyed by client IP.
struct HttpRateLimiter {
    /// Max tokens (burst capacity).
    capacity: u32,
    /// Tokens added per second.
    refill_rate: f64,
    /// Per-IP state: (`tokens_remaining`, `last_refill_instant`).
    buckets: tokio::sync::Mutex<std::collections::HashMap<String, (f64, std::time::Instant)>>,
}

impl HttpRateLimiter {
    fn new(capacity: u32, refill_rate: f64) -> Self {
        Self {
            capacity,
            refill_rate,
            buckets: tokio::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    fn from_env() -> Self {
        let capacity = std::env::var("MLRUNX_RATE_LIMIT_BURST")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(100);
        let refill_rate = std::env::var("MLRUNX_RATE_LIMIT_PER_SECOND")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(50.0);
        Self::new(capacity, refill_rate)
    }

    /// Try to consume one token. Returns Ok(()) if allowed, `Err(retry_after_secs)` if denied.
    async fn check(&self, client_ip: &str) -> Result<(), u64> {
        let now = std::time::Instant::now();
        let mut buckets = self.buckets.lock().await;

        let (tokens, last_refill) = buckets
            .entry(client_ip.to_string())
            .or_insert_with(|| (f64::from(self.capacity), now));

        // Refill tokens based on elapsed time.
        let elapsed = now.duration_since(*last_refill).as_secs_f64();
        *tokens = elapsed
            .mul_add(self.refill_rate, *tokens)
            .min(f64::from(self.capacity));
        *last_refill = now;

        if *tokens >= 1.0 {
            *tokens -= 1.0;
            Ok(())
        } else {
            let wait = (1.0 - *tokens) / self.refill_rate;
            Err(retry_after_seconds(wait))
        }
    }

    /// Periodically prune stale entries to prevent memory growth.
    async fn prune_stale(&self) {
        let cutoff = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(300))
            .unwrap();
        let mut buckets = self.buckets.lock().await;
        buckets.retain(|_, (_, last_seen)| *last_seen > cutoff);
    }
}

fn extract_client_ip(req: &axum::extract::Request) -> String {
    let socket_ip = req
        .extensions()
        .get::<axum::extract::ConnectInfo<SocketAddr>>()
        .map(|info| info.0.ip())
        .or_else(|| req.extensions().get::<SocketAddr>().map(|addr| addr.ip()));

    if should_trust_forwarded_headers(socket_ip) {
        if let Some(forwarded_ip) = trusted_forwarded_client_ip(req.headers()) {
            return forwarded_ip;
        }
    }
    client_ip_from_socket_extensions(req.extensions()).unwrap_or_else(|| "unknown".to_string())
}

const DEFAULT_ALLOWED_UI_ORIGINS: [&str; 2] = ["http://localhost:3000", "http://127.0.0.1:3000"];

fn parse_allowed_origin(origin: &str) -> Result<HeaderValue, String> {
    let parsed: Uri = origin.parse().map_err(|_| {
        format!("Invalid origin '{origin}' in MLRUNX_UI_ALLOWED_ORIGINS (failed URI parse).")
    })?;
    let scheme = parsed.scheme_str().ok_or_else(|| {
        format!("Invalid origin '{origin}' in MLRUNX_UI_ALLOWED_ORIGINS (missing scheme).")
    })?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return Err(format!(
            "Invalid origin '{origin}' in MLRUNX_UI_ALLOWED_ORIGINS (scheme must be http or https)."
        ));
    }
    let authority = parsed.authority().ok_or_else(|| {
        format!("Invalid origin '{origin}' in MLRUNX_UI_ALLOWED_ORIGINS (missing host).")
    })?;
    if let Some(path_and_query) = parsed.path_and_query() {
        if path_and_query.path() != "/" || path_and_query.query().is_some() {
            return Err(format!(
                "Invalid origin '{origin}' in MLRUNX_UI_ALLOWED_ORIGINS (must not include path/query)."
            ));
        }
    }
    let normalized = format!("{scheme}://{authority}");
    HeaderValue::from_str(&normalized).map_err(|_| {
        format!("Invalid origin '{origin}' in MLRUNX_UI_ALLOWED_ORIGINS (invalid header value).")
    })
}

fn parse_allowed_origins(raw: Option<&str>) -> Result<Vec<HeaderValue>, String> {
    let mut origins = Vec::new();

    if let Some(raw) = raw {
        for origin in raw
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            origins.push(parse_allowed_origin(origin)?);
        }
    }

    if origins.is_empty() {
        for origin in DEFAULT_ALLOWED_UI_ORIGINS {
            origins.push(parse_allowed_origin(origin)?);
        }
    }

    Ok(origins)
}

fn load_allowed_origins_from_env() -> Result<Vec<HeaderValue>, String> {
    parse_allowed_origins(std::env::var("MLRUNX_UI_ALLOWED_ORIGINS").ok().as_deref())
}

fn validate_runtime_security_configuration(key_store: &ApiKeyStore) -> Result<(), String> {
    let _ = load_allowed_origins_from_env()?;

    if let Some(domain) = key_store.ui_cookie_domain() {
        if domain.contains(';') || domain.chars().any(char::is_whitespace) {
            return Err(
                "MLRUNX_UI_COOKIE_DOMAIN must not contain semicolons or whitespace.".to_string(),
            );
        }
    }

    if key_store.is_ui_jwt_enabled()
        && key_store.ui_cookie_same_site().eq_ignore_ascii_case("none")
        && !key_store.ui_cookie_secure()
    {
        return Err(
            "MLRUNX_UI_COOKIE_SAMESITE=None requires MLRUNX_UI_COOKIE_SECURE=true.".to_string(),
        );
    }

    if key_store.is_ui_jwt_enabled() && is_production_environment() && !key_store.ui_cookie_secure()
    {
        return Err(
            "UI JWT/session auth in production requires MLRUNX_UI_COOKIE_SECURE=true.".to_string(),
        );
    }

    Ok(())
}

// =============================================================================
// Server Setup
// =============================================================================

#[allow(clippy::too_many_lines)]
fn build_http_router(state: AppState) -> Router {
    let ui_jwt_enabled = state.key_store.is_ui_jwt_enabled();
    let share_links_enabled = share_links_enabled();

    // Always apply restrictive CORS — never fall back to permissive(), even when
    // auth is disabled, to prevent cross-origin attacks from arbitrary websites.
    let allowed_origins =
        load_allowed_origins_from_env().expect("Invalid MLRUNX_UI_ALLOWED_ORIGINS configuration");

    let cors = CorsLayer::new()
        .allow_credentials(ui_jwt_enabled)
        .allow_origin(allowed_origins)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            header::CONTENT_TYPE,
            header::AUTHORIZATION,
            HeaderName::from_static("x-api-key"),
            HeaderName::from_static("x-csrf-token"),
        ]);
    let decompression = RequestDecompressionLayer::new();

    // Routes that require authentication
    let mut protected_routes = Router::new()
        // SDK HTTP transport endpoints (ingestion)
        .route("/api/v1/runs", post(http_init_run))
        .route("/api/v1/ingest/batch", post(http_ingest_batch))
        .route("/api/v1/runs/{run_id}/finish", post(http_finish_run))
        // Project management endpoints
        .route(
            "/api/v1/projects",
            get(http_list_projects).post(http_create_project),
        )
        .route("/api/v1/projects/{project_id}", delete(http_delete_project))
        // Platform admin control-plane endpoints (global admin only)
        .route("/api/v1/admin/users", get(http_admin_list_users))
        .route(
            "/api/v1/admin/users/{user_id}/memberships",
            get(http_admin_list_user_memberships),
        )
        .route(
            "/api/v1/admin/users/{user_id}/disable",
            post(http_admin_disable_user),
        )
        .route(
            "/api/v1/admin/users/{user_id}/enable",
            post(http_admin_enable_user),
        )
        .route("/api/v1/admin/sessions", get(http_admin_list_sessions))
        .route(
            "/api/v1/admin/sessions/{session_id}/revoke",
            post(http_admin_revoke_session),
        )
        .route(
            "/api/v1/admin/audit-events",
            get(http_admin_list_audit_events),
        )
        .route(
            "/api/v1/admin/bootstrap-key/rotate",
            post(http_admin_rotate_bootstrap_key),
        )
        // Query API endpoints
        .route("/api/v1/runs", get(http_list_runs))
        .route(
            "/api/v1/runs/{run_id}",
            get(http_get_run).delete(http_delete_run),
        )
        .route("/api/v1/runs/{run_id}/metrics", get(http_get_metrics))
        .route("/api/v1/runs/{run_id}/events", get(http_get_run_events))
        .route("/api/v1/runs/{run_id}/charts", get(http_get_run_charts))
        .route("/api/v1/runs/{run_id}/images", get(http_get_run_images))
        .route("/api/v1/runs/compare", post(http_compare_runs))
        .route("/api/v1/runs/compare/charts", post(http_compare_run_charts))
        // Key management endpoints (admin only)
        .route("/api/v1/keys", post(http_create_key).get(http_list_keys))
        .route("/api/v1/keys/{key_id}", delete(http_revoke_key));

    if share_links_enabled {
        protected_routes = protected_routes
            .route("/api/v1/runs/{run_id}/share", post(http_create_share_token))
            .route(
                "/api/v1/runs/{run_id}/share/{token}",
                delete(http_revoke_share_token),
            );
    }

    if ui_jwt_enabled {
        protected_routes = protected_routes
            .route("/api/v1/ui-auth/session", get(http_ui_auth_session))
            .route("/api/v1/ui-auth/logout", post(http_ui_auth_logout));
    }

    protected_routes = protected_routes.layer(middleware::from_fn_with_state(
        state.key_store.clone(),
        auth_middleware,
    ));

    // Public routes (no auth required)
    let mut public_routes = Router::new()
        .route("/", get(root))
        .route("/health", get(health));

    if share_links_enabled {
        public_routes = public_routes
            .route("/api/v1/shared/{token}", get(http_get_shared_run))
            .route(
                "/api/v1/shared/{token}/metrics",
                get(http_get_shared_metrics),
            );
    }

    if ui_jwt_enabled {
        public_routes = public_routes.route("/api/v1/ui-auth/login", post(http_ui_auth_login));
    }

    // Per-IP HTTP rate limiter (token bucket).
    let rate_limiter = Arc::new(HttpRateLimiter::from_env());

    // Spawn background task to prune stale rate-limit entries every 60s.
    {
        let rl = rate_limiter.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                rl.prune_stale().await;
            }
        });
    }

    // Security response headers + rate limiting applied to every response.
    let rate_limiter_clone = rate_limiter;
    let security_and_rate_limit = middleware::from_fn(
        move |req: axum::extract::Request, next: axum::middleware::Next| {
            let rl = rate_limiter_clone.clone();
            async move {
                let client_ip = extract_client_ip(&req);
                if let Err(retry_after) = rl.check(&client_ip).await {
                    let mut response = axum::response::Response::new(axum::body::Body::from(
                        "Too many requests".to_string(),
                    ));
                    *response.status_mut() = StatusCode::TOO_MANY_REQUESTS;
                    response.headers_mut().insert(
                        "retry-after",
                        HeaderValue::from_str(&retry_after.to_string())
                            .unwrap_or_else(|_| HeaderValue::from_static("1")),
                    );
                    return response;
                }

                let req_id = uuid::Uuid::now_v7().to_string();
                let mut response = next.run(req).await;
                let headers = response.headers_mut();
                headers.insert(
                    "x-request-id",
                    HeaderValue::from_str(&req_id)
                        .unwrap_or_else(|_| HeaderValue::from_static("unknown")),
                );
                headers.insert(
                    "x-content-type-options",
                    HeaderValue::from_static("nosniff"),
                );
                headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
                headers.insert(
                    "referrer-policy",
                    HeaderValue::from_static("strict-origin-when-cross-origin"),
                );
                headers.insert(
                    "x-permitted-cross-domain-policies",
                    HeaderValue::from_static("none"),
                );
                response
            }
        },
    );

    // Combine routes
    Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .layer(security_and_rate_limit)
        .layer(decompression)
        .layer(cors)
        .with_state(state)
}

#[tokio::main]
async fn main() {
    // Load configuration from environment
    let server_config = config::ServerConfig::from_env();

    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| server_config.log_level.clone().into()),
        )
        .init();

    // Log startup configuration
    server_config.log_startup();

    // Initialize idempotency store
    let idempotency_store = Arc::new(IdempotencyStore::new());

    // Initialize cardinality tracker
    let cardinality_tracker = Arc::new(CardinalityTracker::from_env());
    info!(
        "Cardinality limits: {} tag keys/run, {} metric names/run, {} tags/project",
        cardinality_tracker.config().max_tag_keys_per_run,
        cardinality_tracker.config().max_metric_names_per_run,
        cardinality_tracker.config().max_tags_per_project
    );

    // Initialize SQLite store for persistence
    let sqlite_path =
        std::env::var("MLRUNX_SQLITE_PATH").unwrap_or_else(|_| "mlrunx.db".to_string());
    let sqlite_store = Arc::new(
        SqliteStore::new(&sqlite_path)
            .await
            .expect("Failed to initialize SQLite store"),
    );
    info!("SQLite store initialized at: {}", sqlite_path);

    // Initialize API key store.
    // Default behavior: persistent sqlite-backed store.
    // Rollback/fallback: set MLRUNX_API_KEYS_IN_MEMORY=1 to use legacy in-memory behavior.
    let use_in_memory_keys = std::env::var("MLRUNX_API_KEYS_IN_MEMORY")
        .is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));
    let key_store = if use_in_memory_keys {
        info!("Using in-memory API key store (MLRUNX_API_KEYS_IN_MEMORY enabled)");
        Arc::new(ApiKeyStore::new())
    } else {
        Arc::new(ApiKeyStore::new_with_sqlite(sqlite_store.clone()))
    };
    key_store.init_from_env().await;

    if let Err(err) = key_store.validate_startup_configuration() {
        panic!("Refusing to start: {err}");
    }
    if let Err(err) = validate_runtime_security_configuration(key_store.as_ref()) {
        panic!("Refusing to start: {err}");
    }

    if key_store.is_auth_disabled() {
        assert!(
            allow_insecure_local_dev(),
            "Refusing to start: MLRUNX_AUTH_MODE=disabled requires MLRUNX_ALLOW_INSECURE_LOCAL_DEV=true."
        );
        assert!(
            !is_production_environment(),
            "Refusing to start: authentication cannot be disabled in production."
        );
        assert!(
            server_config.http_addr.ip().is_loopback()
                && server_config.grpc_addr.ip().is_loopback(),
            "Refusing to start: disabled auth mode requires loopback-only bind addresses. \
Set API_HOST=127.0.0.1 (or ::1) when MLRUNX_AUTH_MODE=disabled."
        );
        warn!(
            "Authentication is disabled under local-dev override; this instance must remain local-only."
        );
    }

    // Create shared state
    let store = Arc::new(InMemoryStore::new());
    let app_state = AppState {
        store: store.clone(),
        sqlite_store,
        key_store: key_store.clone(),
        idempotency_store,
        cardinality_tracker,
    };

    // Server addresses from config
    let http_addr = server_config.http_addr;
    let grpc_addr = server_config.grpc_addr;

    // Build HTTP router
    let http_app = build_http_router(app_state);

    // Build gRPC service
    let ingest_service = IngestServiceImpl::new(store);
    let grpc_service = IngestServiceServer::new(ingest_service);

    info!("Starting MLRunX API server");
    info!("  HTTP: http://{}", http_addr);
    info!("  gRPC: grpc://{}", grpc_addr);

    // Spawn gRPC server
    let grpc_handle = tokio::spawn(async move {
        if let Err(e) = TonicServer::builder()
            .add_service(grpc_service)
            .serve(grpc_addr)
            .await
        {
            warn!("gRPC server error: {}", e);
        }
    });

    // Start HTTP server (main thread)
    let http_listener = tokio::net::TcpListener::bind(http_addr).await.unwrap();
    let http_handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(
            http_listener,
            http_app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        {
            warn!("HTTP server error: {}", e);
        }
    });

    // Wait for both servers
    tokio::select! {
        _ = grpc_handle => info!("gRPC server stopped"),
        _ = http_handle => info!("HTTP server stopped"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use http_body_util::BodyExt;
    use jsonwebtoken::{EncodingKey, Header as JwtHeader, encode};
    use serde::Serialize;
    use tower::ServiceExt;

    async fn test_app() -> Router {
        let store = Arc::new(InMemoryStore::new());
        // Use in-memory SQLite for tests
        let sqlite_store = Arc::new(
            SqliteStore::new(":memory:")
                .await
                .expect("Failed to create test SQLite store"),
        );
        // Use dev mode for tests (auth disabled)
        let key_store = Arc::new(ApiKeyStore::new_dev_mode());
        let idempotency_store = Arc::new(IdempotencyStore::new());
        let cardinality_tracker = Arc::new(CardinalityTracker::default());
        let state = AppState {
            store,
            sqlite_store,
            key_store,
            idempotency_store,
            cardinality_tracker,
        };
        build_http_router(state)
    }

    struct UiSessionHarness {
        app: Router,
        key_store: Arc<ApiKeyStore>,
        sqlite_store: Arc<SqliteStore>,
        user_id: String,
        primary_project_id: String,
        secondary_project_id: String,
        jwt_secret: String,
        jwt_subject: String,
    }

    #[derive(Debug, Serialize)]
    struct TestJwtClaims {
        sub: String,
        exp: usize,
        email: String,
        name: String,
        iss: String,
        aud: String,
    }

    #[derive(Debug)]
    struct SessionCookies {
        cookie_header: String,
        csrf_token: String,
    }

    async fn ui_session_harness_with_role(role: &str) -> UiSessionHarness {
        let store = Arc::new(InMemoryStore::new());
        let sqlite_store = Arc::new(
            SqliteStore::new(":memory:")
                .await
                .expect("Failed to create test SQLite store"),
        );
        let jwt_secret = "test-ui-session-secret".to_string();
        let key_store = Arc::new(ApiKeyStore::new_with_sqlite_and_ui_jwt(
            sqlite_store.clone(),
            &jwt_secret,
        ));
        let idempotency_store = Arc::new(IdempotencyStore::new());
        let cardinality_tracker = Arc::new(CardinalityTracker::default());

        let jwt_subject = "user-123".to_string();
        let user_id = sqlite_store
            .get_or_create_user_identity(
                "jwt",
                &jwt_subject,
                Some("user@example.com"),
                Some("User"),
            )
            .await
            .expect("Failed to create user");

        let primary_project_id = sqlite_store
            .get_or_create_project("project-primary")
            .await
            .expect("Failed to create primary project");
        sqlite_store
            .grant_project_membership(&primary_project_id, &user_id, role, None)
            .await
            .expect("Failed to grant primary membership");

        let secondary_project_id = sqlite_store
            .get_or_create_project("project-secondary")
            .await
            .expect("Failed to create secondary project");

        let state = AppState {
            store,
            sqlite_store: sqlite_store.clone(),
            key_store: key_store.clone(),
            idempotency_store,
            cardinality_tracker,
        };

        UiSessionHarness {
            app: build_http_router(state),
            key_store,
            sqlite_store,
            user_id,
            primary_project_id,
            secondary_project_id,
            jwt_secret,
            jwt_subject,
        }
    }

    fn build_test_jwt(secret: &str, subject: &str) -> String {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("System clock is before UNIX_EPOCH")
            .as_secs();
        let claims = TestJwtClaims {
            sub: subject.to_string(),
            exp: (now + 3600) as usize,
            email: "user@example.com".to_string(),
            name: "User".to_string(),
            iss: "https://test-auth.local".to_string(),
            aud: "authenticated".to_string(),
        };
        encode(
            &JwtHeader::default(),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .expect("Failed to encode test JWT")
    }

    fn extract_cookie_value(set_cookie_header: &str, cookie_name: &str) -> Option<String> {
        let first_part = set_cookie_header.split(';').next()?.trim();
        let mut parts = first_part.splitn(2, '=');
        let name = parts.next()?.trim();
        let value = parts.next()?.trim();
        if name == cookie_name && !value.is_empty() {
            Some(value.to_string())
        } else {
            None
        }
    }

    async fn login_ui_session(app: &Router, jwt: &str) -> SessionCookies {
        let login_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ui-auth/login")
                    .header("content-type", "application/json")
                    .extension(axum::extract::ConnectInfo(SocketAddr::from((
                        [127, 0, 0, 1],
                        12345,
                    ))))
                    .body(Body::from(serde_json::json!({ "jwt": jwt }).to_string()))
                    .expect("Failed to build login request"),
            )
            .await
            .expect("Login request failed");

        assert_eq!(login_response.status(), StatusCode::OK);

        let mut session_cookie: Option<String> = None;
        let mut csrf_cookie: Option<String> = None;

        for value in login_response.headers().get_all(header::SET_COOKIE).iter() {
            let set_cookie = value.to_str().expect("Invalid set-cookie header");
            if let Some(token) = extract_cookie_value(set_cookie, "mlrunx_ui_session") {
                session_cookie = Some(token);
            }
            if let Some(token) = extract_cookie_value(set_cookie, "mlrunx_ui_csrf") {
                csrf_cookie = Some(token);
            }
        }

        let session_token = session_cookie.expect("Session cookie was not set");
        let csrf_token = csrf_cookie.expect("CSRF cookie was not set");

        SessionCookies {
            cookie_header: format!(
                "mlrunx_ui_session={session_token}; mlrunx_ui_csrf={csrf_token}"
            ),
            csrf_token,
        }
    }

    async fn login_ui_session_with_bearer(app: &Router, jwt: &str) -> SessionCookies {
        let login_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ui-auth/login")
                    .header("authorization", format!("Bearer {jwt}"))
                    .header("content-type", "application/json")
                    .extension(axum::extract::ConnectInfo(SocketAddr::from((
                        [127, 0, 0, 1],
                        12345,
                    ))))
                    .body(Body::from("{}"))
                    .expect("Failed to build bearer login request"),
            )
            .await
            .expect("Bearer login request failed");

        assert_eq!(login_response.status(), StatusCode::OK);

        let mut session_cookie: Option<String> = None;
        let mut csrf_cookie: Option<String> = None;

        for value in login_response.headers().get_all(header::SET_COOKIE).iter() {
            let set_cookie = value.to_str().expect("Invalid set-cookie header");
            if let Some(token) = extract_cookie_value(set_cookie, "mlrunx_ui_session") {
                session_cookie = Some(token);
            }
            if let Some(token) = extract_cookie_value(set_cookie, "mlrunx_ui_csrf") {
                csrf_cookie = Some(token);
            }
        }

        let session_token = session_cookie.expect("Session cookie was not set");
        let csrf_token = csrf_cookie.expect("CSRF cookie was not set");

        SessionCookies {
            cookie_header: format!(
                "mlrunx_ui_session={session_token}; mlrunx_ui_csrf={csrf_token}"
            ),
            csrf_token,
        }
    }

    async fn promote_ui_user_to_platform_admin(harness: &UiSessionHarness) {
        harness
            .sqlite_store
            .sync_platform_admin_flag(&harness.user_id, Some("user@example.com"))
            .await
            .expect("Failed to mark test user as platform admin");
    }

    async fn response_text(response: axum::response::Response) -> String {
        let body = response
            .into_body()
            .collect()
            .await
            .expect("Failed to read response body")
            .to_bytes();
        String::from_utf8(body.to_vec()).expect("Response body is not valid UTF-8")
    }

    fn percent_encode_query_value(raw: &str) -> String {
        let mut encoded = String::new();
        for byte in raw.bytes() {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                encoded.push(char::from(byte));
            } else {
                encoded.push_str(&format!("%{byte:02X}"));
            }
        }
        encoded
    }

    async fn test_app_with_auth_enabled() -> Router {
        let store = Arc::new(InMemoryStore::new());
        let sqlite_store = Arc::new(
            SqliteStore::new(":memory:")
                .await
                .expect("Failed to create test SQLite store"),
        );
        let key_store = Arc::new(ApiKeyStore::new());
        let idempotency_store = Arc::new(IdempotencyStore::new());
        let cardinality_tracker = Arc::new(CardinalityTracker::default());
        let state = AppState {
            store,
            sqlite_store,
            key_store,
            idempotency_store,
            cardinality_tracker,
        };
        build_http_router(state)
    }

    #[tokio::test]
    async fn test_root_endpoint() {
        let app = test_app().await;
        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let app = test_app().await;
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn test_build_cookie_includes_optional_domain() {
        let cookie = build_cookie(
            "mlrunx_ui_session",
            "abc123",
            900,
            true,
            true,
            "Lax",
            Some("mlrunx.example.com"),
        );

        assert!(cookie.contains("Domain=mlrunx.example.com"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("Secure"));
    }

    #[test]
    fn test_build_clear_cookie_includes_optional_domain() {
        let cookie =
            build_clear_cookie("mlrunx_ui_session", true, "Lax", Some("mlrunx.example.com"));
        assert!(cookie.contains("Max-Age=0"));
        assert!(cookie.contains("Domain=mlrunx.example.com"));
    }

    #[test]
    fn test_parse_allowed_origins_validates_and_normalizes() {
        let origins =
            parse_allowed_origins(Some("https://mlrunx.example.com/, http://localhost:3000"))
                .expect("expected valid origins");
        let rendered: Vec<String> = origins
            .iter()
            .map(|value| value.to_str().expect("invalid header value").to_string())
            .collect();
        assert_eq!(
            rendered,
            vec![
                "https://mlrunx.example.com".to_string(),
                "http://localhost:3000".to_string()
            ]
        );
    }

    #[test]
    fn test_parse_allowed_origins_rejects_path_or_query() {
        assert!(parse_allowed_origins(Some("https://mlrunx.example.com/ui")).is_err());
        assert!(parse_allowed_origins(Some("https://mlrunx.example.com?x=1")).is_err());
    }

    #[tokio::test]
    async fn test_ui_auth_login_accepts_bearer_token() {
        let harness = ui_session_harness_with_role("owner").await;
        let jwt = build_test_jwt(&harness.jwt_secret, &harness.jwt_subject);

        let cookies = login_ui_session_with_bearer(&harness.app, &jwt).await;
        assert!(cookies.cookie_header.contains("mlrunx_ui_session="));
        assert!(!cookies.csrf_token.is_empty());
    }

    #[tokio::test]
    async fn test_init_run_http() {
        let store = Arc::new(InMemoryStore::new());
        let sqlite_store = Arc::new(
            SqliteStore::new(":memory:")
                .await
                .expect("Failed to create test SQLite store"),
        );
        let project_id = sqlite_store
            .create_project("test-project", None)
            .await
            .expect("Failed to create test project")
            .id;
        let key_store = Arc::new(ApiKeyStore::new_dev_mode());
        let idempotency_store = Arc::new(IdempotencyStore::new());
        let cardinality_tracker = Arc::new(CardinalityTracker::default());
        let state = AppState {
            store,
            sqlite_store,
            key_store,
            idempotency_store,
            cardinality_tracker,
        };
        let app = build_http_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/runs")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "project_id": project_id,
                            "name": "test-run"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_init_run_http_requires_project_id() {
        let app = test_app().await;
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/runs")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "name": "missing-project-id"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response_text(response).await;
        assert!(body.contains("project_id is required"));
    }

    #[tokio::test]
    async fn test_ingest_rejects_invalid_metadata_key() {
        let app = test_app().await;

        let create_project_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/projects")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "name": "invalid-metadata-project"
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build create project request"),
            )
            .await
            .expect("Create project request failed");
        assert_eq!(create_project_response.status(), StatusCode::OK);
        let create_payload: serde_json::Value =
            serde_json::from_str(&response_text(create_project_response).await)
                .expect("Create project response must be JSON");
        let project_id = create_payload["project_id"]
            .as_str()
            .expect("project_id should exist")
            .to_string();

        let run_id = "run-invalid-metadata-123";
        let init_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/runs")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "project_id": project_id,
                            "run_id": run_id,
                            "name": "invalid-metadata-run"
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build init request"),
            )
            .await
            .expect("Init request failed");
        assert_eq!(init_response.status(), StatusCode::OK);

        let ingest_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ingest/batch")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "run_id": run_id,
                            "metrics": [],
                            "params": [{"name": "bad key", "value": "1"}],
                            "tags": [],
                            "events": []
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build ingest request"),
            )
            .await
            .expect("Ingest request failed");
        assert_eq!(ingest_response.status(), StatusCode::BAD_REQUEST);

        let body = response_text(ingest_response).await;
        assert!(body.contains("Invalid param key"));

        let oversized_param = "x".repeat(PARAM_VALUE_MAX_LEN + 1);
        let oversized_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ingest/batch")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "run_id": run_id,
                            "metrics": [],
                            "params": [{"name": "lr", "value": oversized_param}],
                            "tags": [],
                            "events": []
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build oversized ingest request"),
            )
            .await
            .expect("Oversized ingest request failed");
        assert_eq!(oversized_response.status(), StatusCode::BAD_REQUEST);
        let oversized_body = response_text(oversized_response).await;
        assert!(oversized_body.contains("Invalid param value"));
    }

    #[tokio::test]
    async fn test_list_runs_supports_metadata_and_time_filters() {
        let app = test_app().await;

        let create_project_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/projects")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "name": "list-filter-project"
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build create project request"),
            )
            .await
            .expect("Create project request failed");
        assert_eq!(create_project_response.status(), StatusCode::OK);
        let create_payload: serde_json::Value =
            serde_json::from_str(&response_text(create_project_response).await)
                .expect("Create project response must be JSON");
        let project_id = create_payload["project_id"]
            .as_str()
            .expect("project_id should exist")
            .to_string();

        for run_id in ["run-filter-a", "run-filter-b"] {
            let init_response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/runs")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({
                                "project_id": project_id,
                                "run_id": run_id,
                                "name": run_id
                            })
                            .to_string(),
                        ))
                        .expect("Failed to build init request"),
                )
                .await
                .expect("Init request failed");
            assert_eq!(init_response.status(), StatusCode::OK);
        }

        let ingest_a = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ingest/batch")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "run_id": "run-filter-a",
                            "metrics": [],
                            "params": [{"name": "lr", "value": "0.001"}],
                            "tags": [{"key": "framework", "value": "pytorch"}],
                            "events": []
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build ingest request for run A"),
            )
            .await
            .expect("Ingest request A failed");
        assert_eq!(ingest_a.status(), StatusCode::OK);

        let ingest_b = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ingest/batch")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "run_id": "run-filter-b",
                            "metrics": [],
                            "params": [{"name": "lr", "value": "0.100"}],
                            "tags": [{"key": "framework", "value": "tensorflow"}],
                            "events": []
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build ingest request for run B"),
            )
            .await
            .expect("Ingest request B failed");
        assert_eq!(ingest_b.status(), StatusCode::OK);

        let filtered_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/api/v1/runs?project={project_id}&tags=framework=pytorch&params=lr=0.001"
                    ))
                    .body(Body::empty())
                    .expect("Failed to build filtered list request"),
            )
            .await
            .expect("Filtered list request failed");
        assert_eq!(filtered_response.status(), StatusCode::OK);
        let filtered_payload: serde_json::Value =
            serde_json::from_str(&response_text(filtered_response).await)
                .expect("Filtered runs response should be JSON");
        let filtered_runs = filtered_payload["runs"]
            .as_array()
            .expect("runs should be an array");
        assert_eq!(filtered_runs.len(), 1);
        assert_eq!(
            filtered_runs[0]["run_id"].as_str(),
            Some("run-filter-a"),
            "compound tag+param filter should match only run-filter-a"
        );

        let future_window_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/api/v1/runs?project={project_id}&created_after=2999-01-01T00:00:00Z"
                    ))
                    .body(Body::empty())
                    .expect("Failed to build created_after list request"),
            )
            .await
            .expect("created_after list request failed");
        assert_eq!(future_window_response.status(), StatusCode::OK);
        let future_payload: serde_json::Value =
            serde_json::from_str(&response_text(future_window_response).await)
                .expect("Future filter response should be JSON");
        assert_eq!(
            future_payload["total"].as_u64(),
            Some(0),
            "future created_after should return zero runs"
        );
    }

    #[tokio::test]
    async fn test_list_runs_supports_structured_filter_expression() {
        let app = test_app().await;

        let create_project_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/projects")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "name": "structured-filter-project"
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build create project request"),
            )
            .await
            .expect("Create project request failed");
        assert_eq!(create_project_response.status(), StatusCode::OK);
        let create_payload: serde_json::Value =
            serde_json::from_str(&response_text(create_project_response).await)
                .expect("Create project response must be JSON");
        let project_id = create_payload["project_id"]
            .as_str()
            .expect("project_id should exist")
            .to_string();

        for (run_id, framework, lr) in [
            ("run-filter-expr-a", "pytorch", "0.001"),
            ("run-filter-expr-b", "tensorflow", "0.100"),
            ("run-filter-expr-c", "pytorch", "0.100"),
        ] {
            let init_response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/runs")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({
                                "project_id": project_id,
                                "run_id": run_id,
                                "name": run_id
                            })
                            .to_string(),
                        ))
                        .expect("Failed to build init request"),
                )
                .await
                .expect("Init request failed");
            assert_eq!(init_response.status(), StatusCode::OK);

            let ingest_response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/ingest/batch")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({
                                "run_id": run_id,
                                "metrics": [],
                                "params": [{"name": "lr", "value": lr}],
                                "tags": [{"key": "framework", "value": framework}],
                                "events": []
                            })
                            .to_string(),
                        ))
                        .expect("Failed to build ingest request"),
                )
                .await
                .expect("Ingest request failed");
            assert_eq!(ingest_response.status(), StatusCode::OK);
        }

        for (run_id, status) in [
            ("run-filter-expr-a", "finished"),
            ("run-filter-expr-c", "failed"),
        ] {
            let finish_response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/api/v1/runs/{run_id}/finish"))
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({
                                "status": status
                            })
                            .to_string(),
                        ))
                        .expect("Failed to build finish request"),
                )
                .await
                .expect("Finish request failed");
            assert_eq!(finish_response.status(), StatusCode::OK);
        }

        let and_filter = percent_encode_query_value(
            "status=finished AND tag.framework=pytorch AND param.lr=0.001",
        );
        let and_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/api/v1/runs?project={project_id}&filter={and_filter}"
                    ))
                    .body(Body::empty())
                    .expect("Failed to build AND filter request"),
            )
            .await
            .expect("AND filter request failed");
        assert_eq!(and_response.status(), StatusCode::OK);
        let and_payload: serde_json::Value =
            serde_json::from_str(&response_text(and_response).await)
                .expect("AND filter response should be JSON");
        let and_runs = and_payload["runs"]
            .as_array()
            .expect("runs should be an array");
        assert_eq!(and_runs.len(), 1);
        assert_eq!(and_runs[0]["run_id"].as_str(), Some("run-filter-expr-a"));

        let or_filter = percent_encode_query_value(
            "(status=failed AND tag.framework=pytorch) OR (status=running AND tag.framework=tensorflow)",
        );
        let or_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/api/v1/runs?project={project_id}&filter={or_filter}"
                    ))
                    .body(Body::empty())
                    .expect("Failed to build OR filter request"),
            )
            .await
            .expect("OR filter request failed");
        assert_eq!(or_response.status(), StatusCode::OK);
        let or_payload: serde_json::Value = serde_json::from_str(&response_text(or_response).await)
            .expect("OR filter response should be JSON");
        let or_runs = or_payload["runs"]
            .as_array()
            .expect("runs should be an array");
        assert_eq!(or_runs.len(), 2);
        let mut run_ids: Vec<String> = or_runs
            .iter()
            .filter_map(|run| run["run_id"].as_str().map(str::to_string))
            .collect();
        run_ids.sort();
        assert_eq!(
            run_ids,
            vec![
                "run-filter-expr-b".to_string(),
                "run-filter-expr-c".to_string()
            ]
        );

        let created_window_filter = percent_encode_query_value(
            "created_at>=2000-01-01T00:00:00Z AND created_at<2999-01-01T00:00:00Z",
        );
        let created_window_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/api/v1/runs?project={project_id}&filter={created_window_filter}"
                    ))
                    .body(Body::empty())
                    .expect("Failed to build created_at filter request"),
            )
            .await
            .expect("created_at filter request failed");
        assert_eq!(created_window_response.status(), StatusCode::OK);
        let created_window_payload: serde_json::Value =
            serde_json::from_str(&response_text(created_window_response).await)
                .expect("created_at filter response should be JSON");
        assert_eq!(
            created_window_payload["total"].as_u64(),
            Some(3),
            "created_at window should match all seeded runs"
        );
    }

    #[tokio::test]
    async fn test_list_runs_rejects_invalid_structured_filter_expression() {
        let app = test_app().await;

        let create_project_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/projects")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "name": "structured-filter-invalid-project"
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build create project request"),
            )
            .await
            .expect("Create project request failed");
        assert_eq!(create_project_response.status(), StatusCode::OK);
        let create_payload: serde_json::Value =
            serde_json::from_str(&response_text(create_project_response).await)
                .expect("Create project response must be JSON");
        let project_id = create_payload["project_id"]
            .as_str()
            .expect("project_id should exist")
            .to_string();

        let invalid_operator_filter = percent_encode_query_value("status==running");
        let invalid_operator_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/api/v1/runs?project={project_id}&filter={invalid_operator_filter}"
                    ))
                    .body(Body::empty())
                    .expect("Failed to build invalid operator filter request"),
            )
            .await
            .expect("Invalid operator filter request failed");
        assert_eq!(invalid_operator_response.status(), StatusCode::BAD_REQUEST);
        let invalid_operator_body = response_text(invalid_operator_response).await;
        assert!(
            invalid_operator_body.contains("Invalid filter expression"),
            "expected parser error in response body: {invalid_operator_body}"
        );

        let invalid_field_filter = percent_encode_query_value("unknown=123");
        let invalid_field_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/api/v1/runs?project={project_id}&filter={invalid_field_filter}"
                    ))
                    .body(Body::empty())
                    .expect("Failed to build invalid field filter request"),
            )
            .await
            .expect("Invalid field filter request failed");
        assert_eq!(invalid_field_response.status(), StatusCode::BAD_REQUEST);
        let invalid_field_body = response_text(invalid_field_response).await;
        assert!(
            invalid_field_body.contains("unsupported filter field"),
            "expected unsupported field error in response body: {invalid_field_body}"
        );
    }

    #[tokio::test]
    async fn test_list_runs_supports_sorting_and_rejects_invalid_sort_values() {
        let app = test_app().await;

        let create_project_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/projects")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "name": "sorted-list-project"
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build create project request"),
            )
            .await
            .expect("Create project request failed");
        assert_eq!(create_project_response.status(), StatusCode::OK);
        let create_payload: serde_json::Value =
            serde_json::from_str(&response_text(create_project_response).await)
                .expect("Create project response must be JSON");
        let project_id = create_payload["project_id"]
            .as_str()
            .expect("project_id should exist")
            .to_string();

        for (run_id, run_name) in [
            ("run-sort-2", "beta"),
            ("run-sort-1", "alpha"),
            ("run-sort-3", "alpha"),
        ] {
            let init_response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/runs")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({
                                "project_id": project_id,
                                "run_id": run_id,
                                "name": run_name
                            })
                            .to_string(),
                        ))
                        .expect("Failed to build init request"),
                )
                .await
                .expect("Init request failed");
            assert_eq!(init_response.status(), StatusCode::OK);
        }

        let finish_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/runs/run-sort-3/finish")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "status": "failed"
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build finish request"),
            )
            .await
            .expect("Finish request failed");
        assert_eq!(finish_response.status(), StatusCode::OK);

        let by_name_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/api/v1/runs?project={project_id}&sort_by=name&sort_order=asc"
                    ))
                    .body(Body::empty())
                    .expect("Failed to build sort-by-name request"),
            )
            .await
            .expect("Sort-by-name request failed");
        assert_eq!(by_name_response.status(), StatusCode::OK);
        let by_name_payload: serde_json::Value =
            serde_json::from_str(&response_text(by_name_response).await)
                .expect("Sort-by-name response should be JSON");
        let name_order: Vec<String> = by_name_payload["runs"]
            .as_array()
            .expect("runs should be an array")
            .iter()
            .filter_map(|run| run["run_id"].as_str().map(str::to_string))
            .collect();
        assert_eq!(
            name_order,
            vec![
                "run-sort-1".to_string(),
                "run-sort-3".to_string(),
                "run-sort-2".to_string(),
            ],
            "name sort should be stable with deterministic tie-breakers"
        );

        let by_status_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/api/v1/runs?project={project_id}&sort_by=status&sort_order=asc"
                    ))
                    .body(Body::empty())
                    .expect("Failed to build sort-by-status request"),
            )
            .await
            .expect("Sort-by-status request failed");
        assert_eq!(by_status_response.status(), StatusCode::OK);
        let by_status_payload: serde_json::Value =
            serde_json::from_str(&response_text(by_status_response).await)
                .expect("Sort-by-status response should be JSON");
        let first_status = by_status_payload["runs"]
            .as_array()
            .expect("runs should be an array")
            .first()
            .and_then(|run| run["status"].as_str());
        assert_eq!(first_status, Some("failed"));

        let invalid_sort_by_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/api/v1/runs?project={project_id}&sort_by=unknown_field"
                    ))
                    .body(Body::empty())
                    .expect("Failed to build invalid sort_by request"),
            )
            .await
            .expect("Invalid sort_by request failed");
        assert_eq!(invalid_sort_by_response.status(), StatusCode::BAD_REQUEST);

        let invalid_sort_order_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/api/v1/runs?project={project_id}&sort_order=sideways"
                    ))
                    .body(Body::empty())
                    .expect("Failed to build invalid sort_order request"),
            )
            .await
            .expect("Invalid sort_order request failed");
        assert_eq!(
            invalid_sort_order_response.status(),
            StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn test_ingest_supports_scalar_and_array_metrics() {
        let app = test_app().await;

        let create_project_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/projects")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "name": "array-metrics-project"
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build create project request"),
            )
            .await
            .expect("Create project request failed");
        assert_eq!(create_project_response.status(), StatusCode::OK);
        let create_payload: serde_json::Value =
            serde_json::from_str(&response_text(create_project_response).await)
                .expect("Create project response must be JSON");
        let project_id = create_payload["project_id"]
            .as_str()
            .expect("project_id should exist")
            .to_string();

        let run_id = "run-array-metrics-123";
        let init_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/runs")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "project_id": project_id,
                            "run_id": run_id,
                            "name": "array-metrics-run"
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build init request"),
            )
            .await
            .expect("Init request failed");
        assert_eq!(init_response.status(), StatusCode::OK);

        let ingest_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ingest/batch")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "run_id": run_id,
                            "metrics": [
                                {"name": "loss", "value": 2.5, "step": 1},
                                {"name": "logits", "value": [0.1, 0.2, 0.3], "step": 1}
                            ],
                            "params": [],
                            "tags": [],
                            "events": []
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build ingest request"),
            )
            .await
            .expect("Ingest request failed");
        assert_eq!(ingest_response.status(), StatusCode::OK);

        let metrics_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/v1/runs/{run_id}/metrics"))
                    .body(Body::empty())
                    .expect("Failed to build metrics request"),
            )
            .await
            .expect("Metrics request failed");
        assert_eq!(metrics_response.status(), StatusCode::OK);
        let payload: serde_json::Value =
            serde_json::from_str(&response_text(metrics_response).await)
                .expect("Metrics response should be JSON");
        let available = payload["available_metrics"]
            .as_array()
            .expect("available_metrics should be an array")
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect::<Vec<_>>();
        assert!(available.contains(&"loss"));
        assert!(available.contains(&"logits[0]"));
        assert!(available.contains(&"logits[1]"));
        assert!(available.contains(&"logits[2]"));
    }

    #[tokio::test]
    async fn test_chart_and_image_logs_are_queryable_and_unsafe_chart_is_rejected() {
        let app = test_app().await;

        let create_project_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/projects")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "name": "structured-logs-project"
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build create project request"),
            )
            .await
            .expect("Create project request failed");
        assert_eq!(create_project_response.status(), StatusCode::OK);
        let create_payload: serde_json::Value =
            serde_json::from_str(&response_text(create_project_response).await)
                .expect("Create project response must be JSON");
        let project_id = create_payload["project_id"]
            .as_str()
            .expect("project_id should exist")
            .to_string();

        let run_id = "run-structured-logs-123";
        let init_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/runs")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "project_id": project_id,
                            "run_id": run_id,
                            "name": "structured-logs-run"
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build init request"),
            )
            .await
            .expect("Init request failed");
        assert_eq!(init_response.status(), StatusCode::OK);

        let safe_chart = serde_json::json!({
            "kind": "chart",
            "payload": {
                "name": "train_curve",
                "chart_type": "line",
                "renderer_hint": "plotly",
                "data": {"x": [1, 2, 3], "y": [0.9, 0.7, 0.5]},
                "layout": {"title": "Training Loss"},
                "options": {"showLegend": true},
                "metadata": {"split": "train"}
            }
        })
        .to_string();
        let unsafe_chart = serde_json::json!({
            "kind": "chart",
            "payload": {
                "name": "unsafe_curve",
                "chart_type": "line",
                "data": {"label": "<script>alert(1)</script>"}
            }
        })
        .to_string();
        let safe_image = serde_json::json!({
            "kind": "image",
            "payload": {
                "name": "sample_1",
                "path": "images/sample_1.png",
                "caption": "sample image",
                "metadata": {"stage": "eval"}
            }
        })
        .to_string();

        let ingest_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ingest/batch")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "run_id": run_id,
                            "metrics": [],
                            "params": [],
                            "tags": [],
                            "events": [
                                {"level": "info", "source": "chart", "message": safe_chart, "step": 10},
                                {"level": "info", "source": "chart", "message": unsafe_chart, "step": 11},
                                {"level": "info", "source": "image", "message": safe_image, "step": 12}
                            ]
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build ingest request"),
            )
            .await
            .expect("Ingest request failed");
        assert_eq!(ingest_response.status(), StatusCode::OK);
        let ingest_payload: serde_json::Value =
            serde_json::from_str(&response_text(ingest_response).await)
                .expect("Ingest response should be JSON");
        let warnings = ingest_payload["warnings"]
            .as_array()
            .expect("warnings should be an array");
        assert!(
            warnings.iter().any(|entry| {
                entry
                    .as_str()
                    .is_some_and(|text| text.contains("Dropped unsafe structured event"))
            }),
            "unsafe chart payload should be dropped with a warning"
        );

        let charts_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/v1/runs/{run_id}/charts"))
                    .body(Body::empty())
                    .expect("Failed to build charts request"),
            )
            .await
            .expect("Charts request failed");
        assert_eq!(charts_response.status(), StatusCode::OK);
        let charts_payload: serde_json::Value =
            serde_json::from_str(&response_text(charts_response).await)
                .expect("Charts response should be JSON");
        let charts = charts_payload["charts"]
            .as_array()
            .expect("charts should be an array");
        assert_eq!(charts.len(), 1);
        assert_eq!(charts[0]["name"].as_str(), Some("train_curve"));

        let images_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/v1/runs/{run_id}/images"))
                    .body(Body::empty())
                    .expect("Failed to build images request"),
            )
            .await
            .expect("Images request failed");
        assert_eq!(images_response.status(), StatusCode::OK);
        let images_payload: serde_json::Value =
            serde_json::from_str(&response_text(images_response).await)
                .expect("Images response should be JSON");
        let images = images_payload["images"]
            .as_array()
            .expect("images should be an array");
        assert_eq!(images.len(), 1);
        assert_eq!(images[0]["name"].as_str(), Some("sample_1"));
    }

    #[tokio::test]
    async fn test_compare_run_charts_returns_common_chart_names() {
        let app = test_app().await;

        let create_project_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/projects")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "name": "compare-charts-project"
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build create project request"),
            )
            .await
            .expect("Create project request failed");
        assert_eq!(create_project_response.status(), StatusCode::OK);
        let create_payload: serde_json::Value =
            serde_json::from_str(&response_text(create_project_response).await)
                .expect("Create project response must be JSON");
        let project_id = create_payload["project_id"]
            .as_str()
            .expect("project_id should exist")
            .to_string();

        for run_id in ["run-chart-a", "run-chart-b"] {
            let init_response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/runs")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({
                                "project_id": project_id,
                                "run_id": run_id,
                                "name": run_id
                            })
                            .to_string(),
                        ))
                        .expect("Failed to build init request"),
                )
                .await
                .expect("Init request failed");
            assert_eq!(init_response.status(), StatusCode::OK);
        }

        let chart_loss = serde_json::json!({
            "kind": "chart",
            "payload": {
                "name": "loss_curve",
                "chart_type": "line",
                "data": {"x": [1, 2], "y": [1.0, 0.8]}
            }
        })
        .to_string();
        let chart_acc = serde_json::json!({
            "kind": "chart",
            "payload": {
                "name": "acc_curve",
                "chart_type": "line",
                "data": {"x": [1, 2], "y": [0.5, 0.7]}
            }
        })
        .to_string();

        for (run_id, messages) in [
            ("run-chart-a", vec![chart_loss.clone(), chart_acc.clone()]),
            ("run-chart-b", vec![chart_loss.clone()]),
        ] {
            let events: Vec<serde_json::Value> = messages
                .into_iter()
                .map(|message| {
                    serde_json::json!({
                        "level": "info",
                        "source": "chart",
                        "message": message
                    })
                })
                .collect();

            let ingest_response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/ingest/batch")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({
                                "run_id": run_id,
                                "metrics": [],
                                "params": [],
                                "tags": [],
                                "events": events
                            })
                            .to_string(),
                        ))
                        .expect("Failed to build ingest request"),
                )
                .await
                .expect("Ingest request failed");
            assert_eq!(ingest_response.status(), StatusCode::OK);
        }

        let compare_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/runs/compare/charts")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "run_ids": ["run-chart-a", "run-chart-b"]
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build compare charts request"),
            )
            .await
            .expect("Compare charts request failed");
        assert_eq!(compare_response.status(), StatusCode::OK);
        let payload: serde_json::Value =
            serde_json::from_str(&response_text(compare_response).await)
                .expect("Compare charts response should be JSON");
        let common = payload["common_chart_names"]
            .as_array()
            .expect("common_chart_names should be an array")
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(common, vec!["loss_curve"]);
    }

    #[tokio::test]
    async fn test_compare_runs_supports_paging_and_deduplicates_run_ids() {
        let app = test_app().await;

        let create_project_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/projects")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "name": "compare-runs-paging-project"
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build create project request"),
            )
            .await
            .expect("Create project request failed");
        assert_eq!(create_project_response.status(), StatusCode::OK);
        let create_payload: serde_json::Value =
            serde_json::from_str(&response_text(create_project_response).await)
                .expect("Create project response must be JSON");
        let project_id = create_payload["project_id"]
            .as_str()
            .expect("project_id should exist")
            .to_string();

        for (run_id, loss_value) in [
            ("run-compare-a", 1.2_f64),
            ("run-compare-b", 1.1_f64),
            ("run-compare-c", 1.0_f64),
        ] {
            let init_response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/runs")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({
                                "project_id": project_id,
                                "run_id": run_id,
                                "name": run_id
                            })
                            .to_string(),
                        ))
                        .expect("Failed to build init request"),
                )
                .await
                .expect("Init request failed");
            assert_eq!(init_response.status(), StatusCode::OK);

            let ingest_response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/ingest/batch")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            serde_json::json!({
                                "run_id": run_id,
                                "metrics": [{"name": "loss", "value": loss_value, "step": 1}],
                                "params": [],
                                "tags": [],
                                "events": []
                            })
                            .to_string(),
                        ))
                        .expect("Failed to build ingest request"),
                )
                .await
                .expect("Ingest request failed");
            assert_eq!(ingest_response.status(), StatusCode::OK);
        }

        let compare_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/runs/compare")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "run_ids": ["run-compare-a", "run-compare-b", "run-compare-a", "run-compare-c"],
                            "offset": 1,
                            "limit": 2
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build compare request"),
            )
            .await
            .expect("Compare request failed");
        assert_eq!(compare_response.status(), StatusCode::OK);
        let payload: serde_json::Value =
            serde_json::from_str(&response_text(compare_response).await)
                .expect("Compare response should be JSON");
        assert_eq!(payload["total"].as_u64(), Some(3));
        assert_eq!(payload["limit"].as_u64(), Some(2));
        assert_eq!(payload["offset"].as_u64(), Some(1));
        let run_ids = payload["runs"]
            .as_array()
            .expect("runs should be an array")
            .iter()
            .filter_map(|run| run["run_id"].as_str())
            .collect::<Vec<_>>();
        assert_eq!(run_ids, vec!["run-compare-b", "run-compare-c"]);
        let common_metrics = payload["common_metrics"]
            .as_array()
            .expect("common_metrics should be an array")
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(common_metrics, vec!["loss"]);

        let invalid_limit_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/runs/compare")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "run_ids": ["run-compare-a"],
                            "limit": 0
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build invalid compare request"),
            )
            .await
            .expect("Invalid compare request failed");
        assert_eq!(invalid_limit_response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_run_events_ingest_and_query() {
        let app = test_app().await;

        let create_project_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/projects")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "name": "events-project"
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build create project request"),
            )
            .await
            .expect("Create project request failed");
        assert_eq!(create_project_response.status(), StatusCode::OK);
        let create_payload: serde_json::Value =
            serde_json::from_str(&response_text(create_project_response).await)
                .expect("Create project response must be JSON");
        let project_id = create_payload["project_id"]
            .as_str()
            .expect("project_id should exist")
            .to_string();

        let run_id = "run-events-http-123";
        let init_run_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/runs")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "project_id": project_id,
                            "run_id": run_id,
                            "name": "events-run"
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build init run request"),
            )
            .await
            .expect("Init run request failed");
        assert_eq!(init_run_response.status(), StatusCode::OK);

        let ingest_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ingest/batch")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "run_id": run_id,
                            "metrics": [],
                            "params": [],
                            "tags": [],
                            "events": [
                                {"level": "INFO", "source": "trainer", "message": "Initializing worker", "step": 1},
                                {"level": "warning", "source": "trainer", "message": "Gradient clipped", "step": 2}
                            ]
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build ingest request"),
            )
            .await
            .expect("Ingest request failed");
        assert_eq!(ingest_response.status(), StatusCode::OK);

        let events_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/v1/runs/{run_id}/events?limit=20"))
                    .body(Body::empty())
                    .expect("Failed to build events request"),
            )
            .await
            .expect("Events request failed");
        assert_eq!(events_response.status(), StatusCode::OK);
        let payload: serde_json::Value =
            serde_json::from_str(&response_text(events_response).await)
                .expect("Events response should be JSON");
        let events = payload["events"]
            .as_array()
            .expect("events should be an array");
        assert!(
            events
                .iter()
                .any(|event| event["message"].as_str() == Some("Initializing worker")),
            "ingested info event should be present"
        );
        assert!(
            events
                .iter()
                .any(|event| event["level"].as_str() == Some("warn")),
            "warning level should normalize to warn"
        );
    }

    #[tokio::test]
    async fn test_project_create_and_list_routes() {
        let app = test_app().await;

        let create_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/projects")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "name": "phase1-project",
                            "description": "Project for route coverage"
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_response.status(), StatusCode::OK);
        let created_payload: serde_json::Value =
            serde_json::from_str(&response_text(create_response).await)
                .expect("Create project response should be JSON");
        let project_id = created_payload["project_id"]
            .as_str()
            .expect("project_id should be present")
            .to_string();

        let list_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/projects")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(list_response.status(), StatusCode::OK);

        let payload: serde_json::Value = serde_json::from_str(&response_text(list_response).await)
            .expect("List projects response should be JSON");
        let projects = payload["projects"]
            .as_array()
            .expect("projects should be an array");
        assert!(
            projects
                .iter()
                .any(|project| project["name"].as_str() == Some("phase1-project")),
            "Created project should be returned by list endpoint"
        );

        let delete_uri = format!("/api/v1/projects/{project_id}");
        let delete_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(delete_uri.as_str())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(delete_response.status(), StatusCode::OK);

        let list_after_delete = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/projects")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(list_after_delete.status(), StatusCode::OK);

        let payload_after_delete: serde_json::Value =
            serde_json::from_str(&response_text(list_after_delete).await)
                .expect("List projects response should be JSON");
        let projects_after_delete = payload_after_delete["projects"]
            .as_array()
            .expect("projects should be an array");
        assert!(
            projects_after_delete
                .iter()
                .all(|project| project["project_id"].as_str() != Some(project_id.as_str())),
            "Deleted project should not be returned by list endpoint"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_get_run_from_sqlite_after_in_memory_reset() {
        let store = Arc::new(InMemoryStore::new());
        let sqlite_store = Arc::new(
            SqliteStore::new(":memory:")
                .await
                .expect("Failed to create test SQLite store"),
        );
        let key_store = Arc::new(ApiKeyStore::new_dev_mode());
        let idempotency_store = Arc::new(IdempotencyStore::new());
        let cardinality_tracker = Arc::new(CardinalityTracker::default());

        let project_id = sqlite_store
            .get_or_create_project("test-project")
            .await
            .unwrap();
        sqlite_store
            .create_run(
                "run-sqlite-only",
                &project_id,
                Some("sqlite-only-run"),
                None,
                None,
            )
            .await
            .unwrap();

        let state = AppState {
            store,
            sqlite_store,
            key_store,
            idempotency_store,
            cardinality_tracker,
        };
        let app = build_http_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/runs/run-sqlite-only")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_ui_session_can_create_list_and_revoke_project_keys() {
        let harness = ui_session_harness_with_role("owner").await;
        let (_, foreign_key) = harness
            .key_store
            .create_key(
                Some(harness.secondary_project_id.clone()),
                Some("foreign-key".to_string()),
                vec!["read".to_string()],
                None,
            )
            .await;

        let jwt = build_test_jwt(&harness.jwt_secret, &harness.jwt_subject);
        let cookies = login_ui_session(&harness.app, &jwt).await;

        let create_response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/keys")
                    .header("content-type", "application/json")
                    .header(header::COOKIE, &cookies.cookie_header)
                    .header("x-csrf-token", &cookies.csrf_token)
                    .body(Body::from(
                        serde_json::json!({
                            "project_id": harness.primary_project_id.clone(),
                            "name": "sdk-write",
                            "scopes": ["read", "write"],
                            "expires_in_seconds": 3600
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build create key request"),
            )
            .await
            .expect("Create key request failed");
        assert_eq!(create_response.status(), StatusCode::OK);
        let created_payload: serde_json::Value =
            serde_json::from_str(&response_text(create_response).await)
                .expect("Create key response should be JSON");
        let created_key_id = created_payload["key_id"]
            .as_str()
            .expect("Create key response must include key_id")
            .to_string();
        assert!(
            created_payload["api_key"]
                .as_str()
                .expect("Create key response must include api_key")
                .starts_with("mlrunx_")
        );

        let stored_keys = harness
            .sqlite_store
            .list_api_keys(Some(&harness.primary_project_id))
            .await
            .expect("Failed to list keys from sqlite");
        let stored_created_key = stored_keys
            .iter()
            .find(|key| key.id == created_key_id)
            .expect("Created key should be persisted in sqlite");
        assert_eq!(
            stored_created_key.created_by_user_id.as_deref(),
            Some(harness.user_id.as_str()),
            "UI-created keys must retain owner user_id for run visibility"
        );

        let list_response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/keys")
                    .header(header::COOKIE, &cookies.cookie_header)
                    .body(Body::empty())
                    .expect("Failed to build list keys request"),
            )
            .await
            .expect("List keys request failed");
        assert_eq!(list_response.status(), StatusCode::OK);
        let list_payload: serde_json::Value =
            serde_json::from_str(&response_text(list_response).await)
                .expect("List keys response should be JSON");
        let keys = list_payload["keys"]
            .as_array()
            .expect("keys must be an array");
        assert!(keys.iter().any(|k| {
            k["key_id"]
                .as_str()
                .map_or(false, |key_id| key_id == created_key_id.as_str())
        }));
        assert!(keys.iter().all(|k| {
            k["key_id"]
                .as_str()
                .map_or(true, |key_id| key_id != foreign_key.id.as_str())
        }));

        let revoke_uri = format!("/api/v1/keys/{created_key_id}");
        let revoke_response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(revoke_uri.as_str())
                    .header(header::COOKIE, &cookies.cookie_header)
                    .header("x-csrf-token", &cookies.csrf_token)
                    .body(Body::empty())
                    .expect("Failed to build revoke key request"),
            )
            .await
            .expect("Revoke key request failed");
        assert_eq!(revoke_response.status(), StatusCode::OK);

        let list_after_revoke = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/keys")
                    .header(header::COOKIE, &cookies.cookie_header)
                    .body(Body::empty())
                    .expect("Failed to build list keys request"),
            )
            .await
            .expect("List keys request failed");
        assert_eq!(list_after_revoke.status(), StatusCode::OK);
        let list_after_revoke_payload: serde_json::Value =
            serde_json::from_str(&response_text(list_after_revoke).await)
                .expect("List keys response should be JSON");
        let keys_after_revoke = list_after_revoke_payload["keys"]
            .as_array()
            .expect("keys must be an array");
        let revoked = keys_after_revoke.iter().find(|k| {
            k["key_id"]
                .as_str()
                .map_or(false, |key_id| key_id == created_key_id.as_str())
        });
        assert!(revoked.is_some(), "Created key should still be listed");
        assert_eq!(
            revoked.and_then(|k| k["is_revoked"].as_bool()),
            Some(true),
            "Created key should be marked revoked"
        );
    }

    #[tokio::test]
    async fn test_ui_session_cannot_list_or_revoke_foreign_user_key_in_same_project() {
        let harness = ui_session_harness_with_role("owner").await;
        let foreign_user_id = harness
            .sqlite_store
            .get_or_create_user_identity(
                "jwt",
                "foreign-key-owner",
                Some("foreign-key-owner@example.com"),
                Some("Foreign Key Owner"),
            )
            .await
            .expect("Failed to create foreign user");
        let (_, foreign_key) = harness
            .key_store
            .create_key_with_owner(
                Some(harness.primary_project_id.clone()),
                Some("foreign-owned".to_string()),
                vec!["read".to_string()],
                Some(3600),
                Some(foreign_user_id),
            )
            .await;

        let jwt = build_test_jwt(&harness.jwt_secret, &harness.jwt_subject);
        let cookies = login_ui_session(&harness.app, &jwt).await;

        let list_response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/keys")
                    .header(header::COOKIE, &cookies.cookie_header)
                    .body(Body::empty())
                    .expect("Failed to build list keys request"),
            )
            .await
            .expect("List keys request failed");
        assert_eq!(list_response.status(), StatusCode::OK);
        let list_payload: serde_json::Value =
            serde_json::from_str(&response_text(list_response).await)
                .expect("List keys response should be JSON");
        let keys = list_payload["keys"]
            .as_array()
            .expect("keys should be an array");
        assert!(
            keys.iter().all(|k| {
                k["key_id"]
                    .as_str()
                    .map_or(true, |key_id| key_id != foreign_key.id.as_str())
            }),
            "Foreign-owned key in the same project must not be listed to this UI session"
        );

        let revoke_uri = format!("/api/v1/keys/{}", foreign_key.id);
        let revoke_response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(revoke_uri.as_str())
                    .header(header::COOKIE, &cookies.cookie_header)
                    .header("x-csrf-token", &cookies.csrf_token)
                    .body(Body::empty())
                    .expect("Failed to build revoke key request"),
            )
            .await
            .expect("Revoke key request failed");
        assert_eq!(revoke_response.status(), StatusCode::NOT_FOUND);

        let stored_foreign = harness
            .sqlite_store
            .list_api_keys(Some(&harness.primary_project_id))
            .await
            .expect("Failed to list keys")
            .into_iter()
            .find(|row| row.id == foreign_key.id)
            .expect("Expected foreign key to exist");
        assert!(
            stored_foreign.revoked_at.is_none(),
            "Foreign-owned key must remain active after unauthorized revoke attempt"
        );
    }

    #[tokio::test]
    async fn test_ui_session_cannot_create_key_for_unscoped_project() {
        let harness = ui_session_harness_with_role("owner").await;
        let jwt = build_test_jwt(&harness.jwt_secret, &harness.jwt_subject);
        let cookies = login_ui_session(&harness.app, &jwt).await;

        let response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/keys")
                    .header("content-type", "application/json")
                    .header(header::COOKIE, &cookies.cookie_header)
                    .header("x-csrf-token", &cookies.csrf_token)
                    .body(Body::from(
                        serde_json::json!({
                            "project_id": harness.secondary_project_id.clone(),
                            "name": "not-allowed",
                            "scopes": ["read"],
                            "expires_in_seconds": 3600
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build create key request"),
            )
            .await
            .expect("Create key request failed");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_ui_created_key_sdk_run_is_listed_with_metrics_in_ui() {
        let harness = ui_session_harness_with_role("owner").await;
        let jwt = build_test_jwt(&harness.jwt_secret, &harness.jwt_subject);
        let cookies = login_ui_session(&harness.app, &jwt).await;

        let create_key_response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/keys")
                    .header("content-type", "application/json")
                    .header(header::COOKIE, &cookies.cookie_header)
                    .header("x-csrf-token", &cookies.csrf_token)
                    .body(Body::from(
                        serde_json::json!({
                            "project_id": harness.primary_project_id.clone(),
                            "name": "sdk-run-key",
                            "scopes": ["read", "write"],
                            "expires_in_seconds": 3600
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build create key request"),
            )
            .await
            .expect("Create key request failed");
        assert_eq!(create_key_response.status(), StatusCode::OK);
        let created_key_payload: serde_json::Value =
            serde_json::from_str(&response_text(create_key_response).await)
                .expect("Create key response should be JSON");
        let sdk_api_key = created_key_payload["api_key"]
            .as_str()
            .expect("Create key response must include api_key")
            .to_string();

        let init_response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/runs")
                    .header("content-type", "application/json")
                    .header("x-api-key", &sdk_api_key)
                    .body(Body::from(
                        serde_json::json!({
                            "project_id": harness.primary_project_id.clone(),
                            "name": "sdk-visible-run"
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build init run request"),
            )
            .await
            .expect("Init run request failed");
        assert_eq!(init_response.status(), StatusCode::OK);
        let init_payload: serde_json::Value =
            serde_json::from_str(&response_text(init_response).await)
                .expect("Init run response should be JSON");
        let run_id = init_payload["run_id"]
            .as_str()
            .expect("Init run response must include run_id")
            .to_string();

        let ingest_response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ingest/batch")
                    .header("content-type", "application/json")
                    .header("x-api-key", &sdk_api_key)
                    .body(Body::from(
                        serde_json::json!({
                            "run_id": run_id,
                            "metrics": [
                                { "name": "loss", "value": 1.23, "step": 1 }
                            ],
                            "params": [],
                            "tags": [],
                            "events": []
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build ingest request"),
            )
            .await
            .expect("Ingest request failed");
        assert_eq!(ingest_response.status(), StatusCode::OK);

        let list_response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/runs")
                    .header(header::COOKIE, &cookies.cookie_header)
                    .body(Body::empty())
                    .expect("Failed to build list runs request"),
            )
            .await
            .expect("List runs request failed");
        assert_eq!(list_response.status(), StatusCode::OK);
        let list_payload: serde_json::Value =
            serde_json::from_str(&response_text(list_response).await)
                .expect("List runs response should be JSON");
        let listed_runs = list_payload["runs"]
            .as_array()
            .expect("runs must be an array");
        assert!(
            listed_runs
                .iter()
                .any(|run| { run["run_id"].as_str().map_or(false, |id| id == run_id) })
        );

        let metrics_uri = format!("/api/v1/runs/{run_id}/metrics");
        let metrics_response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(metrics_uri.as_str())
                    .header(header::COOKIE, &cookies.cookie_header)
                    .body(Body::empty())
                    .expect("Failed to build metrics request"),
            )
            .await
            .expect("Metrics request failed");
        assert_eq!(metrics_response.status(), StatusCode::OK);
        let metrics_payload: serde_json::Value =
            serde_json::from_str(&response_text(metrics_response).await)
                .expect("Metrics response should be JSON");
        let available_metrics = metrics_payload["available_metrics"]
            .as_array()
            .expect("available_metrics should be an array");
        assert!(
            available_metrics
                .iter()
                .any(|name| name.as_str().map_or(false, |value| value == "loss"))
        );
    }

    #[tokio::test]
    async fn test_ui_session_cannot_grant_scope_not_in_membership() {
        let harness = ui_session_harness_with_role("viewer").await;
        let jwt = build_test_jwt(&harness.jwt_secret, &harness.jwt_subject);
        let cookies = login_ui_session(&harness.app, &jwt).await;

        let response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/keys")
                    .header("content-type", "application/json")
                    .header(header::COOKIE, &cookies.cookie_header)
                    .header("x-csrf-token", &cookies.csrf_token)
                    .body(Body::from(
                        serde_json::json!({
                            "project_id": harness.primary_project_id.clone(),
                            "name": "viewer-write",
                            "scopes": ["write"],
                            "expires_in_seconds": 3600
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build create key request"),
            )
            .await
            .expect("Create key request failed");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_ui_session_cannot_create_admin_scoped_key() {
        let harness = ui_session_harness_with_role("owner").await;
        let jwt = build_test_jwt(&harness.jwt_secret, &harness.jwt_subject);
        let cookies = login_ui_session(&harness.app, &jwt).await;

        let response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/keys")
                    .header("content-type", "application/json")
                    .header(header::COOKIE, &cookies.cookie_header)
                    .header("x-csrf-token", &cookies.csrf_token)
                    .body(Body::from(
                        serde_json::json!({
                            "project_id": harness.primary_project_id.clone(),
                            "name": "owner-admin",
                            "scopes": ["admin"],
                            "expires_in_seconds": 3600
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build create key request"),
            )
            .await
            .expect("Create key request failed");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_ui_session_key_creation_requires_project_id() {
        let harness = ui_session_harness_with_role("owner").await;
        let jwt = build_test_jwt(&harness.jwt_secret, &harness.jwt_subject);
        let cookies = login_ui_session(&harness.app, &jwt).await;

        let response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/keys")
                    .header("content-type", "application/json")
                    .header(header::COOKIE, &cookies.cookie_header)
                    .header("x-csrf-token", &cookies.csrf_token)
                    .body(Body::from(
                        serde_json::json!({
                            "name": "proj-a/train-prod",
                            "scopes": ["read", "write"],
                            "expires_in_seconds": 3600
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build create key request"),
            )
            .await
            .expect("Create key request failed");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_ui_session_key_creation_rejects_invalid_name() {
        let harness = ui_session_harness_with_role("owner").await;
        let jwt = build_test_jwt(&harness.jwt_secret, &harness.jwt_subject);
        let cookies = login_ui_session(&harness.app, &jwt).await;

        let response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/keys")
                    .header("content-type", "application/json")
                    .header(header::COOKIE, &cookies.cookie_header)
                    .header("x-csrf-token", &cookies.csrf_token)
                    .body(Body::from(
                        serde_json::json!({
                            "project_id": harness.primary_project_id.clone(),
                            "name": "Invalid_Name",
                            "scopes": ["read", "write"],
                            "expires_in_seconds": 3600
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build create key request"),
            )
            .await
            .expect("Create key request failed");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_ui_session_key_creation_requires_ttl_within_policy_max() {
        let harness = ui_session_harness_with_role("owner").await;
        let jwt = build_test_jwt(&harness.jwt_secret, &harness.jwt_subject);
        let cookies = login_ui_session(&harness.app, &jwt).await;

        let missing_ttl = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/keys")
                    .header("content-type", "application/json")
                    .header(header::COOKIE, &cookies.cookie_header)
                    .header("x-csrf-token", &cookies.csrf_token)
                    .body(Body::from(
                        serde_json::json!({
                            "project_id": harness.primary_project_id.clone(),
                            "name": "proj-a/train-prod",
                            "scopes": ["read", "write"]
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build create key request"),
            )
            .await
            .expect("Create key request failed");
        assert_eq!(missing_ttl.status(), StatusCode::BAD_REQUEST);

        let too_long_ttl = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/keys")
                    .header("content-type", "application/json")
                    .header(header::COOKIE, &cookies.cookie_header)
                    .header("x-csrf-token", &cookies.csrf_token)
                    .body(Body::from(
                        serde_json::json!({
                            "project_id": harness.primary_project_id.clone(),
                            "name": "proj-a/train-prod",
                            "scopes": ["read", "write"],
                            "expires_in_seconds": DEFAULT_UI_KEY_MAX_TTL_SECONDS + 1
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build create key request"),
            )
            .await
            .expect("Create key request failed");
        assert_eq!(too_long_ttl.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_ui_session_only_lists_owned_runs() {
        let harness = ui_session_harness_with_role("owner").await;

        let foreign_user_id = harness
            .sqlite_store
            .get_or_create_user_identity(
                "jwt",
                "foreign-subject",
                Some("foreign@example.com"),
                Some("Foreign User"),
            )
            .await
            .expect("Failed to create foreign user");

        harness
            .sqlite_store
            .create_run(
                "run-owned-by-session-user",
                &harness.primary_project_id,
                Some("owned"),
                None,
                Some(&harness.user_id),
            )
            .await
            .expect("Failed to create owned run");
        harness
            .sqlite_store
            .create_run(
                "run-owned-by-foreign-user",
                &harness.primary_project_id,
                Some("foreign"),
                None,
                Some(&foreign_user_id),
            )
            .await
            .expect("Failed to create foreign run");

        let jwt = build_test_jwt(&harness.jwt_secret, &harness.jwt_subject);
        let cookies = login_ui_session(&harness.app, &jwt).await;

        let response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/runs")
                    .header(header::COOKIE, &cookies.cookie_header)
                    .body(Body::empty())
                    .expect("Failed to build list runs request"),
            )
            .await
            .expect("List runs request failed");

        assert_eq!(response.status(), StatusCode::OK);
        let payload: serde_json::Value = serde_json::from_str(&response_text(response).await)
            .expect("List runs response should be JSON");
        let runs = payload["runs"].as_array().expect("runs should be an array");

        assert!(runs.iter().any(|run| {
            run["run_id"]
                .as_str()
                .map_or(false, |id| id == "run-owned-by-session-user")
        }));
        assert!(!runs.iter().any(|run| {
            run["run_id"]
                .as_str()
                .map_or(false, |id| id == "run-owned-by-foreign-user")
        }));
    }

    #[tokio::test]
    async fn test_ui_session_cannot_read_foreign_owned_run() {
        let harness = ui_session_harness_with_role("owner").await;

        let foreign_user_id = harness
            .sqlite_store
            .get_or_create_user_identity(
                "jwt",
                "foreign-subject-2",
                Some("foreign2@example.com"),
                Some("Foreign User 2"),
            )
            .await
            .expect("Failed to create foreign user");

        harness
            .sqlite_store
            .create_run(
                "run-foreign-read-test",
                &harness.primary_project_id,
                Some("foreign"),
                None,
                Some(&foreign_user_id),
            )
            .await
            .expect("Failed to create foreign run");

        let jwt = build_test_jwt(&harness.jwt_secret, &harness.jwt_subject);
        let cookies = login_ui_session(&harness.app, &jwt).await;

        let response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/runs/run-foreign-read-test")
                    .header(header::COOKIE, &cookies.cookie_header)
                    .body(Body::empty())
                    .expect("Failed to build get run request"),
            )
            .await
            .expect("Get run request failed");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_platform_admin_ui_session_lists_all_runs_across_users_and_projects() {
        let harness = ui_session_harness_with_role("owner").await;

        let foreign_user_id = harness
            .sqlite_store
            .get_or_create_user_identity(
                "jwt",
                "foreign-admin-list-subject",
                Some("foreign-admin-list@example.com"),
                Some("Foreign Admin List User"),
            )
            .await
            .expect("Failed to create foreign user");

        harness
            .sqlite_store
            .create_run(
                "run-foreign-primary-admin-list",
                &harness.primary_project_id,
                Some("foreign-primary"),
                None,
                Some(&foreign_user_id),
            )
            .await
            .expect("Failed to create primary run");
        harness
            .sqlite_store
            .create_run(
                "run-foreign-secondary-admin-list",
                &harness.secondary_project_id,
                Some("foreign-secondary"),
                None,
                Some(&foreign_user_id),
            )
            .await
            .expect("Failed to create secondary run");

        let jwt = build_test_jwt(&harness.jwt_secret, &harness.jwt_subject);
        let cookies = login_ui_session(&harness.app, &jwt).await;
        promote_ui_user_to_platform_admin(&harness).await;

        let response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/runs")
                    .header(header::COOKIE, &cookies.cookie_header)
                    .body(Body::empty())
                    .expect("Failed to build list runs request"),
            )
            .await
            .expect("List runs request failed");

        assert_eq!(response.status(), StatusCode::OK);
        let payload: serde_json::Value = serde_json::from_str(&response_text(response).await)
            .expect("List runs response should be JSON");
        let runs = payload["runs"].as_array().expect("runs should be an array");

        assert!(runs.iter().any(|run| {
            run["run_id"]
                .as_str()
                .map_or(false, |id| id == "run-foreign-primary-admin-list")
        }));
        assert!(runs.iter().any(|run| {
            run["run_id"]
                .as_str()
                .map_or(false, |id| id == "run-foreign-secondary-admin-list")
        }));
    }

    #[tokio::test]
    async fn test_platform_admin_ui_session_can_read_foreign_run() {
        let harness = ui_session_harness_with_role("owner").await;

        let foreign_user_id = harness
            .sqlite_store
            .get_or_create_user_identity(
                "jwt",
                "foreign-admin-read-subject",
                Some("foreign-admin-read@example.com"),
                Some("Foreign Admin Read User"),
            )
            .await
            .expect("Failed to create foreign user");

        harness
            .sqlite_store
            .create_run(
                "run-foreign-admin-read",
                &harness.secondary_project_id,
                Some("foreign"),
                None,
                Some(&foreign_user_id),
            )
            .await
            .expect("Failed to create foreign run");

        let jwt = build_test_jwt(&harness.jwt_secret, &harness.jwt_subject);
        let cookies = login_ui_session(&harness.app, &jwt).await;
        promote_ui_user_to_platform_admin(&harness).await;

        let response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/runs/run-foreign-admin-read")
                    .header(header::COOKIE, &cookies.cookie_header)
                    .body(Body::empty())
                    .expect("Failed to build get run request"),
            )
            .await
            .expect("Get run request failed");

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_ui_editor_can_delete_own_run_without_admin_scope() {
        let harness = ui_session_harness_with_role("editor").await;
        harness
            .sqlite_store
            .create_run(
                "run-editor-delete",
                &harness.primary_project_id,
                Some("editor-owned"),
                None,
                Some(&harness.user_id),
            )
            .await
            .expect("Failed to create editor-owned run");

        let jwt = build_test_jwt(&harness.jwt_secret, &harness.jwt_subject);
        let cookies = login_ui_session(&harness.app, &jwt).await;

        let response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/v1/runs/run-editor-delete")
                    .header(header::COOKIE, &cookies.cookie_header)
                    .header("x-csrf-token", &cookies.csrf_token)
                    .body(Body::empty())
                    .expect("Failed to build delete run request"),
            )
            .await
            .expect("Delete run request failed");

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_ui_session_cannot_delete_foreign_owned_run() {
        let harness = ui_session_harness_with_role("owner").await;

        let foreign_user_id = harness
            .sqlite_store
            .get_or_create_user_identity(
                "jwt",
                "foreign-delete-subject",
                Some("foreign-delete@example.com"),
                Some("Foreign Delete User"),
            )
            .await
            .expect("Failed to create foreign user");

        harness
            .sqlite_store
            .create_run(
                "run-foreign-delete-test",
                &harness.primary_project_id,
                Some("foreign-delete"),
                None,
                Some(&foreign_user_id),
            )
            .await
            .expect("Failed to create foreign run");

        let jwt = build_test_jwt(&harness.jwt_secret, &harness.jwt_subject);
        let cookies = login_ui_session(&harness.app, &jwt).await;

        let delete_response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/v1/runs/run-foreign-delete-test")
                    .header(header::COOKIE, &cookies.cookie_header)
                    .header("x-csrf-token", &cookies.csrf_token)
                    .body(Body::empty())
                    .expect("Failed to build delete run request"),
            )
            .await
            .expect("Delete run request failed");

        assert_eq!(delete_response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_ui_owner_can_delete_owned_project() {
        let harness = ui_session_harness_with_role("owner").await;
        let jwt = build_test_jwt(&harness.jwt_secret, &harness.jwt_subject);
        let cookies = login_ui_session(&harness.app, &jwt).await;

        let create_response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/projects")
                    .header("content-type", "application/json")
                    .header(header::COOKIE, &cookies.cookie_header)
                    .header("x-csrf-token", &cookies.csrf_token)
                    .body(Body::from(
                        serde_json::json!({
                            "name": "owned-delete-project",
                            "description": "owner can delete"
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build create project request"),
            )
            .await
            .expect("Create project request failed");
        assert_eq!(create_response.status(), StatusCode::OK);
        let created_payload: serde_json::Value =
            serde_json::from_str(&response_text(create_response).await)
                .expect("Create project response should be JSON");
        let project_id = created_payload["project_id"]
            .as_str()
            .expect("project_id should be present")
            .to_string();

        let delete_uri = format!("/api/v1/projects/{project_id}");
        let delete_response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(delete_uri.as_str())
                    .header(header::COOKIE, &cookies.cookie_header)
                    .header("x-csrf-token", &cookies.csrf_token)
                    .body(Body::empty())
                    .expect("Failed to build delete project request"),
            )
            .await
            .expect("Delete project request failed");
        assert_eq!(delete_response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_ui_editor_cannot_delete_project_without_owner_role() {
        let harness = ui_session_harness_with_role("editor").await;
        let jwt = build_test_jwt(&harness.jwt_secret, &harness.jwt_subject);
        let cookies = login_ui_session(&harness.app, &jwt).await;

        let delete_uri = format!("/api/v1/projects/{}", harness.primary_project_id);
        let response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(delete_uri.as_str())
                    .header(header::COOKIE, &cookies.cookie_header)
                    .header("x-csrf-token", &cookies.csrf_token)
                    .body(Body::empty())
                    .expect("Failed to build delete project request"),
            )
            .await
            .expect("Delete project request failed");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_global_admin_key_can_delete_project() {
        let harness = ui_session_harness_with_role("owner").await;
        let project = harness
            .sqlite_store
            .create_project("global-admin-delete-project", None)
            .await
            .expect("Failed to create project");

        let (global_admin_key, _) = harness
            .key_store
            .create_key(
                None,
                Some("platform-admin".to_string()),
                vec!["admin".to_string()],
                None,
            )
            .await;

        let delete_uri = format!("/api/v1/projects/{}", project.id);
        let response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(delete_uri.as_str())
                    .header("x-api-key", &global_admin_key)
                    .body(Body::empty())
                    .expect("Failed to build delete project request"),
            )
            .await
            .expect("Delete project request failed");

        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            harness
                .sqlite_store
                .get_project_by_id(&project.id)
                .await
                .expect("Failed to query project")
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_platform_admin_endpoints_require_global_admin_key() {
        let harness = ui_session_harness_with_role("owner").await;
        let (scoped_admin_key, _) = harness
            .key_store
            .create_key(
                Some(harness.primary_project_id.clone()),
                Some("project-admin".to_string()),
                vec!["admin".to_string()],
                None,
            )
            .await;
        let (global_admin_key, _) = harness
            .key_store
            .create_key(
                None,
                Some("platform-admin".to_string()),
                vec!["admin".to_string()],
                None,
            )
            .await;

        let forbidden = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/admin/users")
                    .header("x-api-key", scoped_admin_key)
                    .body(Body::empty())
                    .expect("Failed to build admin users request"),
            )
            .await
            .expect("Admin users request failed");
        assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

        let allowed = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/admin/users")
                    .header("x-api-key", global_admin_key)
                    .body(Body::empty())
                    .expect("Failed to build admin users request"),
            )
            .await
            .expect("Admin users request failed");
        assert_eq!(allowed.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_non_admin_ui_session_cannot_access_admin_or_session_endpoints() {
        let harness = ui_session_harness_with_role("owner").await;
        let jwt = build_test_jwt(&harness.jwt_secret, &harness.jwt_subject);
        let cookies = login_ui_session(&harness.app, &jwt).await;

        for (method, uri) in [
            ("GET", "/api/v1/admin/users"),
            ("GET", "/api/v1/admin/sessions"),
            ("GET", "/api/v1/admin/audit-events"),
        ] {
            let request = Request::builder()
                .method(method)
                .uri(uri)
                .header(header::COOKIE, &cookies.cookie_header)
                .body(Body::empty())
                .expect("Failed to build non-admin admin request");

            let response = harness
                .app
                .clone()
                .oneshot(request)
                .await
                .expect("Non-admin admin request failed");
            assert_eq!(
                response.status(),
                StatusCode::FORBIDDEN,
                "expected FORBIDDEN for {method} {uri}"
            );
        }
    }

    #[tokio::test]
    async fn test_non_admin_ui_session_cannot_revoke_foreign_user_session() {
        let harness = ui_session_harness_with_role("owner").await;

        let foreign_subject = "foreign-session-subject";
        let foreign_jwt = build_test_jwt(&harness.jwt_secret, foreign_subject);
        let foreign_cookies = login_ui_session(&harness.app, &foreign_jwt).await;
        let foreign_user_id = harness
            .sqlite_store
            .get_or_create_user_identity(
                "jwt",
                foreign_subject,
                Some("foreign-session@example.com"),
                Some("Foreign Session User"),
            )
            .await
            .expect("Failed to resolve foreign user identity");

        let foreign_session_id = harness
            .sqlite_store
            .list_auth_sessions_for_admin(Some(&foreign_user_id), false)
            .await
            .expect("Failed to list foreign sessions")
            .into_iter()
            .find(|session| session.revoked_at.is_none())
            .map(|session| session.id)
            .expect("Expected active foreign session");

        let jwt = build_test_jwt(&harness.jwt_secret, &harness.jwt_subject);
        let cookies = login_ui_session(&harness.app, &jwt).await;

        let revoke_uri = format!("/api/v1/admin/sessions/{foreign_session_id}/revoke");
        let revoke_response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(revoke_uri.as_str())
                    .header(header::COOKIE, &cookies.cookie_header)
                    .header("x-csrf-token", &cookies.csrf_token)
                    .body(Body::empty())
                    .expect("Failed to build revoke foreign session request"),
            )
            .await
            .expect("Revoke foreign session request failed");
        assert_eq!(revoke_response.status(), StatusCode::FORBIDDEN);

        let foreign_session_response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/ui-auth/session")
                    .header(header::COOKIE, &foreign_cookies.cookie_header)
                    .body(Body::empty())
                    .expect("Failed to build foreign session status request"),
            )
            .await
            .expect("Foreign session status request failed");
        assert_eq!(foreign_session_response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_platform_admin_can_disable_and_enable_user() {
        let harness = ui_session_harness_with_role("owner").await;
        let (global_admin_key, _) = harness
            .key_store
            .create_key(
                None,
                Some("platform-admin".to_string()),
                vec!["admin".to_string()],
                None,
            )
            .await;

        let disable_uri = format!("/api/v1/admin/users/{}/disable", harness.user_id);
        let disable_response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(disable_uri.as_str())
                    .header("x-api-key", &global_admin_key)
                    .body(Body::empty())
                    .expect("Failed to build disable request"),
            )
            .await
            .expect("Disable request failed");
        assert_eq!(disable_response.status(), StatusCode::OK);
        let disabled_payload: serde_json::Value =
            serde_json::from_str(&response_text(disable_response).await)
                .expect("Disable response should be JSON");
        assert_eq!(disabled_payload["disabled"].as_bool(), Some(true));

        let enable_uri = format!("/api/v1/admin/users/{}/enable", harness.user_id);
        let enable_response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(enable_uri.as_str())
                    .header("x-api-key", &global_admin_key)
                    .body(Body::empty())
                    .expect("Failed to build enable request"),
            )
            .await
            .expect("Enable request failed");
        assert_eq!(enable_response.status(), StatusCode::OK);
        let enabled_payload: serde_json::Value =
            serde_json::from_str(&response_text(enable_response).await)
                .expect("Enable response should be JSON");
        assert_eq!(enabled_payload["disabled"].as_bool(), Some(false));
    }

    #[tokio::test]
    async fn test_platform_admin_can_list_and_revoke_ui_sessions() {
        let harness = ui_session_harness_with_role("owner").await;
        let jwt = build_test_jwt(&harness.jwt_secret, &harness.jwt_subject);
        let cookies = login_ui_session(&harness.app, &jwt).await;

        let (global_admin_key, _) = harness
            .key_store
            .create_key(
                None,
                Some("platform-admin".to_string()),
                vec!["admin".to_string()],
                None,
            )
            .await;

        let sessions_uri = format!(
            "/api/v1/admin/sessions?user_id={}&include_revoked=true",
            harness.user_id
        );
        let list_response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(sessions_uri.as_str())
                    .header("x-api-key", &global_admin_key)
                    .body(Body::empty())
                    .expect("Failed to build list sessions request"),
            )
            .await
            .expect("List sessions request failed");
        assert_eq!(list_response.status(), StatusCode::OK);
        let list_payload: serde_json::Value =
            serde_json::from_str(&response_text(list_response).await)
                .expect("List sessions response should be JSON");
        let sessions = list_payload["sessions"]
            .as_array()
            .expect("sessions should be an array");
        let session_id = sessions
            .iter()
            .find_map(|session| {
                if session["user_id"].as_str() == Some(harness.user_id.as_str())
                    && session["revoked_at"].is_null()
                {
                    session["session_id"].as_str().map(|id| id.to_string())
                } else {
                    None
                }
            })
            .expect("Expected at least one active session for test user");

        let revoke_uri = format!("/api/v1/admin/sessions/{session_id}/revoke");
        let revoke_response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(revoke_uri.as_str())
                    .header("x-api-key", &global_admin_key)
                    .body(Body::empty())
                    .expect("Failed to build revoke session request"),
            )
            .await
            .expect("Revoke session request failed");
        assert_eq!(revoke_response.status(), StatusCode::OK);

        // The revoked session cookie should no longer authenticate.
        let session_response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/ui-auth/session")
                    .header(header::COOKIE, &cookies.cookie_header)
                    .body(Body::empty())
                    .expect("Failed to build session check request"),
            )
            .await
            .expect("Session check request failed");
        assert_eq!(session_response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_platform_admin_can_list_audit_events() {
        let harness = ui_session_harness_with_role("owner").await;
        let (global_admin_key, _) = harness
            .key_store
            .create_key(
                None,
                Some("platform-admin".to_string()),
                vec!["admin".to_string()],
                None,
            )
            .await;

        // Prime at least one audit event under the admin namespace.
        let seed_response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/admin/users")
                    .header("x-api-key", &global_admin_key)
                    .body(Body::empty())
                    .expect("Failed to build admin users request"),
            )
            .await
            .expect("Admin users request failed");
        assert_eq!(seed_response.status(), StatusCode::OK);

        let audit_response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/admin/audit-events?action=admin.users.list&limit=20")
                    .header("x-api-key", &global_admin_key)
                    .body(Body::empty())
                    .expect("Failed to build admin audit request"),
            )
            .await
            .expect("Admin audit request failed");
        assert_eq!(audit_response.status(), StatusCode::OK);

        let payload: serde_json::Value = serde_json::from_str(&response_text(audit_response).await)
            .expect("Admin audit response should be JSON");
        let events = payload["events"]
            .as_array()
            .expect("events should be an array");
        assert!(
            events
                .iter()
                .any(|event| event["action"].as_str() == Some("admin.users.list")),
            "Expected admin.users.list audit entry"
        );
    }

    #[tokio::test]
    async fn test_share_links_disabled_by_default() {
        let _policy = set_share_link_policy_override_for_tests(Some((false, 1)));

        let harness = ui_session_harness_with_role("owner").await;
        harness
            .sqlite_store
            .create_run(
                "run-share-disabled-default",
                &harness.primary_project_id,
                Some("share-disabled"),
                None,
                Some(&harness.user_id),
            )
            .await
            .expect("Failed to create share-disabled run");

        let jwt = build_test_jwt(&harness.jwt_secret, &harness.jwt_subject);
        let cookies = login_ui_session(&harness.app, &jwt).await;

        let create_response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/runs/run-share-disabled-default/share")
                    .header("content-type", "application/json")
                    .header(header::COOKIE, &cookies.cookie_header)
                    .header("x-csrf-token", &cookies.csrf_token)
                    .body(Body::from(
                        serde_json::json!({
                            "expires_in_days": 1
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build share create request"),
            )
            .await
            .expect("Share create request failed");

        assert_eq!(create_response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_share_links_enforce_bounded_ttl_policy() {
        let _policy = set_share_link_policy_override_for_tests(Some((true, 1)));

        let harness = ui_session_harness_with_role("owner").await;
        harness
            .sqlite_store
            .create_run(
                "run-share-ttl-policy",
                &harness.primary_project_id,
                Some("share-ttl"),
                None,
                Some(&harness.user_id),
            )
            .await
            .expect("Failed to create share policy run");

        let jwt = build_test_jwt(&harness.jwt_secret, &harness.jwt_subject);
        let cookies = login_ui_session(&harness.app, &jwt).await;

        let bad_response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/runs/run-share-ttl-policy/share")
                    .header("content-type", "application/json")
                    .header(header::COOKIE, &cookies.cookie_header)
                    .header("x-csrf-token", &cookies.csrf_token)
                    .body(Body::from(
                        serde_json::json!({
                            "expires_in_days": 2
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build over-ttl share request"),
            )
            .await
            .expect("Over-ttl share request failed");
        assert_eq!(bad_response.status(), StatusCode::BAD_REQUEST);
        let bad_body = response_text(bad_response).await;
        assert!(
            bad_body.contains("exceeds policy maximum of 1 days"),
            "expected bounded TTL policy error, got: {bad_body}"
        );

        let ok_response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/runs/run-share-ttl-policy/share")
                    .header("content-type", "application/json")
                    .header(header::COOKIE, &cookies.cookie_header)
                    .header("x-csrf-token", &cookies.csrf_token)
                    .body(Body::from(
                        serde_json::json!({
                            "expires_in_days": 1
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build valid share request"),
            )
            .await
            .expect("Valid share request failed");
        assert_eq!(ok_response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_share_link_revoke_invalidates_token_and_hashes_at_rest() {
        let _policy = set_share_link_policy_override_for_tests(Some((true, 1)));

        let harness = ui_session_harness_with_role("owner").await;
        harness
            .sqlite_store
            .create_run(
                "run-share-revoke",
                &harness.primary_project_id,
                Some("share-revoke"),
                None,
                Some(&harness.user_id),
            )
            .await
            .expect("Failed to create share revoke run");

        let jwt = build_test_jwt(&harness.jwt_secret, &harness.jwt_subject);
        let cookies = login_ui_session(&harness.app, &jwt).await;

        let create_response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/runs/run-share-revoke/share")
                    .header("content-type", "application/json")
                    .header(header::COOKIE, &cookies.cookie_header)
                    .header("x-csrf-token", &cookies.csrf_token)
                    .body(Body::from(
                        serde_json::json!({
                            "expires_in_days": 1
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build share create request"),
            )
            .await
            .expect("Share create request failed");
        assert_eq!(create_response.status(), StatusCode::OK);
        let create_payload: serde_json::Value =
            serde_json::from_str(&response_text(create_response).await)
                .expect("Share create response should be JSON");
        let token = create_payload["token"]
            .as_str()
            .expect("share token should exist")
            .to_string();

        let stored_tokens = harness
            .sqlite_store
            .list_share_tokens("run-share-revoke")
            .await
            .expect("Failed to list share tokens");
        let expected_hash = harness.key_store.hmac_fingerprint(&token);
        assert!(stored_tokens.iter().any(|row| row.token == expected_hash));
        assert!(
            stored_tokens.iter().all(|row| row.token != token),
            "share token must not be stored in plaintext"
        );

        let shared_response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/v1/shared/{token}"))
                    .body(Body::empty())
                    .expect("Failed to build shared run request"),
            )
            .await
            .expect("Shared run request failed");
        assert_eq!(shared_response.status(), StatusCode::OK);

        let revoke_response = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/runs/run-share-revoke/share/{token}"))
                    .header(header::COOKIE, &cookies.cookie_header)
                    .header("x-csrf-token", &cookies.csrf_token)
                    .body(Body::empty())
                    .expect("Failed to build share revoke request"),
            )
            .await
            .expect("Share revoke request failed");
        assert_eq!(revoke_response.status(), StatusCode::OK);

        let shared_after_revoke = harness
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/v1/shared/{token}"))
                    .body(Body::empty())
                    .expect("Failed to build shared run request"),
            )
            .await
            .expect("Shared run request failed");
        assert_eq!(shared_after_revoke.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_protected_run_init_requires_auth_when_not_in_dev_mode() {
        let app = test_app_with_auth_enabled().await;
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/runs")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"project": "auth-project", "name": "auth-test"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_project_key_cannot_delete_run_owned_by_different_key() {
        let store = Arc::new(InMemoryStore::new());
        let sqlite_store = Arc::new(
            SqliteStore::new(":memory:")
                .await
                .expect("Failed to create test SQLite store"),
        );
        let key_store = Arc::new(ApiKeyStore::new_with_sqlite(sqlite_store.clone()));
        let idempotency_store = Arc::new(IdempotencyStore::new());
        let cardinality_tracker = Arc::new(CardinalityTracker::default());

        let project_id = sqlite_store
            .create_project("api-key-delete-test", None)
            .await
            .expect("Failed to create project")
            .id;
        let (owner_raw_key, owner_key) = key_store
            .create_key(
                Some(project_id.clone()),
                Some("owner-key".to_string()),
                vec!["read".to_string(), "write".to_string()],
                None,
            )
            .await;
        let (other_raw_key, _) = key_store
            .create_key(
                Some(project_id.clone()),
                Some("other-key".to_string()),
                vec!["read".to_string(), "write".to_string()],
                None,
            )
            .await;

        sqlite_store
            .create_run(
                "run-owned-by-owner-key",
                &project_id,
                Some("owned"),
                Some(&owner_key.id),
                None,
            )
            .await
            .expect("Failed to create owned run");

        let state = AppState {
            store,
            sqlite_store,
            key_store,
            idempotency_store,
            cardinality_tracker,
        };
        let app = build_http_router(state);

        let forbidden = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/v1/runs/run-owned-by-owner-key")
                    .header("x-api-key", &other_raw_key)
                    .body(Body::empty())
                    .expect("Failed to build delete request"),
            )
            .await
            .expect("Delete request failed");
        assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

        let allowed = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/v1/runs/run-owned-by-owner-key")
                    .header("x-api-key", &owner_raw_key)
                    .body(Body::empty())
                    .expect("Failed to build delete request"),
            )
            .await
            .expect("Delete request failed");
        assert_eq!(allowed.status(), StatusCode::OK);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_ingest_batch_requires_run_in_sqlite_even_if_present_in_memory() {
        let store = Arc::new(InMemoryStore::new());
        let sqlite_store = Arc::new(
            SqliteStore::new(":memory:")
                .await
                .expect("Failed to create test SQLite store"),
        );
        let key_store = Arc::new(ApiKeyStore::new_dev_mode());
        let idempotency_store = Arc::new(IdempotencyStore::new());
        let cardinality_tracker = Arc::new(CardinalityTracker::default());

        // Insert run in memory only; SQLite intentionally does not contain this run.
        store.runs.write().await.insert(
            "run-memory-only".to_string(),
            services::ingest::RunState {
                run_id: "run-memory-only".to_string(),
                project_id: "project-memory".to_string(),
                name: Some("memory-only".to_string()),
                status: mlrunx_proto::mlrunx::v1::RunStatus::Running,
                created_at: std::time::SystemTime::now(),
                updated_at: std::time::SystemTime::now(),
                metrics_count: 0,
                params_count: 0,
                tags: std::collections::HashMap::new(),
            },
        );

        let state = AppState {
            store,
            sqlite_store,
            key_store,
            idempotency_store,
            cardinality_tracker,
        };
        let app = build_http_router(state);

        let body = r#"{
            "run_id": "run-memory-only",
            "batch_id": "batch-1",
            "seq": 1,
            "metrics": [{"name": "loss", "value": 0.5, "step": 1}],
            "params": [],
            "tags": []
        }"#;

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ingest/batch")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
