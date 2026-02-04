//! ClickHouse storage implementation for metrics.
//!
//! Provides high-throughput metric storage using ClickHouse.
//! See: /migrations/clickhouse/001_metrics_schema.sql for schema.

use clickhouse::{Client, Row};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info, instrument};

/// Errors that can occur in ClickHouse operations.
#[derive(Error, Debug)]
pub enum ClickHouseError {
    #[error("ClickHouse client error: {0}")]
    Client(#[from] clickhouse::error::Error),

    #[error("Configuration error: {0}")]
    Config(String),
}

/// Configuration for ClickHouse connection.
#[derive(Debug, Clone)]
pub struct ClickHouseConfig {
    /// ClickHouse HTTP URL (e.g., "http://localhost:8123")
    pub url: String,
    /// Database name
    pub database: String,
    /// Username for authentication
    pub user: String,
    /// Password for authentication
    pub password: String,
}

impl Default for ClickHouseConfig {
    fn default() -> Self {
        Self {
            url: "http://localhost:8123".to_string(),
            database: "mlrunx".to_string(),
            user: "mlrunx".to_string(),
            password: "mlrunx_dev".to_string(),
        }
    }
}

impl ClickHouseConfig {
    /// Create config from environment variables.
    pub fn from_env() -> Self {
        Self {
            url: std::env::var("CLICKHOUSE_URL")
                .unwrap_or_else(|_| "http://localhost:8123".to_string()),
            database: std::env::var("CLICKHOUSE_DATABASE").unwrap_or_else(|_| "mlrunx".to_string()),
            user: std::env::var("CLICKHOUSE_USER").unwrap_or_else(|_| "mlrunx".to_string()),
            password: std::env::var("CLICKHOUSE_PASSWORD")
                .unwrap_or_else(|_| "mlrunx_dev".to_string()),
        }
    }
}

/// ClickHouse client wrapper with connection pooling.
#[derive(Clone)]
pub struct ClickHouseClient {
    client: Client,
    database: String,
}

impl ClickHouseClient {
    /// Create a new ClickHouse client.
    pub fn new(config: &ClickHouseConfig) -> Self {
        let client = Client::default()
            .with_url(&config.url)
            .with_database(&config.database)
            .with_user(&config.user)
            .with_password(&config.password);

        Self {
            client,
            database: config.database.clone(),
        }
    }

    /// Get the underlying client for custom queries.
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Check connection health.
    pub async fn health_check(&self) -> Result<(), ClickHouseError> {
        self.client.query("SELECT 1").execute().await?;
        Ok(())
    }

    /// Run migrations to create tables.
    #[instrument(skip(self))]
    pub async fn run_migrations(&self) -> Result<(), ClickHouseError> {
        info!("Running ClickHouse migrations");

        // Create database if not exists
        self.client
            .query(&format!("CREATE DATABASE IF NOT EXISTS {}", self.database))
            .execute()
            .await?;

        // Create metrics table
        self.client
            .query(include_str!(
                "../../../../migrations/clickhouse/001_metrics_schema.sql"
            ))
            .execute()
            .await
            .map_err(|e| {
                // Handle case where objects already exist
                if e.to_string().contains("already exists") {
                    info!("Tables already exist, skipping migration");
                    return ClickHouseError::Config("Tables already exist".to_string());
                }
                ClickHouseError::Client(e)
            })
            .or_else(|e| {
                if matches!(e, ClickHouseError::Config(_)) {
                    Ok(())
                } else {
                    Err(e)
                }
            })?;

        info!("ClickHouse migrations complete");
        Ok(())
    }
}

/// A single metric point for storage.
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct MetricPoint {
    pub run_id: String,
    pub project_id: String,
    pub name: String,
    pub step: i64,
    pub value: f64,
    #[serde(with = "clickhouse::serde::time::datetime64::millis")]
    pub timestamp: time::OffsetDateTime,
    pub batch_id: String,
}

/// A metric point for queries (with ingested_at).
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct MetricPointFull {
    pub run_id: String,
    pub project_id: String,
    pub name: String,
    pub step: i64,
    pub value: f64,
    #[serde(with = "clickhouse::serde::time::datetime64::millis")]
    pub timestamp: time::OffsetDateTime,
    #[serde(with = "clickhouse::serde::time::datetime64::millis")]
    pub ingested_at: time::OffsetDateTime,
    pub batch_id: String,
}

/// Summary statistics for a metric.
#[derive(Debug, Clone, Serialize, Deserialize, Row)]
pub struct MetricSummary {
    pub run_id: String,
    pub project_id: String,
    pub name: String,
    pub min_value: f64,
    pub max_value: f64,
    pub last_value: f64,
    pub last_step: i64,
    pub count: u64,
}

/// Repository for metrics storage operations.
#[derive(Clone)]
pub struct MetricsRepository {
    client: ClickHouseClient,
}

impl MetricsRepository {
    /// Create a new metrics repository.
    pub fn new(client: ClickHouseClient) -> Self {
        Self { client }
    }

    /// Insert a batch of metric points.
    #[instrument(skip(self, points), fields(count = points.len()))]
    pub async fn insert_batch(&self, points: &[MetricPoint]) -> Result<u64, ClickHouseError> {
        if points.is_empty() {
            return Ok(0);
        }

        let mut insert = self.client.client.insert("metrics")?;

        for point in points {
            insert.write(point).await?;
        }

        insert.end().await?;

        debug!(count = points.len(), "Inserted metric batch");
        Ok(points.len() as u64)
    }

    /// Query metrics for a run by name.
    #[instrument(skip(self))]
    pub async fn get_metrics(
        &self,
        run_id: &str,
        name: &str,
        limit: Option<u64>,
    ) -> Result<Vec<MetricPointFull>, ClickHouseError> {
        let limit = limit.unwrap_or(10000);

        let query = format!(
            r#"
            SELECT run_id, project_id, name, step, value, timestamp, ingested_at, batch_id
            FROM metrics
            WHERE run_id = ? AND name = ?
            ORDER BY step ASC
            LIMIT ?
            "#
        );

        let points = self
            .client
            .client
            .query(&query)
            .bind(run_id)
            .bind(name)
            .bind(limit)
            .fetch_all::<MetricPointFull>()
            .await?;

        Ok(points)
    }

    /// Get all metric names for a run.
    #[instrument(skip(self))]
    pub async fn get_metric_names(&self, run_id: &str) -> Result<Vec<String>, ClickHouseError> {
        let query = r#"
            SELECT DISTINCT name
            FROM metrics
            WHERE run_id = ?
            ORDER BY name
        "#;

        let names: Vec<String> = self
            .client
            .client
            .query(query)
            .bind(run_id)
            .fetch_all::<String>()
            .await?;

        Ok(names)
    }

    /// Get summary statistics for a run's metrics.
    #[instrument(skip(self))]
    pub async fn get_summaries(&self, run_id: &str) -> Result<Vec<MetricSummary>, ClickHouseError> {
        // Try to get from materialized view first (faster)
        let query = r#"
            SELECT run_id, project_id, name, min_value, max_value, last_value, last_step, count
            FROM metrics_summary FINAL
            WHERE run_id = ?
            ORDER BY name
        "#;

        let summaries = self
            .client
            .client
            .query(query)
            .bind(run_id)
            .fetch_all::<MetricSummary>()
            .await?;

        Ok(summaries)
    }

    /// Get total metric count for a run.
    #[instrument(skip(self))]
    pub async fn get_run_metric_count(&self, run_id: &str) -> Result<u64, ClickHouseError> {
        let query = r#"
            SELECT sum(count) as total
            FROM run_metrics_count FINAL
            WHERE run_id = ?
        "#;

        let count: u64 = self
            .client
            .client
            .query(query)
            .bind(run_id)
            .fetch_one()
            .await
            .unwrap_or(0);

        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ClickHouseConfig::default();
        assert_eq!(config.url, "http://localhost:8123");
        assert_eq!(config.database, "mlrunx");
        assert_eq!(config.user, "mlrunx");
    }

    #[test]
    fn test_metric_point_creation() {
        let point = MetricPoint {
            run_id: "run-123".to_string(),
            project_id: "project-1".to_string(),
            name: "loss".to_string(),
            step: 100,
            value: 0.5,
            timestamp: time::OffsetDateTime::now_utc(),
            batch_id: "batch-1".to_string(),
        };

        assert_eq!(point.name, "loss");
        assert_eq!(point.step, 100);
    }
}
