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
    http::StatusCode,
    middleware,
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use tonic::transport::Server as TonicServer;
use tower_http::{cors::CorsLayer, decompression::RequestDecompressionLayer};
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use auth::{ApiKeyStore, AuthContext, auth_middleware};
use mlrunx_proto::mlrunx::v1::ingest_service_server::IngestServiceServer;
use services::{
    CardinalityTracker, IdempotencyResult, IdempotencyStore, IngestServiceImpl, MetricPayload,
    ParamPayload, TagPayload, compute_payload_hash, ingest::InMemoryStore,
};
use storage::{SqliteStore, MetricRow};

/// Application state shared across handlers.
#[derive(Clone)]
pub struct AppState {
    store: Arc<InMemoryStore>,
    sqlite_store: Arc<SqliteStore>,
    key_store: Arc<ApiKeyStore>,
    idempotency_store: Arc<IdempotencyStore>,
    cardinality_tracker: Arc<CardinalityTracker>,
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
    if state.sqlite_store.run_exists(&run_id).await.unwrap_or(false) {
        // Verify the caller can access this existing run's project with write scope
        if let Ok(existing_project) = state.sqlite_store.get_run_project_id(&run_id).await {
            auth.require_access(&existing_project, "write")?;
        }
        return Ok(Json(InitRunHttpResponse {
            run_id,
            offline: false,
        }));
    }

    // Get or create project in SQLite
    let project_id = state.sqlite_store
        .get_or_create_project(&req.project)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Enforce project scope and write permission
    auth.require_access(&project_id, "write")?;

    // Create run in SQLite
    state.sqlite_store
        .create_run(&run_id, &project_id, req.name.as_deref())
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Set initial tags if provided
    if let Some(tags) = &req.tags {
        let tag_pairs: Vec<(String, String)> = tags.iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        state.sqlite_store
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
    // Verify the caller can access the run's project and has write scope
    if let Ok(run_project) = state.sqlite_store.get_run_project_id(&req.run_id).await {
        auth.require_access(&run_project, "write")?;
    }
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

    // Get project_id from run (read lock first)
    let project_id = {
        let runs = state.store.runs.read().await;
        let run = runs.get(&req.run_id).ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!("Run not found: {}", req.run_id),
            )
        })?;
        run.project_id.clone()
    };

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
        let sqlite_metrics: Vec<MetricRow> = req.metrics
            .iter()
            .filter(|m| accepted_metrics.contains(&m.name))
            .map(|m| MetricRow {
                name: m.name.clone(),
                step: m.step,
                value: m.value,
                timestamp: m.timestamp,
            })
            .collect();

        if let Err(e) = state.sqlite_store.insert_metrics(&req.run_id, &sqlite_metrics).await {
            warn!(error = %e, "Failed to persist metrics to SQLite");
        }

        // Also update metrics count in SQLite
        if let Err(e) = state.sqlite_store.increment_metrics_count(&req.run_id, accepted_metric_count as i64).await {
            warn!(error = %e, "Failed to update metrics count in SQLite");
        }

        // Also maintain in-memory for backward compatibility
        let mut metrics_store = state.store.metrics.write().await;
        let run_metrics = metrics_store
            .entry(req.run_id.clone())
            .or_insert_with(services::RunMetrics::new);

        for metric in req.metrics.iter().filter(|m| accepted_metrics.contains(&m.name)) {
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
        let tag_pairs: Vec<(String, String)> = accepted_tags.iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if let Err(e) = state.sqlite_store.set_tags(&req.run_id, &tag_pairs).await {
            warn!(error = %e, "Failed to persist tags to SQLite");
        }
    }

    // Persist params to SQLite
    if param_count > 0 {
        let param_pairs: Vec<(String, String)> = req.params
            .iter()
            .map(|p| (p.name.clone(), p.value.clone()))
            .collect();
        if let Err(e) = state.sqlite_store.insert_params(&req.run_id, &param_pairs).await {
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
    let run_project = state.sqlite_store.get_run_project_id(&run_id).await
        .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;
    auth.require_access(&run_project, "write")?;

    // Update in SQLite
    state.sqlite_store
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
    let run_project = state.sqlite_store.get_run_project_id(&run_id).await
        .map_err(|e| match e {
            storage::SqliteError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        })?;
    auth.require_access(&run_project, "admin")?;

    // Delete from SQLite (cascades to metrics, tags, params, batches)
    state.sqlite_store
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

/// Create a new API key (admin only).
async fn http_create_key(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(req): Json<CreateKeyRequest>,
) -> Result<Json<CreateKeyResponse>, (StatusCode, String)> {
    // Only admin can create keys
    auth.require_scope("admin")?;

    // Validate scopes
    let valid_scopes = ["admin", "write", "read"];
    for scope in &req.scopes {
        if !valid_scopes.contains(&scope.as_str()) {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("Invalid scope '{}'. Valid scopes: admin, write, read", scope),
            ));
        }
    }

    if req.scopes.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "At least one scope is required.".to_string(),
        ));
    }

    let (raw_key, key) = state
        .key_store
        .create_key(req.project_id.clone(), req.name.clone(), req.scopes.clone())
        .await;

    info!(
        key_prefix = %key.key_prefix,
        project_id = ?req.project_id,
        scopes = ?req.scopes,
        "Created new API key"
    );

    Ok(Json(CreateKeyResponse {
        api_key: raw_key,
        key_id: key.id,
        key_prefix: key.key_prefix,
        project_id: key.project_id,
        name: key.name,
        scopes: key.scopes,
    }))
}

/// List API keys (admin sees all, scoped users see their project's keys).
async fn http_list_keys(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
) -> Result<Json<ListKeysResponse>, (StatusCode, String)> {
    auth.require_scope("admin")?;

    let project_filter = auth.project_id();
    let keys = state.key_store.list_keys(project_filter).await;

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
    auth.require_scope("admin")?;

    // Find the key by id and get its hash for revocation
    let keys = state.key_store.list_keys(None).await;
    let target = keys.iter().find(|k| k.id == key_id);

    match target {
        Some(key) => {
            state.key_store.revoke_key(&key.key_hash).await;
            info!(key_id = %key_id, key_prefix = %key.key_prefix, "Revoked API key");
            Ok(Json(serde_json::json!({ "status": "ok", "revoked": key_id })))
        }
        None => Err((
            StatusCode::NOT_FOUND,
            format!("Key not found: {}", key_id),
        )),
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
    let run_project = state.sqlite_store.get_run_project_id(&run_id).await
        .map_err(|e| match e {
            storage::SqliteError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        })?;
    auth.require_access(&run_project, "read")?;

    // Generate a short, URL-safe token
    let token = generate_share_token();

    // Calculate expiry
    let expires_at = req.expires_in_days.map(|days| {
        let expires = chrono::Utc::now() + chrono::Duration::days(days);
        expires.format("%Y-%m-%d %H:%M:%S").to_string()
    });

    state.sqlite_store
        .create_share_token(
            &token,
            &run_id,
            Some(&auth.api_key.key_prefix),
            expires_at.as_deref(),
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    info!(run_id = %run_id, "Created share token");

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
    let share = state.sqlite_store
        .validate_share_token(&token)
        .await
        .map_err(|e| match e {
            storage::SqliteError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        })?;

    // Fetch the run
    let run = state.sqlite_store
        .get_run(&share.run_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let tags = state.sqlite_store
        .get_tags(&share.run_id)
        .await
        .unwrap_or_default()
        .into_iter()
        .collect::<std::collections::HashMap<String, String>>();

    let available_metrics = state.sqlite_store
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
    let share = state.sqlite_store
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
    let sqlite_series = state.sqlite_store
        .get_metrics(&share.run_id, &names, query.max_points)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let available_metrics = state.sqlite_store
        .get_metric_names(&share.run_id)
        .await
        .unwrap_or_default();

    let series: Vec<services::MetricSeries> = sqlite_series
        .into_iter()
        .map(|s| services::MetricSeries {
            name: s.name,
            points: s.points.into_iter().map(|p| services::AggregatedPoint {
                step: p.step,
                mean: p.mean,
                min: p.min,
                max: p.max,
                count: p.count,
            }).collect(),
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
    let run_project = state.sqlite_store.get_run_project_id(&run_id).await
        .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;
    auth.require_access(&run_project, "read")?;

    state.sqlite_store
        .revoke_share_token(&token)
        .await
        .map_err(|e| match e {
            storage::SqliteError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        })?;

    info!(run_id = %run_id, "Revoked share token");

    Ok(Json(serde_json::json!({ "status": "ok", "revoked": token })))
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
    // Require at least read scope
    auth.require_scope("read")?;

    let limit = query.limit.unwrap_or(100).min(1000);
    let offset = query.offset.unwrap_or(0);

    // Enforce project scope: scoped keys can only list runs in their project.
    // If the caller has a project_id, use it (overriding any query param).
    // Admin/dev keys can still filter by any project or see all.
    let effective_project = match auth.project_id() {
        Some(scoped_project) => {
            // If caller also passed a ?project= filter, verify it matches their scope
            if let Some(ref requested) = query.project {
                if requested != scoped_project {
                    return Err((
                        StatusCode::FORBIDDEN,
                        format!("Access denied: your key is scoped to project '{}', cannot query '{}'.", scoped_project, requested),
                    ));
                }
            }
            Some(scoped_project.to_string())
        }
        None => query.project.clone(), // Admin: use whatever was requested
    };

    // Query from SQLite
    let (sqlite_runs, total) = state.sqlite_store
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
        let tags = state.sqlite_store
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
    auth.require_access(&run.project_id, "read")?;

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
    let run_project = state.sqlite_store.get_run_project_id(&run_id).await
        .map_err(|_| (StatusCode::NOT_FOUND, format!("Run not found: {}", run_id)))?;
    auth.require_access(&run_project, "read")?;

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
    let sqlite_series = state.sqlite_store
        .get_metrics(&run_id, &names, query.max_points)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Get available metric names
    let available_metrics = state.sqlite_store
        .get_metric_names(&run_id)
        .await
        .unwrap_or_default();

    // Convert SQLite format to API format
    let series: Vec<services::MetricSeries> = sqlite_series
        .into_iter()
        .map(|s| services::MetricSeries {
            name: s.name,
            points: s.points.into_iter().map(|p| services::AggregatedPoint {
                step: p.step,
                mean: p.mean,
                min: p.min,
                max: p.max,
                count: p.count,
            }).collect(),
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
        auth.require_access(&run.project_id, "read")?;

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
    let cors = CorsLayer::permissive();
    let decompression = RequestDecompressionLayer::new();

    // Routes that require authentication
    let protected_routes = Router::new()
        // SDK HTTP transport endpoints (ingestion)
        .route("/api/v1/runs", post(http_init_run))
        .route("/api/v1/ingest/batch", post(http_ingest_batch))
        .route("/api/v1/runs/{run_id}/finish", post(http_finish_run))
        // Query API endpoints
        .route("/api/v1/runs", get(http_list_runs))
        .route("/api/v1/runs/{run_id}", get(http_get_run).delete(http_delete_run))
        .route("/api/v1/runs/{run_id}/metrics", get(http_get_metrics))
        .route("/api/v1/runs/compare", post(http_compare_runs))
        // Key management endpoints (admin only)
        .route("/api/v1/keys", post(http_create_key).get(http_list_keys))
        .route("/api/v1/keys/{key_id}", delete(http_revoke_key))
        // Share token management (requires auth)
        .route("/api/v1/runs/{run_id}/share", post(http_create_share_token))
        .route("/api/v1/runs/{run_id}/share/{token}", delete(http_revoke_share_token))
        .layer(middleware::from_fn_with_state(
            state.key_store.clone(),
            auth_middleware,
        ));

    // Public routes (no auth required)
    let public_routes = Router::new()
        .route("/", get(root))
        .route("/health", get(health))
        // Shared run endpoints (public, no auth — token is the credential)
        .route("/api/v1/shared/{token}", get(http_get_shared_run))
        .route("/api/v1/shared/{token}/metrics", get(http_get_shared_metrics));

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

    // Initialize API key store
    let key_store = Arc::new(ApiKeyStore::new());
    key_store.init_from_env().await;

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
    let sqlite_path = std::env::var("MLRUNX_SQLITE_PATH")
        .unwrap_or_else(|_| "mlrunx.db".to_string());
    let sqlite_store = Arc::new(
        SqliteStore::new(&sqlite_path)
            .await
            .expect("Failed to initialize SQLite store")
    );
    info!("SQLite store initialized at: {}", sqlite_path);

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
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    async fn test_app() -> Router {
        let store = Arc::new(InMemoryStore::new());
        // Use in-memory SQLite for tests
        let sqlite_store = Arc::new(
            SqliteStore::new(":memory:")
                .await
                .expect("Failed to create test SQLite store")
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
                .expect("Failed to create test SQLite store")
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
}
