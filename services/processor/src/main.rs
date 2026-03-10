use std::{sync::Arc, time::Duration};

use anyhow::Result;
use mlrunx_api::{
    observability,
    queue::{QueuedStreamEntry, RedisIngestQueue},
    storage::{
        CreateParameterInput, MetricRow, ParameterRepository, ParameterValue, RunEventInput,
        RunRepository, SqliteStore,
        clickhouse::{ClickHouseClient, ClickHouseConfig, MetricPoint, MetricsRepository},
    },
};
use tracing::{info, warn};

#[derive(Clone)]
struct ProcessorState {
    sqlite_store: Arc<SqliteStore>,
    queue: Arc<RedisIngestQueue>,
    metrics_repo: Arc<MetricsRepository>,
}

fn offset_datetime_from_unix_seconds(timestamp: Option<f64>) -> time::OffsetDateTime {
    let Some(seconds) = timestamp else {
        return time::OffsetDateTime::now_utc();
    };
    if !seconds.is_finite() {
        return time::OffsetDateTime::now_utc();
    }

    let nanos = (seconds * 1_000_000_000.0).round();
    if !(i128::MIN as f64..=i128::MAX as f64).contains(&nanos) {
        return time::OffsetDateTime::now_utc();
    }

    time::OffsetDateTime::from_unix_timestamp_nanos(nanos as i128)
        .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
}

async fn sync_postgres_metadata(batch: &mlrunx_api_http_types::QueuedIngestBatch) {
    if let Ok(run_uuid) = uuid::Uuid::parse_str(&batch.run_id) {
        if !batch.params.is_empty() {
            let inputs = batch
                .params
                .iter()
                .map(|param| CreateParameterInput {
                    run_id: run_uuid,
                    name: param.name.clone(),
                    value: ParameterValue::String(param.value.clone()),
                })
                .collect();
            if let Err(error) = ParameterRepository::upsert_batch(inputs).await {
                warn!(run_id = %batch.run_id, error = %error, "PostgreSQL parameter sync failed");
            }
        }

        if !batch.tags.is_empty() {
            match RunRepository::get_by_id(run_uuid).await {
                Ok(run) => {
                    let mut merged_tags = run.tags.as_object().cloned().unwrap_or_default();
                    for tag in &batch.tags {
                        merged_tags.insert(
                            tag.key.clone(),
                            serde_json::Value::String(tag.value.clone()),
                        );
                    }
                    if let Err(error) =
                        RunRepository::update_tags(run_uuid, serde_json::Value::Object(merged_tags))
                            .await
                    {
                        warn!(run_id = %batch.run_id, error = %error, "PostgreSQL tag sync failed");
                    }
                }
                Err(error) => {
                    warn!(run_id = %batch.run_id, error = %error, "PostgreSQL run lookup failed");
                }
            }
        }
    }
}

async fn process_entry(state: &ProcessorState, entry: QueuedStreamEntry) -> Result<()> {
    if state
        .sqlite_store
        .batch_exists(&entry.batch.batch_id)
        .await
        .unwrap_or(false)
    {
        state.queue.ack(&entry.redis_id).await?;
        return Ok(());
    }

    let sqlite_metrics: Vec<MetricRow> = entry
        .batch
        .metrics
        .iter()
        .map(|metric| MetricRow {
            name: metric.name.clone(),
            step: metric.step,
            value: metric.value,
            timestamp: metric.timestamp,
        })
        .collect();
    if !sqlite_metrics.is_empty() {
        state
            .sqlite_store
            .insert_metrics(&entry.batch.run_id, &sqlite_metrics)
            .await?;
        state
            .sqlite_store
            .increment_metrics_count(
                &entry.batch.run_id,
                i64::try_from(sqlite_metrics.len()).unwrap_or(i64::MAX),
            )
            .await?;
    }

    let clickhouse_points: Vec<MetricPoint> = entry
        .batch
        .metrics
        .iter()
        .map(|metric| MetricPoint {
            run_id: entry.batch.run_id.clone(),
            project_id: entry.batch.project_id.clone(),
            name: metric.name.clone(),
            step: metric.step,
            value: metric.value,
            timestamp: offset_datetime_from_unix_seconds(metric.timestamp),
            batch_id: entry.batch.batch_id.clone(),
        })
        .collect();
    if !clickhouse_points.is_empty() {
        state.metrics_repo.insert_batch(&clickhouse_points).await?;
    }

    if !entry.batch.tags.is_empty() {
        let tag_pairs: Vec<(String, String)> = entry
            .batch
            .tags
            .iter()
            .map(|tag| (tag.key.clone(), tag.value.clone()))
            .collect();
        state
            .sqlite_store
            .set_tags(&entry.batch.run_id, &tag_pairs)
            .await?;
    }

    if !entry.batch.params.is_empty() {
        let param_pairs: Vec<(String, String)> = entry
            .batch
            .params
            .iter()
            .map(|param| (param.name.clone(), param.value.clone()))
            .collect();
        state
            .sqlite_store
            .insert_params(&entry.batch.run_id, &param_pairs)
            .await?;
    }

    let mut events: Vec<RunEventInput> = entry
        .batch
        .events
        .iter()
        .map(|event| RunEventInput {
            level: event.level.clone(),
            source: event.source.clone(),
            message: event.message.clone(),
            step: event.step,
            timestamp: event.timestamp,
        })
        .collect();
    events.extend(entry.batch.warnings.iter().map(|warning| RunEventInput {
        level: "warn".to_string(),
        source: "ingest".to_string(),
        message: warning.clone(),
        step: None,
        timestamp: None,
    }));
    if !events.is_empty() {
        state
            .sqlite_store
            .insert_run_events(&entry.batch.run_id, &events)
            .await?;
    }

    state
        .sqlite_store
        .record_batch(
            &entry.batch.batch_id,
            &entry.batch.run_id,
            entry.batch.seq,
            &entry.batch.payload_hash,
        )
        .await?;
    sync_postgres_metadata(&entry.batch).await;
    state.queue.ack(&entry.redis_id).await?;

    let queue_lag_ms = chrono::Utc::now().timestamp_millis() - entry.batch.queued_at_unix_ms;
    info!(
        run_id = %entry.batch.run_id,
        batch_id = %entry.batch.batch_id,
        queue_lag_ms,
        metrics = entry.batch.metrics.len(),
        params = entry.batch.params.len(),
        tags = entry.batch.tags.len(),
        events = entry.batch.events.len(),
        warnings = entry.batch.warnings.len(),
        "Processed queued ingest batch"
    );

    Ok(())
}

#[tokio::main]
async fn main() {
    let default_log = std::env::var("RUST_LOG")
        .unwrap_or_else(|_| "info,mlrunx_processor=debug,mlrunx_api=debug".to_string());
    observability::init_tracing(&default_log, "mlrunx-processor")
        .expect("Failed to initialize tracing");

    let sqlite_path =
        std::env::var("MLRUNX_SQLITE_PATH").unwrap_or_else(|_| "mlrunx.db".to_string());
    let sqlite_store = Arc::new(
        SqliteStore::new(&sqlite_path)
            .await
            .expect("Failed to initialize SQLite store"),
    );
    let queue = Arc::new(RedisIngestQueue::from_env().expect("Failed to initialize Redis queue"));
    queue
        .ensure_consumer_group()
        .await
        .expect("Failed to initialize Redis consumer group");

    let clickhouse_client = ClickHouseClient::new(&ClickHouseConfig::from_env());
    let metrics_repo = Arc::new(MetricsRepository::new(clickhouse_client));

    let state = ProcessorState {
        sqlite_store,
        queue,
        metrics_repo,
    };

    info!(
        stream_key = %state.queue.config().stream_key,
        consumer_group = %state.queue.config().consumer_group,
        consumer_name = %state.queue.config().consumer_name,
        "Starting MLRunX processor"
    );

    loop {
        match state.queue.read_next().await {
            Ok(entries) if entries.is_empty() => {}
            Ok(entries) => {
                for entry in entries {
                    if let Err(error) = process_entry(&state, entry).await {
                        warn!(error = %error, "Failed to process queued batch");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
            Err(error) => {
                warn!(error = %error, "Redis stream read failed");
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}
