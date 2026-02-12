//! MLRunX API Server
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
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    dead_code,
    unused_imports
)] // Scaffolding crate; tighten lints as API wiring stabilizes.

mod auth;
mod config;
mod services;
mod storage;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    Extension, Json, Router,
    extract::State,
    http::{
        HeaderMap, HeaderValue, Method, StatusCode,
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
use mlrunx_proto::mlrunx::v1::ingest_service_server::IngestServiceServer;
use services::{
    CardinalityTracker, IdempotencyResult, IdempotencyStore, IngestServiceImpl, MetricPayload,
    ParamPayload, TagPayload, compute_payload_hash, ingest::InMemoryStore,
};
use storage::{MetricRow, SqliteStore};

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
    fn scope(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Admin => "admin",
        }
    }

    fn env_flag_name(self) -> &'static str {
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

#[derive(Debug, Deserialize)]
struct UiAuthLoginRequest {
    jwt: String,
}

#[derive(Debug, Serialize)]
struct UiAuthLoginResponse {
    status: String,
    user_id: String,
    expires_at: String,
    project_count: usize,
}

#[derive(Debug, Serialize)]
struct UiAuthSessionResponse {
    authenticated: bool,
    auth_mode: String,
    scopes: Vec<String>,
    project_ids: Vec<String>,
    key_prefix: String,
    is_dev_mode: bool,
}

#[derive(Debug, Serialize)]
struct UiAuthLogoutResponse {
    status: String,
}

fn env_flag(name: &str) -> bool {
    std::env::var(name).map_or(false, |v| {
        v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes")
    })
}

fn env_flag_default(name: &str, default_value: bool) -> bool {
    std::env::var(name).map_or(default_value, |v| {
        v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes")
    })
}

fn auth_mode_label(auth: &AuthContext) -> &'static str {
    match auth.auth_mode {
        AuthMode::ApiKey => "api_key",
        AuthMode::UiJwt => "ui_jwt",
    }
}

fn audit_actor_ids(auth: &AuthContext) -> (Option<String>, Option<String>) {
    if auth.is_dev_mode {
        return (None, None);
    }

    if auth.is_ui_jwt() {
        let user_id = auth
            .api_key
            .id
            .split_once(':')
            .map(|(_, value)| value.to_string());
        (user_id, None)
    } else {
        (None, Some(auth.api_key.id.clone()))
    }
}

fn should_enforce_scope(auth: &AuthContext, tier: EndpointRbacTier) -> bool {
    if auth.is_dev_mode {
        return false;
    }
    if !auth.is_ui_jwt() {
        return true;
    }

    if !env_flag_default("MLRUNX_RBAC_ENDPOINT_ENFORCEMENT_ENABLED", true) {
        return false;
    }
    env_flag_default(tier.env_flag_name(), true)
}

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

fn infer_client_ip(headers: &HeaderMap) -> Option<String> {
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

fn build_cookie(
    name: &str,
    value: &str,
    ttl_seconds: u64,
    http_only: bool,
    secure: bool,
    same_site: &str,
) -> String {
    let mut cookie = format!("{name}={value}; Path=/; Max-Age={ttl_seconds}; SameSite={same_site}");
    if http_only {
        cookie.push_str("; HttpOnly");
    }
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

fn build_clear_cookie(name: &str, secure: bool, same_site: &str) -> String {
    let mut cookie = format!(
        "{name}=; Path=/; Max-Age=0; Expires=Thu, 01 Jan 1970 00:00:00 GMT; SameSite={same_site}"
    );
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

async fn http_ui_auth_login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<UiAuthLoginRequest>,
) -> Result<(HeaderMap, Json<UiAuthLoginResponse>), (StatusCode, String)> {
    if req.jwt.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "JWT is required.".to_string()));
    }

    let user_agent = header_string(&headers, "user-agent");
    let client_ip = infer_client_ip(&headers);

    let issue = state
        .key_store
        .create_ui_session_from_jwt(&req.jwt, user_agent.as_deref(), client_ip.as_deref())
        .await
        .map_err(|e| (StatusCode::UNAUTHORIZED, e))?;

    let secure_cookie = state.key_store.ui_cookie_secure();
    let same_site = state.key_store.ui_cookie_same_site();
    let ttl = state.key_store.ui_session_ttl_seconds();

    let session_cookie = build_cookie(
        state.key_store.ui_session_cookie_name(),
        &issue.session_token,
        ttl,
        true,
        secure_cookie,
        same_site,
    );
    let csrf_cookie = build_cookie(
        state.key_store.ui_csrf_cookie_name(),
        &issue.csrf_token,
        ttl,
        false,
        secure_cookie,
        same_site,
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

    let clear_session_cookie = build_clear_cookie(&session_cookie_name, secure_cookie, &same_site);
    let clear_csrf_cookie = build_clear_cookie(&csrf_cookie_name, secure_cookie, &same_site);

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
    project: String,
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

/// Initialize a run via HTTP (for SDK HTTP transport).
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
        // Verify the caller can access this existing run's project with write scope
        if let Ok(existing_project) = state.sqlite_store.get_run_project_id(&run_id).await {
            require_endpoint_access(
                &state,
                &auth,
                EndpointRbacTier::Write,
                Some(&existing_project),
                Some(&run_id),
                "run.init",
                "run",
                Some(&run_id),
            )
            .await?;
        }
        return Ok(Json(InitRunHttpResponse {
            run_id,
            offline: false,
        }));
    }

    // Get or create project in SQLite
    let project_id = state
        .sqlite_store
        .get_or_create_project(&req.project)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Enforce project scope and write permission
    require_endpoint_access(
        &state,
        &auth,
        EndpointRbacTier::Write,
        Some(&project_id),
        Some(&run_id),
        "run.init",
        "run",
        Some(&run_id),
    )
    .await?;

    // Create run in SQLite
    state
        .sqlite_store
        .create_run(&run_id, &project_id, req.name.as_deref())
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Set initial tags if provided
    if let Some(tags) = &req.tags {
        let tag_pairs: Vec<(String, String)> =
            tags.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        state
            .sqlite_store
            .set_tags(&run_id, &tag_pairs)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    // Also maintain in-memory state for backward compatibility
    let now = std::time::SystemTime::now();
    let run_state = services::ingest::RunState {
        run_id: run_id.clone(),
        project_id: req.project.clone(),
        name: req.name.clone(),
        status: mlrunx_proto::mlrunx::v1::RunStatus::Running,
        created_at: now,
        updated_at: now,
        metrics_count: 0,
        params_count: 0,
        tags: req.tags.clone().unwrap_or_default(),
    };

    let mut runs = state.store.runs.write().await;
    runs.insert(run_id.clone(), run_state);

    info!(run_id = %run_id, project = %req.project, "HTTP: Initialized run (SQLite)");

    emit_audit_event(
        &state,
        Some(&auth),
        Some(&project_id),
        Some(&run_id),
        "run.init",
        "run",
        Some(&run_id),
        "success",
        serde_json::json!({
            "project_name": req.project,
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
    metrics: Vec<MetricData>,
    params: Vec<ParamData>,
    tags: Vec<TagData>,
    #[allow(dead_code)]
    timestamp: Option<f64>,
    #[allow(dead_code)]
    stats: Option<BatchStats>,
}

#[derive(Debug, Deserialize)]
struct MetricData {
    name: String,
    value: f64,
    step: i64,
    timestamp: Option<f64>,
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

/// Ingest a batch of events via HTTP (for SDK HTTP transport).
async fn http_ingest_batch(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(req): Json<IngestBatchHttpRequest>,
) -> Result<Json<IngestBatchHttpResponse>, (StatusCode, String)> {
    // Resolve run project first and fail closed when the run does not exist.
    let run_project = state
        .sqlite_store
        .get_run_project_id(&req.run_id)
        .await
        .map_err(|e| match e {
            storage::SqliteError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        })?;

    // Verify the caller can access the run's project and has write scope.
    require_endpoint_access(
        &state,
        &auth,
        EndpointRbacTier::Write,
        Some(&run_project),
        Some(&req.run_id),
        "run.ingest",
        "run",
        Some(&req.run_id),
    )
    .await?;
    // Generate batch_id if not provided
    let batch_id = req
        .batch_id
        .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
    let seq = req.seq.unwrap_or(0);

    // Convert request data for hashing
    let metric_payloads: Vec<MetricPayload> = req
        .metrics
        .iter()
        .map(|m| MetricPayload {
            name: m.name.clone(),
            value: m.value,
            step: m.step,
        })
        .collect();

    let param_payloads: Vec<ParamPayload> = req
        .params
        .iter()
        .map(|p| ParamPayload {
            name: p.name.clone(),
            value: p.value.clone(),
        })
        .collect();

    let tag_payloads: Vec<TagPayload> = req
        .tags
        .iter()
        .map(|t| TagPayload {
            key: t.key.clone(),
            value: t.value.clone(),
        })
        .collect();

    // Compute payload hash for idempotency
    let payload_hash = compute_payload_hash(&metric_payloads, &param_payloads, &tag_payloads);

    // Check and record for idempotency
    let metric_count = req.metrics.len();
    let param_count = req.params.len();
    let tag_count = req.tags.len();

    let project_id = run_project;

    let idempotency_result = state
        .idempotency_store
        .check_and_record(
            &project_id,
            &req.run_id,
            &batch_id,
            seq,
            &payload_hash,
            metric_count as i32,
            param_count as i32,
            tag_count as i32,
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
                    "Batch {} conflicts with existing batch (expected hash {}, got {})",
                    batch_id, expected_hash, actual_hash
                ),
            ));
        }
        IdempotencyResult::OutOfOrder {
            expected_seq,
            actual_seq,
        } => {
            warnings.push(format!(
                "Batch received out of order (expected seq >= {}, got {})",
                expected_seq, actual_seq
            ));
        }
        IdempotencyResult::New => {
            // New batch - proceed normally
        }
    }

    // Validate cardinality limits
    let tags_for_validation: Vec<(String, String)> = req
        .tags
        .iter()
        .map(|t| (t.key.clone(), t.value.clone()))
        .collect();
    let metric_names: Vec<String> = req.metrics.iter().map(|m| m.name.clone()).collect();

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

    // Now process the batch
    let mut runs = state.store.runs.write().await;

    let run = runs.get_mut(&req.run_id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            format!("Run not found: {}", req.run_id),
        )
    })?;

    if run.status != mlrunx_proto::mlrunx::v1::RunStatus::Running {
        return Err((
            StatusCode::PRECONDITION_FAILED,
            format!("Run {} is not running", req.run_id),
        ));
    }

    // Count only accepted items
    let accepted_metric_count = req
        .metrics
        .iter()
        .filter(|m| accepted_metrics.contains(&m.name))
        .count();
    let accepted_tag_count = accepted_tags.len();

    // Persist metrics to SQLite
    if accepted_metric_count > 0 {
        let sqlite_metrics: Vec<MetricRow> = req
            .metrics
            .iter()
            .filter(|m| accepted_metrics.contains(&m.name))
            .map(|m| MetricRow {
                name: m.name.clone(),
                step: m.step,
                value: m.value,
                timestamp: m.timestamp,
            })
            .collect();

        if let Err(e) = state
            .sqlite_store
            .insert_metrics(&req.run_id, &sqlite_metrics)
            .await
        {
            warn!(error = %e, "Failed to persist metrics to SQLite");
        }

        // Also update metrics count in SQLite
        if let Err(e) = state
            .sqlite_store
            .increment_metrics_count(&req.run_id, accepted_metric_count as i64)
            .await
        {
            warn!(error = %e, "Failed to update metrics count in SQLite");
        }

        // Also maintain in-memory for backward compatibility
        let mut metrics_store = state.store.metrics.write().await;
        let run_metrics = metrics_store
            .entry(req.run_id.clone())
            .or_insert_with(services::RunMetrics::new);

        for metric in req
            .metrics
            .iter()
            .filter(|m| accepted_metrics.contains(&m.name))
        {
            run_metrics.add_point(services::MetricPoint {
                name: metric.name.clone(),
                step: metric.step,
                value: metric.value,
                timestamp: metric.timestamp,
            });
        }
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
        let param_pairs: Vec<(String, String)> = req
            .params
            .iter()
            .map(|p| (p.name.clone(), p.value.clone()))
            .collect();
        if let Err(e) = state
            .sqlite_store
            .insert_params(&req.run_id, &param_pairs)
            .await
        {
            warn!(error = %e, "Failed to persist params to SQLite");
        }
    }

    run.metrics_count += accepted_metric_count as u64;
    run.params_count += param_count as u64;

    // Update tags (only accepted ones) in memory
    for (key, value) in accepted_tags {
        run.tags.insert(key.clone(), value.clone());
    }

    run.updated_at = std::time::SystemTime::now();

    let total = accepted_metric_count + param_count + accepted_tag_count;
    let dropped = validation.dropped_tags.len() + validation.dropped_metrics.len();

    tracing::debug!(
        run_id = %req.run_id,
        batch_id = %batch_id,
        seq = seq,
        metrics = accepted_metric_count,
        params = param_count,
        tags = accepted_tag_count,
        dropped = dropped,
        "HTTP: Ingested batch (SQLite)"
    );

    Ok(Json(IngestBatchHttpResponse {
        status: "ok".to_string(),
        accepted: total as i64,
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
    // Verify the caller can access the run's project and has write scope
    let run_project = state
        .sqlite_store
        .get_run_project_id(&run_id)
        .await
        .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;
    require_endpoint_access(
        &state,
        &auth,
        EndpointRbacTier::Write,
        Some(&run_project),
        Some(&run_id),
        "run.finish",
        "run",
        Some(&run_id),
    )
    .await?;

    // Update in SQLite
    state
        .sqlite_store
        .finish_run(&run_id, &req.status)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Also update in-memory for backward compatibility
    let mut runs = state.store.runs.write().await;
    if let Some(run) = runs.get_mut(&run_id) {
        run.status = match req.status.as_str() {
            "finished" => mlrunx_proto::mlrunx::v1::RunStatus::Finished,
            "failed" => mlrunx_proto::mlrunx::v1::RunStatus::Failed,
            "killed" => mlrunx_proto::mlrunx::v1::RunStatus::Killed,
            _ => mlrunx_proto::mlrunx::v1::RunStatus::Finished,
        };
        run.updated_at = std::time::SystemTime::now();
    }

    info!(run_id = %run_id, status = %req.status, "HTTP: Finished run (SQLite)");

    emit_audit_event(
        &state,
        Some(&auth),
        Some(&run_project),
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
    // Verify the caller can access the run's project and has admin scope
    let run_project = state
        .sqlite_store
        .get_run_project_id(&run_id)
        .await
        .map_err(|e| match e {
            storage::SqliteError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        })?;
    require_endpoint_access(
        &state,
        &auth,
        EndpointRbacTier::Admin,
        Some(&run_project),
        Some(&run_id),
        "run.delete",
        "run",
        Some(&run_id),
    )
    .await?;

    // Delete from SQLite (cascades to metrics, tags, params, batches)
    state
        .sqlite_store
        .delete_run(&run_id)
        .await
        .map_err(|e| match e {
            storage::SqliteError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        })?;

    // Also remove from in-memory store
    state.store.runs.write().await.remove(&run_id);
    state.store.metrics.write().await.remove(&run_id);

    info!(run_id = %run_id, "HTTP: Deleted run");

    emit_audit_event(
        &state,
        Some(&auth),
        Some(&run_project),
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
                format!(
                    "Invalid scope '{}'. Valid scopes: admin, write, read",
                    scope
                ),
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
    let allowed_projects = auth.allowed_project_ids().ok_or((
        StatusCode::FORBIDDEN,
        "UI session is missing project memberships.".to_string(),
    ))?;

    if let Some(project_id) = requested_project_id {
        if allowed_projects.contains(project_id) {
            return Ok(project_id.to_string());
        }
        return Err((
            StatusCode::FORBIDDEN,
            format!(
                "Access denied: cannot manage API keys for project '{}'.",
                project_id
            ),
        ));
    }

    if allowed_projects.len() == 1 {
        if let Some(project_id) = allowed_projects.iter().next() {
            return Ok(project_id.clone());
        }
    }

    Err((
        StatusCode::BAD_REQUEST,
        "project_id is required when your account has multiple project memberships.".to_string(),
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
                format!(
                    "Insufficient permissions: your UI session cannot grant '{}' scope.",
                    scope
                ),
            ));
        }
    }
    Ok(())
}

fn filter_keys_to_ui_memberships(auth: &AuthContext, keys: Vec<auth::ApiKey>) -> Vec<auth::ApiKey> {
    let Some(allowed_projects) = auth.allowed_project_ids() else {
        return Vec::new();
    };

    keys.into_iter()
        .filter(|key| {
            key.project_id
                .as_ref()
                .map_or(false, |project_id| allowed_projects.contains(project_id))
        })
        .collect()
}

/// Create a new API key.
async fn http_create_key(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(req): Json<CreateKeyRequest>,
) -> Result<Json<CreateKeyResponse>, (StatusCode, String)> {
    let normalized_scopes = normalize_requested_scopes(&req.scopes)?;
    let mut target_project_id = req.project_id.clone();

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

    let (raw_key, key) = state
        .key_store
        .create_key(
            target_project_id.clone(),
            req.name.clone(),
            normalized_scopes.clone(),
        )
        .await;

    info!(
        key_prefix = %key.key_prefix,
        project_id = ?target_project_id,
        scopes = ?normalized_scopes,
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
        }),
    )
    .await;

    Ok(Json(CreateKeyResponse {
        api_key: raw_key,
        key_id: key.id,
        key_prefix: key.key_prefix,
        project_id: key.project_id,
        name: key.name,
        scopes: key.scopes,
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

/// Revoke an API key by its key_id.
async fn http_revoke_key(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    axum::extract::Path(key_id): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
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
        None => Err((StatusCode::NOT_FOUND, format!("Key not found: {}", key_id))),
    }
}

// =============================================================================
// Share Token API Handlers
// =============================================================================

/// Request to create a share link.
#[derive(Debug, Deserialize)]
struct CreateShareRequest {
    /// Number of days until the link expires (None = never)
    expires_in_days: Option<i64>,
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

/// Create a share token for a run.
async fn http_create_share_token(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    axum::extract::Path(run_id): axum::extract::Path<String>,
    Json(req): Json<CreateShareRequest>,
) -> Result<Json<CreateShareResponse>, (StatusCode, String)> {
    // Verify the caller can access the run
    let run_project = state
        .sqlite_store
        .get_run_project_id(&run_id)
        .await
        .map_err(|e| match e {
            storage::SqliteError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        })?;
    require_endpoint_access(
        &state,
        &auth,
        EndpointRbacTier::Read,
        Some(&run_project),
        Some(&run_id),
        "share_token.create",
        "run",
        Some(&run_id),
    )
    .await?;

    // Generate a short, URL-safe token
    let token = generate_share_token();

    // Calculate expiry
    let expires_at = req.expires_in_days.map(|days| {
        let expires = chrono::Utc::now() + chrono::Duration::days(days);
        expires.format("%Y-%m-%d %H:%M:%S").to_string()
    });

    state
        .sqlite_store
        .create_share_token(
            &token,
            &run_id,
            Some(&auth.api_key.key_prefix),
            expires_at.as_deref(),
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    info!(run_id = %run_id, "Created share token");

    emit_audit_event(
        &state,
        Some(&auth),
        Some(&run_project),
        Some(&run_id),
        "share_token.create",
        "share_token",
        Some(&token),
        "success",
        serde_json::json!({
            "expires_at": expires_at.clone(),
        }),
    )
    .await;

    Ok(Json(CreateShareResponse {
        share_url: format!("/api/v1/shared/{}", token),
        token,
        run_id,
        expires_at,
    }))
}

/// Get a shared run via token (PUBLIC — no auth required).
async fn http_get_shared_run(
    State(state): State<AppState>,
    axum::extract::Path(token): axum::extract::Path<String>,
) -> Result<Json<SharedRunResponse>, (StatusCode, String)> {
    // Validate the share token
    let share = state
        .sqlite_store
        .validate_share_token(&token)
        .await
        .map_err(|e| match e {
            storage::SqliteError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        })?;

    // Fetch the run
    let run = state
        .sqlite_store
        .get_run(&share.run_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

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
        metrics_count: run.metrics_count as u64,
        params_count: run.params_count as u64,
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
    // Validate the share token
    let share = state
        .sqlite_store
        .validate_share_token(&token)
        .await
        .map_err(|e| match e {
            storage::SqliteError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
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
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

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
    // Verify the caller can access the run
    let run_project = state
        .sqlite_store
        .get_run_project_id(&run_id)
        .await
        .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;
    require_endpoint_access(
        &state,
        &auth,
        EndpointRbacTier::Read,
        Some(&run_project),
        Some(&run_id),
        "share_token.revoke",
        "share_token",
        Some(&token),
    )
    .await?;

    state
        .sqlite_store
        .revoke_share_token(&token)
        .await
        .map_err(|e| match e {
            storage::SqliteError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        })?;

    info!(run_id = %run_id, "Revoked share token");

    emit_audit_event(
        &state,
        Some(&auth),
        Some(&run_project),
        Some(&run_id),
        "share_token.revoke",
        "share_token",
        Some(&token),
        "success",
        serde_json::json!({}),
    )
    .await;

    Ok(Json(
        serde_json::json!({ "status": "ok", "revoked": token }),
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
    /// Filter by run status
    status: Option<String>,
    /// Free-text search query
    q: Option<String>,
    /// Comma-separated tag filters (key or key=value)
    tags: Option<String>,
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

/// List runs with optional filtering (queries from SQLite).
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

    // Enforce project scope:
    // - API key scoped callers: existing behavior.
    // - UI JWT callers: project must be one of the user's active memberships.
    let effective_project = if let Some(allowed_projects) = auth.allowed_project_ids() {
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
                    format!(
                        "Access denied: this user is not a member of project '{}'.",
                        requested
                    ),
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
                                "Access denied: your key is scoped to project '{}', cannot query '{}'.",
                                scoped_project, requested
                            ),
                        ));
                    }
                }
                Some(scoped_project.to_string())
            }
            None => query.project.clone(), // Admin/dev: use whatever was requested.
        }
    };

    // Query from SQLite
    let (sqlite_runs, total) = state
        .sqlite_store
        .list_runs(
            effective_project.as_deref(),
            query.status.as_deref(),
            query.q.as_deref(),
            limit,
            offset,
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

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
            metrics_count: run.metrics_count as u64,
            params_count: run.params_count as u64,
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
    // Run list is sourced from SQLite; run detail must use the same source.
    let run = state
        .sqlite_store
        .get_run(&run_id)
        .await
        .map_err(|e| match e {
            storage::SqliteError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
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
        metrics_count: run.metrics_count as u64,
        params_count: run.params_count as u64,
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

fn default_max_points() -> usize {
    1000
}

/// Get metrics for a run with optional downsampling.
async fn http_get_metrics(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    axum::extract::Path(run_id): axum::extract::Path<String>,
    axum::extract::Query(query): axum::extract::Query<MetricsQuery>,
) -> Result<Json<services::MetricsQueryResponse>, (StatusCode, String)> {
    // Verify run exists, check project access, and require read scope
    let run_project = state
        .sqlite_store
        .get_run_project_id(&run_id)
        .await
        .map_err(|_| (StatusCode::NOT_FOUND, format!("Run not found: {}", run_id)))?;
    require_endpoint_access(
        &state,
        &auth,
        EndpointRbacTier::Read,
        Some(&run_project),
        Some(&run_id),
        "run.metrics.read",
        "run",
        Some(&run_id),
    )
    .await?;

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
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

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
}

fn default_alignment() -> String {
    "step".to_string()
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

    if req.run_ids.len() > 100 {
        return Err((
            StatusCode::BAD_REQUEST,
            "Maximum 100 runs can be compared".to_string(),
        ));
    }

    // Collect data for each run
    let mut runs_data = Vec::new();
    let mut all_metric_sets: Vec<std::collections::HashSet<String>> = Vec::new();

    for run_id in &req.run_ids {
        let run = state
            .sqlite_store
            .get_run(run_id)
            .await
            .map_err(|e| match e {
                storage::SqliteError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
                _ => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
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

        let names = if req.metric_names.is_empty() {
            vec![]
        } else {
            req.metric_names.clone()
        };

        let sqlite_series = state
            .sqlite_store
            .get_metrics(run_id, &names, req.max_points)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

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
    }))
}

// =============================================================================
// Server Setup
// =============================================================================

fn build_http_router(state: AppState) -> Router {
    let cors = if env_flag("MLRUNX_UI_JWT_AUTH_ENABLED") {
        let mut allowed_origins: Vec<HeaderValue> = std::env::var("MLRUNX_UI_ALLOWED_ORIGINS")
            .ok()
            .map(|raw| {
                raw.split(',')
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .filter_map(|origin| HeaderValue::from_str(origin).ok())
                    .collect()
            })
            .unwrap_or_default();

        if allowed_origins.is_empty() {
            allowed_origins.push(HeaderValue::from_static("http://localhost:3000"));
            allowed_origins.push(HeaderValue::from_static("http://127.0.0.1:3000"));
        }

        CorsLayer::new()
            .allow_credentials(true)
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
            ])
    } else {
        CorsLayer::permissive()
    };
    let decompression = RequestDecompressionLayer::new();

    // Routes that require authentication
    let protected_routes = Router::new()
        // UI auth session endpoints (JWT/session path)
        .route("/api/v1/ui-auth/session", get(http_ui_auth_session))
        .route("/api/v1/ui-auth/logout", post(http_ui_auth_logout))
        // SDK HTTP transport endpoints (ingestion)
        .route("/api/v1/runs", post(http_init_run))
        .route("/api/v1/ingest/batch", post(http_ingest_batch))
        .route("/api/v1/runs/{run_id}/finish", post(http_finish_run))
        // Query API endpoints
        .route("/api/v1/runs", get(http_list_runs))
        .route(
            "/api/v1/runs/{run_id}",
            get(http_get_run).delete(http_delete_run),
        )
        .route("/api/v1/runs/{run_id}/metrics", get(http_get_metrics))
        .route("/api/v1/runs/compare", post(http_compare_runs))
        // Key management endpoints (admin only)
        .route("/api/v1/keys", post(http_create_key).get(http_list_keys))
        .route("/api/v1/keys/{key_id}", delete(http_revoke_key))
        // Share token management (requires auth)
        .route("/api/v1/runs/{run_id}/share", post(http_create_share_token))
        .route(
            "/api/v1/runs/{run_id}/share/{token}",
            delete(http_revoke_share_token),
        )
        .layer(middleware::from_fn_with_state(
            state.key_store.clone(),
            auth_middleware,
        ));

    // Public routes (no auth required)
    let public_routes = Router::new()
        .route("/", get(root))
        .route("/health", get(health))
        // UI auth bootstrap endpoint: exchange JWT for secure session cookies.
        .route("/api/v1/ui-auth/login", post(http_ui_auth_login))
        // Shared run endpoints (public, no auth — token is the credential)
        .route("/api/v1/shared/{token}", get(http_get_shared_run))
        .route(
            "/api/v1/shared/{token}/metrics",
            get(http_get_shared_metrics),
        );

    // Combine routes
    Router::new()
        .merge(public_routes)
        .merge(protected_routes)
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
        .map_or(false, |v| v == "1" || v.eq_ignore_ascii_case("true"));
    let key_store = if use_in_memory_keys {
        info!("Using in-memory API key store (MLRUNX_API_KEYS_IN_MEMORY enabled)");
        Arc::new(ApiKeyStore::new())
    } else {
        Arc::new(ApiKeyStore::new_with_sqlite(sqlite_store.clone()))
    };
    key_store.init_from_env().await;

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
        if let Err(e) = axum::serve(http_listener, http_app).await {
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
            sqlite_store,
            key_store: key_store.clone(),
            idempotency_store,
            cardinality_tracker,
        };

        UiSessionHarness {
            app: build_http_router(state),
            key_store,
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

    async fn response_text(response: axum::response::Response) -> String {
        let body = response
            .into_body()
            .collect()
            .await
            .expect("Failed to read response body")
            .to_bytes();
        String::from_utf8(body.to_vec()).expect("Response body is not valid UTF-8")
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

    #[tokio::test]
    async fn test_init_run_http() {
        let app = test_app().await;
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/runs")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"project": "test-project", "name": "test-run"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
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
            .create_run("run-sqlite-only", &project_id, Some("sqlite-only-run"))
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
                            "scopes": ["read", "write"]
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
                            "scopes": ["read"]
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
                            "scopes": ["write"]
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
                            "scopes": ["admin"]
                        })
                        .to_string(),
                    ))
                    .expect("Failed to build create key request"),
            )
            .await
            .expect("Create key request failed");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
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
