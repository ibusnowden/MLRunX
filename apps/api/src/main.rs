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
    CardinalityTracker, EventPayload, IdempotencyResult, IdempotencyStore, IngestServiceImpl,
    MetricPayload, ParamPayload, TagPayload, compute_payload_hash, ingest::InMemoryStore,
};
use storage::{
    AuditEventRow, AuthSessionAdminRow, CreateProjectInput, CreateRunInput, MetricRow,
    ProjectRepository, ProjectRow, RunEventInput, RunEventRow, RunRepository, RunRow,
    RunStatus as PostgresRunStatus, SqliteStore, UserProjectMembershipRow, UserRow,
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

#[derive(Debug, Deserialize)]
struct UiAuthLoginRequest {
    #[serde(default)]
    jwt: Option<String>,
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
    if auth.is_dev_mode || !auth.is_ui_jwt() {
        return Ok(());
    }

    let user_id = auth_user_id(auth).ok_or((
        StatusCode::FORBIDDEN,
        "Unable to resolve user identity for run authorization.".to_string(),
    ))?;

    if let Some(owner_user_id) = run.created_by_user_id.as_deref() {
        if owner_user_id != user_id {
            return Err((
                StatusCode::FORBIDDEN,
                "Access denied: this user cannot access that run.".to_string(),
            ));
        }
    }

    Ok(())
}

async fn require_ui_project_owner(
    state: &AppState,
    auth: &AuthContext,
    project_id: &str,
) -> Result<(), (StatusCode, String)> {
    if auth.is_dev_mode || !auth.is_ui_jwt() {
        return Ok(());
    }

    let user_id = auth_user_id(auth).ok_or((
        StatusCode::FORBIDDEN,
        "Unable to resolve user identity for project authorization.".to_string(),
    ))?;

    let memberships = state
        .sqlite_store
        .list_active_project_memberships(&user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

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

    let tags_json = tags
        .map(|t| serde_json::to_value(t).unwrap_or_else(|_| serde_json::json!({})))
        .unwrap_or_else(|| serde_json::json!({}));

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
    let status_value = match postgres_run_status_from_http(status) {
        Some(value) => value,
        None => {
            warn!(
                run_id = %run_id,
                status = %status,
                "Skipped PostgreSQL run status shadow write for unknown status"
            );
            return;
        }
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

    let user_agent = header_string(&headers, "user-agent");
    let client_ip = infer_client_ip(&headers);

    let issue = state
        .key_store
        .create_ui_session_from_jwt(&jwt, user_agent.as_deref(), client_ip.as_deref())
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

    let rows = if auth.is_global() && !auth.is_ui_jwt() {
        state
            .sqlite_store
            .list_projects()
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    } else if let Some(allowed_projects) = auth.allowed_project_ids() {
        let project_ids: Vec<String> = allowed_projects.iter().cloned().collect();
        state
            .sqlite_store
            .list_projects_by_ids(&project_ids)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    } else if let Some(project_id) = auth.project_id() {
        state
            .sqlite_store
            .list_projects_by_ids(&[project_id.to_string()])
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    } else {
        Vec::new()
    };

    let projects = rows.into_iter().map(project_response_from_row).collect();
    Ok(Json(ListProjectsResponse { projects }))
}

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
        let user_id = auth_user_id(&auth).ok_or((
            StatusCode::FORBIDDEN,
            "Unable to resolve user identity for project creation.".to_string(),
        ))?;

        let row = state
            .sqlite_store
            .create_project(name, req.description.as_deref())
            .await
            .map_err(|e| {
                if is_unique_project_name_error(&e) {
                    (
                        StatusCode::CONFLICT,
                        format!("Project '{}' already exists.", name),
                    )
                } else {
                    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
                }
            })?;

        state
            .sqlite_store
            .grant_project_membership(&row.id, &user_id, "owner", Some(&user_id))
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
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
                    format!("Project '{}' already exists.", name),
                )
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
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
    let project = state
        .sqlite_store
        .get_project_by_id(&project_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((
            StatusCode::NOT_FOUND,
            format!("Project not found: '{project_id}'"),
        ))?;

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
            _ => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
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
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
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
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
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
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
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
    require_platform_admin_access(&state, &auth, "admin.user.disable", "user", Some(&user_id))
        .await?;

    let user_exists = state
        .sqlite_store
        .get_user_by_id(&user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
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
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let user = state
        .sqlite_store
        .get_user_by_id(&user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((
            StatusCode::NOT_FOUND,
            format!("User not found: '{user_id}'"),
        ))?;

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
    require_platform_admin_access(&state, &auth, "admin.user.enable", "user", Some(&user_id))
        .await?;

    let user_exists = state
        .sqlite_store
        .get_user_by_id(&user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
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
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let user = state
        .sqlite_store
        .get_user_by_id(&user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((
            StatusCode::NOT_FOUND,
            format!("User not found: '{user_id}'"),
        ))?;

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
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
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
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
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
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
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
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
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
    let project_id = req.project_id.as_ref().ok_or((
        StatusCode::BAD_REQUEST,
        "project_id is required. Create a project first via POST /api/v1/projects.".to_string(),
    ))?;
    let resolved_project_name = state
        .sqlite_store
        .get_project_name_by_id(project_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
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
    maybe_shadow_write_run_to_postgres(&run_id, project_id, req.name.as_deref(), req.tags.as_ref())
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
        Some(&project_id),
        Some(&run_id),
        "run.init",
        "run",
        Some(&run_id),
        "success",
        serde_json::json!({
            "project_name": resolved_project_name,
            "project_id": project_id,
            "created_by_user_id": created_by_user_id,
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
struct LogEventData {
    level: Option<String>,
    source: Option<String>,
    message: String,
    step: Option<i64>,
    timestamp: Option<f64>,
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
    let max_len = 2000usize;
    if trimmed.len() <= max_len {
        Some(trimmed.to_string())
    } else {
        Some(trimmed.chars().take(max_len).collect())
    }
}

/// Ingest a batch of events via HTTP (for SDK HTTP transport).
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
            _ => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
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
    let metric_count = req.metrics.len();
    let param_count = req.params.len();
    let tag_count = req.tags.len();
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
            metric_count as i32,
            param_count as i32,
            tag_count as i32,
            event_count as i32,
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

    // Validate run status from SQLite (single source of truth for HTTP paths).
    let run = state
        .sqlite_store
        .get_run(&req.run_id)
        .await
        .map_err(|e| match e {
            storage::SqliteError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
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
    let sqlite_metrics: Vec<MetricRow> = req
        .metrics
        .iter()
        .filter(|m| accepted_metrics.contains(&m.name))
        .filter_map(|m| {
            let finite_value = m.value.is_finite();
            let finite_timestamp = m.timestamp.map(|ts| ts.is_finite()).unwrap_or(true);
            if finite_value && finite_timestamp {
                Some(MetricRow {
                    name: m.name.clone(),
                    step: m.step,
                    value: m.value,
                    timestamp: m.timestamp,
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
            .increment_metrics_count(&req.run_id, accepted_metric_count as i64)
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

    let mut dropped_event_count = 0usize;
    let sqlite_events: Vec<RunEventInput> = req
        .events
        .iter()
        .filter_map(|event| {
            let message = match sanitize_run_event_message(&event.message) {
                Some(message) => message,
                None => {
                    dropped_event_count += 1;
                    return None;
                }
            };
            let timestamp = match event.timestamp {
                Some(value) if !value.is_finite() => {
                    dropped_event_count += 1;
                    return None;
                }
                value => value,
            };
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
    let run = state
        .sqlite_store
        .get_run(&run_id)
        .await
        .map_err(|e| match e {
            storage::SqliteError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
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
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
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
    // Verify the caller can access the run's project and has write scope.
    // Ownership checks below prevent cross-user run deletion.
    let run = state
        .sqlite_store
        .get_run(&run_id)
        .await
        .map_err(|e| match e {
            storage::SqliteError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
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
            _ => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
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
    let run = state
        .sqlite_store
        .get_run(&run_id)
        .await
        .map_err(|e| match e {
            storage::SqliteError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
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
        Some(&run.project_id),
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
    let run = state
        .sqlite_store
        .get_run(&run_id)
        .await
        .map_err(|e| match e {
            storage::SqliteError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        })?;
    require_endpoint_access(
        &state,
        &auth,
        EndpointRbacTier::Read,
        Some(&run.project_id),
        Some(&run_id),
        "share_token.revoke",
        "share_token",
        Some(&token),
    )
    .await?;
    require_ui_run_owner(&auth, &run)?;
    require_api_key_run_owner_for_mutation(&auth, &run, "revoke share links for")?;

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
        Some(&run.project_id),
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

    let owner_user_filter = if auth.is_ui_jwt() {
        Some(auth_user_id(&auth).ok_or((
            StatusCode::FORBIDDEN,
            "Unable to resolve user identity for run listing.".to_string(),
        ))?)
    } else {
        None
    };

    // Query from SQLite
    let (sqlite_runs, total) = state
        .sqlite_store
        .list_runs(
            effective_project.as_deref(),
            owner_user_filter.as_deref(),
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

#[derive(Debug, Deserialize)]
struct RunEventsQuery {
    after_id: Option<i64>,
    #[serde(default = "default_run_events_limit")]
    limit: usize,
}

fn default_run_events_limit() -> usize {
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

/// Get metrics for a run with optional downsampling.
async fn http_get_metrics(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    axum::extract::Path(run_id): axum::extract::Path<String>,
    axum::extract::Query(query): axum::extract::Query<MetricsQuery>,
) -> Result<Json<services::MetricsQueryResponse>, (StatusCode, String)> {
    // Verify run exists, check project access, and require read scope
    let run = state
        .sqlite_store
        .get_run(&run_id)
        .await
        .map_err(|_| (StatusCode::NOT_FOUND, format!("Run not found: {}", run_id)))?;
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

/// Get structured run events for log/timeline display.
async fn http_get_run_events(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    axum::extract::Path(run_id): axum::extract::Path<String>,
    axum::extract::Query(query): axum::extract::Query<RunEventsQuery>,
) -> Result<Json<ListRunEventsResponse>, (StatusCode, String)> {
    let run = state
        .sqlite_store
        .get_run(&run_id)
        .await
        .map_err(|_| (StatusCode::NOT_FOUND, format!("Run not found: {}", run_id)))?;

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
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

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
    let ui_jwt_enabled = state.key_store.is_ui_jwt_enabled();

    let cors = if ui_jwt_enabled {
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
        // Query API endpoints
        .route("/api/v1/runs", get(http_list_runs))
        .route(
            "/api/v1/runs/{run_id}",
            get(http_get_run).delete(http_delete_run),
        )
        .route("/api/v1/runs/{run_id}/metrics", get(http_get_metrics))
        .route("/api/v1/runs/{run_id}/events", get(http_get_run_events))
        .route("/api/v1/runs/compare", post(http_compare_runs))
        // Key management endpoints (admin only)
        .route("/api/v1/keys", post(http_create_key).get(http_list_keys))
        .route("/api/v1/keys/{key_id}", delete(http_revoke_key))
        // Share token management (requires auth)
        .route("/api/v1/runs/{run_id}/share", post(http_create_share_token))
        .route(
            "/api/v1/runs/{run_id}/share/{token}",
            delete(http_revoke_share_token),
        );

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
        .route("/health", get(health))
        // Shared run endpoints (public, no auth — token is the credential)
        .route("/api/v1/shared/{token}", get(http_get_shared_run))
        .route(
            "/api/v1/shared/{token}/metrics",
            get(http_get_shared_metrics),
        );

    if ui_jwt_enabled {
        public_routes = public_routes.route("/api/v1/ui-auth/login", post(http_ui_auth_login));
    }

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

    if key_store.is_auth_disabled() && is_production_environment() {
        panic!(
            "Refusing to start: authentication is disabled in production. \
Set MLRUNX_AUTH_MODE=api_key or MLRUNX_AUTH_MODE=hybrid and restart."
        );
    }
    if key_store.is_auth_disabled() {
        warn!("Authentication is disabled; use only in development/test environments.");
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

    async fn login_ui_session_with_bearer(app: &Router, jwt: &str) -> SessionCookies {
        let login_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/ui-auth/login")
                    .header("authorization", format!("Bearer {jwt}"))
                    .header("content-type", "application/json")
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

    async fn response_text(response: axum::response::Response) -> String {
        let body = response
            .into_body()
            .collect()
            .await
            .expect("Failed to read response body")
            .to_bytes();
        String::from_utf8(body.to_vec()).expect("Response body is not valid UTF-8")
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
            )
            .await;
        let (global_admin_key, _) = harness
            .key_store
            .create_key(
                None,
                Some("platform-admin".to_string()),
                vec!["admin".to_string()],
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
    async fn test_platform_admin_can_disable_and_enable_user() {
        let harness = ui_session_harness_with_role("owner").await;
        let (global_admin_key, _) = harness
            .key_store
            .create_key(
                None,
                Some("platform-admin".to_string()),
                vec!["admin".to_string()],
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
            )
            .await;
        let (other_raw_key, _) = key_store
            .create_key(
                Some(project_id.clone()),
                Some("other-key".to_string()),
                vec!["read".to_string(), "write".to_string()],
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
