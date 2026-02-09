//! Ingest Service Implementation
//!
//! Handles high-throughput data ingestion from ML training SDKs.
//! See: /docs/spec/ingest.md for full semantics.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

use prost_types::Timestamp;
use tokio::sync::RwLock;
use tonic::{Request, Response, Status};
use tracing::{debug, info, instrument, warn};
use uuid::Uuid;

use crate::storage::{CreateParameterInput, ParameterRepository, ParameterValue};
use mlrunx_proto::mlrunx::v1::{
    CreateArtifactUploadRequest, CreateArtifactUploadResponse, FinalizeArtifactUploadRequest,
    FinalizeArtifactUploadResponse, FinishRunRequest, FinishRunResponse, HeartbeatRequest,
    HeartbeatResponse, InitRunRequest, InitRunResponse, LogMetricsRequest, LogMetricsResponse,
    LogMetricsStreamRequest, LogMetricsStreamResponse, LogParamsRequest, LogParamsResponse,
    LogTagsRequest, LogTagsResponse, RunId, RunStatus, ingest_service_server::IngestService,
};

/// In-memory run state for alpha (will be replaced by PostgreSQL in STO-002).
#[derive(Debug, Clone)]
pub struct RunState {
    pub run_id: String,
    pub project_id: String,
    pub name: Option<String>,
    pub status: RunStatus,
    pub created_at: SystemTime,
    pub updated_at: SystemTime,
    pub metrics_count: u64,
    pub params_count: u64,
    pub tags: HashMap<String, String>,
}

/// In-memory storage for runs (temporary until STO-001/002).
#[derive(Debug, Default)]
pub struct InMemoryStore {
    pub runs: RwLock<HashMap<String, RunState>>,
    /// Track seen batch IDs for idempotency
    pub seen_batches: RwLock<HashMap<String, ()>>,
    /// Metric data storage per run
    pub metrics: RwLock<HashMap<String, super::metrics::RunMetrics>>,
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Implementation of the IngestService gRPC service.
pub struct IngestServiceImpl {
    store: Arc<InMemoryStore>,
}

impl IngestServiceImpl {
    pub fn new(store: Arc<InMemoryStore>) -> Self {
        Self { store }
    }
}

fn now_timestamp() -> Option<Timestamp> {
    let now = SystemTime::now();
    let duration = now.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    Some(Timestamp {
        seconds: duration.as_secs() as i64,
        nanos: duration.subsec_nanos() as i32,
    })
}

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes"))
}

fn warning_detail(
    code: impl Into<String>,
    message: impl Into<String>,
    field: impl Into<String>,
) -> mlrunx_proto::mlrunx::v1::ErrorDetail {
    mlrunx_proto::mlrunx::v1::ErrorDetail {
        code: code.into(),
        message: message.into(),
        field: field.into(),
        severity: 1, // ERROR_SEVERITY_WARNING
    }
}

fn build_postgres_parameter_inputs(
    run_id: &str,
    params: &[mlrunx_proto::mlrunx::v1::Parameter],
) -> Result<Vec<CreateParameterInput>, String> {
    let run_uuid = Uuid::parse_str(run_id).map_err(|_| {
        format!("run_id '{run_id}' is not a UUID; skipping PostgreSQL parameter shadow write")
    })?;

    Ok(params
        .iter()
        .map(|param| CreateParameterInput {
            run_id: run_uuid,
            name: param.name.clone(),
            value: ParameterValue::String(param.value.clone()),
        })
        .collect())
}

#[tonic::async_trait]
impl IngestService for IngestServiceImpl {
    /// Initialize a new run or return existing if idempotent.
    #[instrument(skip(self, request), fields(project_id, run_id))]
    async fn init_run(
        &self,
        request: Request<InitRunRequest>,
    ) -> Result<Response<InitRunResponse>, Status> {
        let req = request.into_inner();

        // Validate required fields
        let project_id = req
            .project_id
            .ok_or_else(|| Status::invalid_argument("project_id is required"))?;

        tracing::Span::current().record("project_id", &project_id.value);

        // Generate or use provided run_id
        let run_id = req.run_id.unwrap_or_else(|| Uuid::now_v7().to_string());
        tracing::Span::current().record("run_id", &run_id);

        let mut runs = self.store.runs.write().await;

        // Check if run already exists (idempotent)
        if let Some(existing) = runs.get(&run_id) {
            info!(run_id = %run_id, "Returning existing run (idempotent)");
            return Ok(Response::new(InitRunResponse {
                run_id: Some(RunId {
                    value: existing.run_id.clone(),
                }),
                resume_token: format!("resume-{}", run_id),
                server_time: now_timestamp(),
                resumed: true,
                warnings: vec![],
            }));
        }

        // Create new run
        let now = SystemTime::now();
        let run_state = RunState {
            run_id: run_id.clone(),
            project_id: project_id.value.clone(),
            name: req.name.clone(),
            status: RunStatus::Running,
            created_at: now,
            updated_at: now,
            metrics_count: 0,
            params_count: 0,
            tags: req
                .tags
                .iter()
                .map(|t| (t.key.clone(), t.value.clone()))
                .collect(),
        };

        runs.insert(run_id.clone(), run_state);
        info!(
            run_id = %run_id,
            project = %project_id.value,
            name = ?req.name,
            "Initialized new run"
        );

        Ok(Response::new(InitRunResponse {
            run_id: Some(RunId {
                value: run_id.clone(),
            }),
            resume_token: format!("resume-{}", run_id),
            server_time: now_timestamp(),
            resumed: false,
            warnings: vec![],
        }))
    }

    /// Log a batch of metrics.
    #[instrument(skip(self, request), fields(run_id, batch_id, point_count))]
    async fn log_metrics(
        &self,
        request: Request<LogMetricsRequest>,
    ) -> Result<Response<LogMetricsResponse>, Status> {
        let req = request.into_inner();

        let run_id = req
            .run_id
            .ok_or_else(|| Status::invalid_argument("run_id is required"))?;
        tracing::Span::current().record("run_id", &run_id.value);
        tracing::Span::current().record("batch_id", &req.batch_id);

        // Check batch idempotency
        {
            let mut seen = self.store.seen_batches.write().await;
            let batch_key = format!("{}:{}", run_id.value, req.batch_id);
            if seen.contains_key(&batch_key) {
                debug!(batch_id = %req.batch_id, "Batch already processed (idempotent)");
                return Ok(Response::new(LogMetricsResponse {
                    accepted_count: 0,
                    deduplicated_count: req
                        .metrics
                        .as_ref()
                        .map(|m| m.points.len() as i64)
                        .unwrap_or(0),
                    warnings: vec![],
                    server_time: now_timestamp(),
                }));
            }
            seen.insert(batch_key, ());
        }

        // Verify run exists
        let point_count = req.metrics.as_ref().map(|m| m.points.len()).unwrap_or(0);
        tracing::Span::current().record("point_count", point_count);

        {
            let mut runs = self.store.runs.write().await;
            let run = runs
                .get_mut(&run_id.value)
                .ok_or_else(|| Status::not_found(format!("Run not found: {}", run_id.value)))?;

            if run.status != RunStatus::Running {
                return Err(Status::failed_precondition(format!(
                    "Run {} is not running (status: {:?})",
                    run_id.value, run.status
                )));
            }

            run.metrics_count += point_count as u64;
            run.updated_at = SystemTime::now();
        }

        // Store actual metric points for querying
        if let Some(batch) = &req.metrics {
            let mut metrics_store = self.store.metrics.write().await;
            let run_metrics = metrics_store
                .entry(run_id.value.clone())
                .or_insert_with(super::metrics::RunMetrics::new);

            for point in &batch.points {
                run_metrics.add_point(super::metrics::MetricPoint {
                    name: point.name.clone(),
                    step: point.step,
                    value: point.value,
                    timestamp: point
                        .timestamp
                        .as_ref()
                        .map(|t| t.seconds as f64 + t.nanos as f64 / 1e9),
                });
            }
        }

        debug!(
            run_id = %run_id.value,
            batch_id = %req.batch_id,
            points = point_count,
            "Logged metrics batch"
        );

        Ok(Response::new(LogMetricsResponse {
            accepted_count: point_count as i64,
            deduplicated_count: 0,
            warnings: vec![],
            server_time: now_timestamp(),
        }))
    }

    /// Log metrics as a bidirectional stream.
    async fn log_metrics_stream(
        &self,
        _request: Request<tonic::Streaming<LogMetricsStreamRequest>>,
    ) -> Result<Response<Self::LogMetricsStreamStream>, Status> {
        // Streaming will be implemented in a future iteration
        Err(Status::unimplemented("Streaming not yet implemented"))
    }

    type LogMetricsStreamStream = std::pin::Pin<
        Box<dyn futures::Stream<Item = Result<LogMetricsStreamResponse, Status>> + Send>,
    >;

    /// Log parameters.
    #[instrument(skip(self, request), fields(run_id, param_count))]
    async fn log_params(
        &self,
        request: Request<LogParamsRequest>,
    ) -> Result<Response<LogParamsResponse>, Status> {
        let req = request.into_inner();

        let run_id = req
            .run_id
            .ok_or_else(|| Status::invalid_argument("run_id is required"))?;
        tracing::Span::current().record("run_id", &run_id.value);

        let param_count = req.params.len();
        tracing::Span::current().record("param_count", param_count);
        let mut warnings = Vec::new();

        {
            let mut runs = self.store.runs.write().await;
            let run = runs
                .get_mut(&run_id.value)
                .ok_or_else(|| Status::not_found(format!("Run not found: {}", run_id.value)))?;

            run.params_count += param_count as u64;
            run.updated_at = SystemTime::now();
        }

        // STO-002 start: optional PostgreSQL shadow write for parameter durability.
        // Current safety posture: in-memory remains the primary path; DB write is best-effort.
        if env_flag("MLRUNX_POSTGRES_SHADOW_WRITES_ENABLED") {
            match build_postgres_parameter_inputs(&run_id.value, &req.params) {
                Ok(inputs) => match ParameterRepository::upsert_batch(inputs).await {
                    Ok(written) => {
                        debug!(
                            run_id = %run_id.value,
                            params = param_count,
                            postgres_upserted = written,
                            "Shadow wrote parameters to PostgreSQL"
                        );
                    }
                    Err(err) => {
                        warn!(
                            run_id = %run_id.value,
                            error = %err,
                            "PostgreSQL parameter shadow write failed"
                        );
                        warnings.push(warning_detail(
                            "POSTGRES_SHADOW_WRITE_FAILED",
                            "PostgreSQL parameter shadow write failed; in-memory ingest path remains active.",
                            "params",
                        ));
                    }
                },
                Err(message) => {
                    warn!(run_id = %run_id.value, "{message}");
                    warnings.push(warning_detail(
                        "POSTGRES_SHADOW_WRITE_SKIPPED",
                        message,
                        "run_id",
                    ));
                }
            }
        }

        debug!(
            run_id = %run_id.value,
            params = param_count,
            "Logged parameters"
        );

        Ok(Response::new(LogParamsResponse {
            accepted_count: param_count as i64,
            existing_count: 0,
            warnings,
        }))
    }

    /// Log or update tags.
    #[instrument(skip(self, request), fields(run_id))]
    async fn log_tags(
        &self,
        request: Request<LogTagsRequest>,
    ) -> Result<Response<LogTagsResponse>, Status> {
        let req = request.into_inner();

        let run_id = req
            .run_id
            .ok_or_else(|| Status::invalid_argument("run_id is required"))?;
        tracing::Span::current().record("run_id", &run_id.value);

        let mut updated = 0i64;
        let mut removed = 0i64;

        {
            let mut runs = self.store.runs.write().await;
            let run = runs
                .get_mut(&run_id.value)
                .ok_or_else(|| Status::not_found(format!("Run not found: {}", run_id.value)))?;

            // Update/add tags
            for tag in &req.tags {
                run.tags.insert(tag.key.clone(), tag.value.clone());
                updated += 1;
            }

            // Remove tags
            for key in &req.remove_keys {
                if run.tags.remove(key).is_some() {
                    removed += 1;
                }
            }

            run.updated_at = SystemTime::now();
        }

        debug!(
            run_id = %run_id.value,
            updated = updated,
            removed = removed,
            "Updated tags"
        );

        Ok(Response::new(LogTagsResponse {
            updated_count: updated,
            removed_count: removed,
            warnings: vec![],
        }))
    }

    /// Create artifact upload (get presigned URL).
    async fn create_artifact_upload(
        &self,
        _request: Request<CreateArtifactUploadRequest>,
    ) -> Result<Response<CreateArtifactUploadResponse>, Status> {
        // Artifact upload will be implemented in STO-003
        Err(Status::unimplemented("Artifact upload not yet implemented"))
    }

    /// Finalize artifact upload.
    async fn finalize_artifact_upload(
        &self,
        _request: Request<FinalizeArtifactUploadRequest>,
    ) -> Result<Response<FinalizeArtifactUploadResponse>, Status> {
        // Artifact upload will be implemented in STO-003
        Err(Status::unimplemented("Artifact upload not yet implemented"))
    }

    /// Heartbeat to indicate run is alive.
    #[instrument(skip(self, request), fields(run_id))]
    async fn heartbeat(
        &self,
        request: Request<HeartbeatRequest>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
        let req = request.into_inner();

        let run_id = req
            .run_id
            .ok_or_else(|| Status::invalid_argument("run_id is required"))?;
        tracing::Span::current().record("run_id", &run_id.value);

        {
            let mut runs = self.store.runs.write().await;
            let run = runs
                .get_mut(&run_id.value)
                .ok_or_else(|| Status::not_found(format!("Run not found: {}", run_id.value)))?;

            run.updated_at = SystemTime::now();
        }

        debug!(run_id = %run_id.value, "Heartbeat received");

        Ok(Response::new(HeartbeatResponse {
            server_time: now_timestamp(),
            request_resync: false,
        }))
    }

    /// Finish a run.
    #[instrument(skip(self, request), fields(run_id, status))]
    async fn finish_run(
        &self,
        request: Request<FinishRunRequest>,
    ) -> Result<Response<FinishRunResponse>, Status> {
        let req = request.into_inner();
        let status = req.status();
        tracing::Span::current().record("status", format!("{:?}", status));

        let run_id = req
            .run_id
            .ok_or_else(|| Status::invalid_argument("run_id is required"))?;
        tracing::Span::current().record("run_id", &run_id.value);

        let (duration, metrics_count) = {
            let mut runs = self.store.runs.write().await;
            let run = runs
                .get_mut(&run_id.value)
                .ok_or_else(|| Status::not_found(format!("Run not found: {}", run_id.value)))?;

            run.status = status;
            run.updated_at = SystemTime::now();

            let duration = run
                .updated_at
                .duration_since(run.created_at)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0);

            (duration, run.metrics_count)
        };

        info!(
            run_id = %run_id.value,
            status = ?status,
            duration_s = duration,
            metrics = metrics_count,
            "Run finished"
        );

        Ok(Response::new(FinishRunResponse {
            duration_seconds: duration,
            total_metrics: metrics_count as i64,
            total_artifacts: 0,
            finished_at: now_timestamp(),
            warnings: vec![],
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_init_run() {
        let store = Arc::new(InMemoryStore::new());
        let service = IngestServiceImpl::new(store);

        let request = Request::new(InitRunRequest {
            project_id: Some(mlrunx_proto::mlrunx::v1::ProjectId {
                value: "test-project".to_string(),
            }),
            run_id: Some("test-run-123".to_string()),
            name: Some("My Test Run".to_string()),
            ..Default::default()
        });

        let response = service.init_run(request).await.unwrap();
        let resp = response.into_inner();

        assert_eq!(resp.run_id.unwrap().value, "test-run-123");
        assert!(!resp.resumed);
    }

    #[tokio::test]
    async fn test_init_run_idempotent() {
        let store = Arc::new(InMemoryStore::new());
        let service = IngestServiceImpl::new(store);

        let make_request = || {
            Request::new(InitRunRequest {
                project_id: Some(mlrunx_proto::mlrunx::v1::ProjectId {
                    value: "test-project".to_string(),
                }),
                run_id: Some("test-run-123".to_string()),
                name: Some("My Test Run".to_string()),
                ..Default::default()
            })
        };

        // First call creates the run
        let resp1 = service.init_run(make_request()).await.unwrap().into_inner();
        assert!(!resp1.resumed);

        // Second call returns existing (idempotent)
        let resp2 = service.init_run(make_request()).await.unwrap().into_inner();
        assert!(resp2.resumed);
    }

    #[tokio::test]
    async fn test_log_metrics() {
        let store = Arc::new(InMemoryStore::new());
        let service = IngestServiceImpl::new(store.clone());

        // First create a run
        let init_request = Request::new(InitRunRequest {
            project_id: Some(mlrunx_proto::mlrunx::v1::ProjectId {
                value: "test-project".to_string(),
            }),
            run_id: Some("test-run".to_string()),
            ..Default::default()
        });
        service.init_run(init_request).await.unwrap();

        // Log some metrics
        let metrics_request = Request::new(LogMetricsRequest {
            run_id: Some(RunId {
                value: "test-run".to_string(),
            }),
            batch_id: "batch-1".to_string(),
            metrics: Some(mlrunx_proto::mlrunx::v1::MetricBatch {
                points: vec![
                    mlrunx_proto::mlrunx::v1::MetricPoint {
                        name: "loss".to_string(),
                        step: 0,
                        value: 0.5,
                        timestamp: None,
                    },
                    mlrunx_proto::mlrunx::v1::MetricPoint {
                        name: "accuracy".to_string(),
                        step: 0,
                        value: 0.8,
                        timestamp: None,
                    },
                ],
            }),
            sequence: None,
        });

        let response = service.log_metrics(metrics_request).await.unwrap();
        let resp = response.into_inner();

        assert_eq!(resp.accepted_count, 2);
    }

    #[tokio::test]
    async fn test_finish_run() {
        let store = Arc::new(InMemoryStore::new());
        let service = IngestServiceImpl::new(store);

        // Create a run
        let init_request = Request::new(InitRunRequest {
            project_id: Some(mlrunx_proto::mlrunx::v1::ProjectId {
                value: "test-project".to_string(),
            }),
            run_id: Some("test-run".to_string()),
            ..Default::default()
        });
        service.init_run(init_request).await.unwrap();

        // Finish the run
        let finish_request = Request::new(FinishRunRequest {
            run_id: Some(RunId {
                value: "test-run".to_string(),
            }),
            status: RunStatus::Finished.into(),
            exit_code: None,
            error_message: None,
            summary: vec![],
        });

        let response = service.finish_run(finish_request).await.unwrap();
        let resp = response.into_inner();

        assert!(resp.duration_seconds >= 0.0);
    }

    #[test]
    fn test_build_postgres_parameter_inputs_with_uuid() {
        let run_id = Uuid::now_v7().to_string();
        let params = vec![
            mlrunx_proto::mlrunx::v1::Parameter {
                name: "lr".to_string(),
                value: "0.001".to_string(),
            },
            mlrunx_proto::mlrunx::v1::Parameter {
                name: "optimizer".to_string(),
                value: "adamw".to_string(),
            },
        ];

        let result = build_postgres_parameter_inputs(&run_id, &params).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "lr");
    }

    #[test]
    fn test_build_postgres_parameter_inputs_with_non_uuid_run_id() {
        let params = vec![mlrunx_proto::mlrunx::v1::Parameter {
            name: "lr".to_string(),
            value: "0.001".to_string(),
        }];

        let err = build_postgres_parameter_inputs("run-non-uuid-id", &params).unwrap_err();
        assert!(err.contains("not a UUID"));
    }
}
