use anyhow::{Context, Result};
use mlrunx_api_http_types::QueuedIngestBatch;
use redis::{FromRedisValue, streams::StreamReadReply};

#[derive(Debug, Clone)]
pub struct QueueConfig {
    pub redis_url: String,
    pub stream_key: String,
    pub consumer_group: String,
    pub consumer_name: String,
    pub read_count: usize,
    pub block_ms: usize,
    pub max_len_approx: usize,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            redis_url: "redis://127.0.0.1:6379".to_string(),
            stream_key: "mlrunx:ingest".to_string(),
            consumer_group: "mlrunx-processor".to_string(),
            consumer_name: format!("processor-{}", uuid::Uuid::now_v7()),
            read_count: 32,
            block_ms: 1_000,
            max_len_approx: 100_000,
        }
    }
}

impl QueueConfig {
    pub fn from_env() -> Self {
        let default = Self::default();
        Self {
            redis_url: std::env::var("REDIS_URL").unwrap_or(default.redis_url),
            stream_key: std::env::var("MLRUNX_INGEST_STREAM_KEY").unwrap_or(default.stream_key),
            consumer_group: std::env::var("MLRUNX_INGEST_STREAM_GROUP")
                .unwrap_or(default.consumer_group),
            consumer_name: std::env::var("MLRUNX_INGEST_STREAM_CONSUMER")
                .unwrap_or(default.consumer_name),
            read_count: std::env::var("MLRUNX_INGEST_STREAM_READ_COUNT")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(default.read_count),
            block_ms: std::env::var("MLRUNX_INGEST_STREAM_BLOCK_MS")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(default.block_ms),
            max_len_approx: std::env::var("MLRUNX_INGEST_STREAM_MAXLEN")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(default.max_len_approx),
        }
    }
}

#[derive(Debug, Clone)]
pub struct QueuedStreamEntry {
    pub redis_id: String,
    pub batch: QueuedIngestBatch,
}

#[derive(Clone)]
pub struct RedisIngestQueue {
    client: redis::Client,
    config: QueueConfig,
}

impl RedisIngestQueue {
    pub fn from_env() -> Result<Self> {
        Self::new(QueueConfig::from_env())
    }

    pub fn new(config: QueueConfig) -> Result<Self> {
        let client = redis::Client::open(config.redis_url.clone())
            .with_context(|| format!("invalid Redis URL '{}'", config.redis_url))?;
        Ok(Self { client, config })
    }

    pub const fn config(&self) -> &QueueConfig {
        &self.config
    }

    pub async fn enqueue(&self, batch: &QueuedIngestBatch) -> Result<String> {
        let payload = serde_json::to_string(batch).context("failed to serialize queued batch")?;
        let mut connection = self.connection().await?;
        let stream_id = redis::cmd("XADD")
            .arg(&self.config.stream_key)
            .arg("MAXLEN")
            .arg("~")
            .arg(i64::try_from(self.config.max_len_approx).unwrap_or(i64::MAX))
            .arg("*")
            .arg("payload")
            .arg(payload)
            .query_async(&mut connection)
            .await
            .context("failed to enqueue ingest batch")?;
        Ok(stream_id)
    }

    pub async fn ensure_consumer_group(&self) -> Result<()> {
        let mut connection = self.connection().await?;
        let result: Result<(), redis::RedisError> = redis::cmd("XGROUP")
            .arg("CREATE")
            .arg(&self.config.stream_key)
            .arg(&self.config.consumer_group)
            .arg("0")
            .arg("MKSTREAM")
            .query_async(&mut connection)
            .await;

        if let Err(error) = result {
            let busy_group = error.to_string().contains("BUSYGROUP");
            if !busy_group {
                return Err(error).context("failed to create Redis consumer group");
            }
        }

        Ok(())
    }

    pub async fn read_next(&self) -> Result<Vec<QueuedStreamEntry>> {
        let mut connection = self.connection().await?;
        let reply: StreamReadReply = redis::cmd("XREADGROUP")
            .arg("GROUP")
            .arg(&self.config.consumer_group)
            .arg(&self.config.consumer_name)
            .arg("COUNT")
            .arg(i64::try_from(self.config.read_count).unwrap_or(i64::MAX))
            .arg("BLOCK")
            .arg(i64::try_from(self.config.block_ms).unwrap_or(i64::MAX))
            .arg("STREAMS")
            .arg(&self.config.stream_key)
            .arg(">")
            .query_async(&mut connection)
            .await
            .context("failed to read ingest batches from Redis")?;

        let mut entries = Vec::new();
        for stream in reply.keys {
            for stream_id in stream.ids {
                let payload_value = stream_id
                    .map
                    .get("payload")
                    .context("Redis stream entry missing payload field")?;
                let payload = String::from_redis_value(payload_value)
                    .context("failed to decode Redis stream payload")?;
                let batch: QueuedIngestBatch =
                    serde_json::from_str(&payload).context("failed to decode queued batch")?;
                entries.push(QueuedStreamEntry {
                    redis_id: stream_id.id,
                    batch,
                });
            }
        }

        Ok(entries)
    }

    pub async fn ack(&self, redis_id: &str) -> Result<()> {
        let mut connection = self.connection().await?;
        let _: i64 = redis::cmd("XACK")
            .arg(&self.config.stream_key)
            .arg(&self.config.consumer_group)
            .arg(redis_id)
            .query_async(&mut connection)
            .await
            .context("failed to acknowledge Redis stream entry")?;
        Ok(())
    }

    async fn connection(&self) -> Result<redis::aio::MultiplexedConnection> {
        self.client
            .get_multiplexed_tokio_connection()
            .await
            .context("failed to connect to Redis")
    }
}
