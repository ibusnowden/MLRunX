//! SQLite storage implementation for local development.
//!
//! Provides persistent storage without requiring Docker/ClickHouse/PostgreSQL.
//! Data is stored in a single SQLite file for easy development and testing.

use std::path::Path;
use std::sync::Arc;

use rusqlite::{Connection, params};
use thiserror::Error;
use tokio::sync::Mutex;
use tracing::{debug, info};

/// Errors that can occur in SQLite operations.
#[derive(Error, Debug)]
pub enum SqliteError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Configuration for SQLite storage.
#[derive(Debug, Clone)]
pub struct SqliteConfig {
    /// Path to the SQLite database file
    pub path: String,
}

impl Default for SqliteConfig {
    fn default() -> Self {
        Self {
            path: "mlrunx.db".to_string(),
        }
    }
}

impl SqliteConfig {
    /// Create config from environment variables.
    pub fn from_env() -> Self {
        Self {
            path: std::env::var("MLRUNX_SQLITE_PATH")
                .unwrap_or_else(|_| "mlrunx.db".to_string()),
        }
    }
}

/// SQLite-backed storage for runs and metrics.
///
/// This replaces InMemoryStore for persistent local development.
pub struct SqliteStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteStore {
    /// Create a new SQLite store, initializing the database schema.
    pub async fn new<P: AsRef<Path>>(path: P) -> Result<Self, SqliteError> {
        let conn = Connection::open(path)?;

        // Enable WAL mode for better concurrent read/write performance
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;

        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
        };

        // Initialize schema
        store.init_schema().await?;

        info!("SQLite store initialized");
        Ok(store)
    }

    /// Create an in-memory SQLite store (useful for testing).
    pub async fn new_in_memory() -> Result<Self, SqliteError> {
        let conn = Connection::open_in_memory()?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
        };

        store.init_schema().await?;

        Ok(store)
    }

    /// Initialize the database schema.
    async fn init_schema(&self) -> Result<(), SqliteError> {
        let conn = self.conn.lock().await;

        conn.execute_batch(r#"
            -- Projects table
            CREATE TABLE IF NOT EXISTS projects (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                description TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            );

            -- Runs table
            CREATE TABLE IF NOT EXISTS runs (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                name TEXT,
                status TEXT NOT NULL DEFAULT 'running',
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                finished_at TEXT,
                metrics_count INTEGER NOT NULL DEFAULT 0,
                params_count INTEGER NOT NULL DEFAULT 0,
                FOREIGN KEY (project_id) REFERENCES projects(id)
            );
            CREATE INDEX IF NOT EXISTS idx_runs_project ON runs(project_id);
            CREATE INDEX IF NOT EXISTS idx_runs_status ON runs(status);
            CREATE INDEX IF NOT EXISTS idx_runs_created ON runs(created_at DESC);

            -- Tags table (key-value pairs for runs)
            CREATE TABLE IF NOT EXISTS tags (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                run_id TEXT NOT NULL,
                key TEXT NOT NULL,
                value TEXT NOT NULL,
                FOREIGN KEY (run_id) REFERENCES runs(id),
                UNIQUE(run_id, key)
            );
            CREATE INDEX IF NOT EXISTS idx_tags_run ON tags(run_id);

            -- Metrics table (time series data)
            CREATE TABLE IF NOT EXISTS metrics (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                run_id TEXT NOT NULL,
                name TEXT NOT NULL,
                step INTEGER NOT NULL,
                value REAL NOT NULL,
                timestamp REAL,
                FOREIGN KEY (run_id) REFERENCES runs(id)
            );
            CREATE INDEX IF NOT EXISTS idx_metrics_run ON metrics(run_id);
            CREATE INDEX IF NOT EXISTS idx_metrics_run_name ON metrics(run_id, name);
            CREATE INDEX IF NOT EXISTS idx_metrics_run_step ON metrics(run_id, step);

            -- Parameters table (hyperparameters)
            CREATE TABLE IF NOT EXISTS params (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                run_id TEXT NOT NULL,
                name TEXT NOT NULL,
                value TEXT NOT NULL,
                FOREIGN KEY (run_id) REFERENCES runs(id),
                UNIQUE(run_id, name)
            );
            CREATE INDEX IF NOT EXISTS idx_params_run ON params(run_id);

            -- Batches table (for idempotency tracking)
            CREATE TABLE IF NOT EXISTS batches (
                id TEXT PRIMARY KEY,
                run_id TEXT NOT NULL,
                seq INTEGER NOT NULL DEFAULT 0,
                payload_hash TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                FOREIGN KEY (run_id) REFERENCES runs(id)
            );
            CREATE INDEX IF NOT EXISTS idx_batches_run ON batches(run_id);

            -- Share tokens table (public read-only links)
            CREATE TABLE IF NOT EXISTS share_tokens (
                token TEXT PRIMARY KEY,
                run_id TEXT NOT NULL,
                created_by_key_prefix TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                expires_at TEXT,
                revoked_at TEXT,
                FOREIGN KEY (run_id) REFERENCES runs(id)
            );
            CREATE INDEX IF NOT EXISTS idx_share_tokens_run ON share_tokens(run_id);
        "#)?;

        debug!("SQLite schema initialized");
        Ok(())
    }

    // =========================================================================
    // Project operations
    // =========================================================================

    /// Get or create a project by name.
    pub async fn get_or_create_project(&self, name: &str) -> Result<String, SqliteError> {
        let conn = self.conn.lock().await;

        // Try to get existing
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM projects WHERE name = ?1",
                params![name],
                |row| row.get(0),
            )
            .ok();

        if let Some(id) = existing {
            return Ok(id);
        }

        // Create new
        let id = uuid::Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO projects (id, name) VALUES (?1, ?2)",
            params![id, name],
        )?;

        info!(project_id = %id, name = %name, "Created project");
        Ok(id)
    }

    // =========================================================================
    // Run operations
    // =========================================================================

    /// Create a new run.
    pub async fn create_run(
        &self,
        run_id: &str,
        project_id: &str,
        name: Option<&str>,
    ) -> Result<(), SqliteError> {
        let conn = self.conn.lock().await;

        conn.execute(
            "INSERT INTO runs (id, project_id, name, status) VALUES (?1, ?2, ?3, 'running')",
            params![run_id, project_id, name],
        )?;

        debug!(run_id = %run_id, project = %project_id, "Created run");
        Ok(())
    }

    /// Get the project_id for a run (lightweight ownership check).
    pub async fn get_run_project_id(&self, run_id: &str) -> Result<String, SqliteError> {
        let conn = self.conn.lock().await;

        conn.query_row(
            "SELECT project_id FROM runs WHERE id = ?1",
            params![run_id],
            |row| row.get(0),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                SqliteError::NotFound(format!("Run not found: {}", run_id))
            }
            _ => SqliteError::Database(e),
        })
    }

    /// Check if a run exists.
    pub async fn run_exists(&self, run_id: &str) -> Result<bool, SqliteError> {
        let conn = self.conn.lock().await;

        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM runs WHERE id = ?1",
            params![run_id],
            |row| row.get(0),
        )?;

        Ok(count > 0)
    }

    /// Get run by ID.
    pub async fn get_run(&self, run_id: &str) -> Result<RunRow, SqliteError> {
        let conn = self.conn.lock().await;

        conn.query_row(
            r#"SELECT id, project_id, name, status, created_at, updated_at,
                      finished_at, metrics_count, params_count,
                      (julianday(COALESCE(finished_at, updated_at)) - julianday(created_at)) * 86400.0
               FROM runs WHERE id = ?1"#,
            params![run_id],
            |row| {
                Ok(RunRow {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    name: row.get(2)?,
                    status: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                    finished_at: row.get(6)?,
                    metrics_count: row.get(7)?,
                    params_count: row.get(8)?,
                    duration_seconds: row.get(9)?,
                })
            },
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                SqliteError::NotFound(format!("Run not found: {}", run_id))
            }
            _ => SqliteError::Database(e),
        })
    }

    /// List runs with optional filtering.
    pub async fn list_runs(
        &self,
        project: Option<&str>,
        status: Option<&str>,
        query: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<RunRow>, usize), SqliteError> {
        let conn = self.conn.lock().await;

        let mut sql = String::from(
            "SELECT r.id, r.project_id, r.name, r.status, r.created_at, r.updated_at,
                    r.finished_at, r.metrics_count, r.params_count,
                    (julianday(COALESCE(r.finished_at, r.updated_at)) - julianday(r.created_at)) * 86400.0
             FROM runs r
             LEFT JOIN projects p ON r.project_id = p.id
             WHERE 1=1"
        );
        let mut count_sql = String::from(
            "SELECT COUNT(*) FROM runs r LEFT JOIN projects p ON r.project_id = p.id WHERE 1=1",
        );

        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = vec![];

        if let Some(p) = project {
            sql.push_str(" AND (r.project_id = ? OR p.name = ?)");
            count_sql.push_str(" AND (r.project_id = ? OR p.name = ?)");
            params_vec.push(Box::new(p.to_string()));
            params_vec.push(Box::new(p.to_string()));
        }

        if let Some(s) = status {
            sql.push_str(" AND r.status = ?");
            count_sql.push_str(" AND r.status = ?");
            params_vec.push(Box::new(s.to_string()));
        }

        if let Some(q) = query {
            let pattern = format!("%{}%", q);
            sql.push_str(" AND (r.name LIKE ? OR r.id LIKE ?)");
            count_sql.push_str(" AND (r.name LIKE ? OR r.id LIKE ?)");
            params_vec.push(Box::new(pattern.clone()));
            params_vec.push(Box::new(pattern));
        }

        sql.push_str(" ORDER BY r.created_at DESC LIMIT ? OFFSET ?");

        // Get total count
        let total: usize = {
            let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
            conn.query_row(&count_sql, params_refs.as_slice(), |row| row.get(0))?
        };

        // Add limit and offset
        params_vec.push(Box::new(limit as i64));
        params_vec.push(Box::new(offset as i64));

        // Get runs
        let params_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            Ok(RunRow {
                id: row.get(0)?,
                project_id: row.get(1)?,
                name: row.get(2)?,
                status: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
                finished_at: row.get(6)?,
                metrics_count: row.get(7)?,
                params_count: row.get(8)?,
                duration_seconds: row.get(9)?,
            })
        })?;

        let runs: Result<Vec<_>, _> = rows.collect();
        Ok((runs?, total))
    }

    /// Delete a run and all its associated data (metrics, tags, params, batches).
    pub async fn delete_run(&self, run_id: &str) -> Result<(), SqliteError> {
        let conn = self.conn.lock().await;

        // Delete related data first (no ON DELETE CASCADE in schema)
        conn.execute("DELETE FROM metrics WHERE run_id = ?1", params![run_id])?;
        conn.execute("DELETE FROM tags WHERE run_id = ?1", params![run_id])?;
        conn.execute("DELETE FROM params WHERE run_id = ?1", params![run_id])?;
        conn.execute("DELETE FROM batches WHERE run_id = ?1", params![run_id])?;

        // Delete the run itself
        let changes = conn.execute("DELETE FROM runs WHERE id = ?1", params![run_id])?;

        if changes == 0 {
            return Err(SqliteError::NotFound(format!("Run not found: {}", run_id)));
        }

        info!(run_id = %run_id, "Deleted run and associated data");
        Ok(())
    }

    /// Update run status.
    pub async fn finish_run(&self, run_id: &str, status: &str) -> Result<(), SqliteError> {
        let conn = self.conn.lock().await;

        conn.execute(
            "UPDATE runs SET status = ?1, finished_at = datetime('now'), updated_at = datetime('now') WHERE id = ?2",
            params![status, run_id],
        )?;

        debug!(run_id = %run_id, status = %status, "Finished run");
        Ok(())
    }

    /// Update run metrics count.
    pub async fn increment_metrics_count(&self, run_id: &str, count: i64) -> Result<(), SqliteError> {
        let conn = self.conn.lock().await;

        conn.execute(
            "UPDATE runs SET metrics_count = metrics_count + ?1, updated_at = datetime('now') WHERE id = ?2",
            params![count, run_id],
        )?;

        Ok(())
    }

    // =========================================================================
    // Tag operations
    // =========================================================================

    /// Get tags for a run.
    pub async fn get_tags(&self, run_id: &str) -> Result<Vec<(String, String)>, SqliteError> {
        let conn = self.conn.lock().await;

        let mut stmt = conn.prepare("SELECT key, value FROM tags WHERE run_id = ?1")?;
        let rows = stmt.query_map(params![run_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        let tags: Result<Vec<_>, _> = rows.collect();
        Ok(tags?)
    }

    /// Set tags for a run (upsert).
    pub async fn set_tags(&self, run_id: &str, tags: &[(String, String)]) -> Result<(), SqliteError> {
        let conn = self.conn.lock().await;

        for (key, value) in tags {
            conn.execute(
                "INSERT OR REPLACE INTO tags (run_id, key, value) VALUES (?1, ?2, ?3)",
                params![run_id, key, value],
            )?;
        }

        Ok(())
    }

    // =========================================================================
    // Metric operations
    // =========================================================================

    /// Insert metrics batch.
    pub async fn insert_metrics(&self, run_id: &str, metrics: &[MetricRow]) -> Result<usize, SqliteError> {
        let conn = self.conn.lock().await;

        let mut count = 0;
        for metric in metrics {
            conn.execute(
                "INSERT INTO metrics (run_id, name, step, value, timestamp) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![run_id, metric.name, metric.step, metric.value, metric.timestamp],
            )?;
            count += 1;
        }

        Ok(count)
    }

    /// Get metrics for a run.
    pub async fn get_metrics(
        &self,
        run_id: &str,
        names: &[String],
        max_points: usize,
    ) -> Result<Vec<MetricSeriesRow>, SqliteError> {
        let conn = self.conn.lock().await;

        // Get available metric names
        let names_to_query: Vec<String> = if names.is_empty() {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT name FROM metrics WHERE run_id = ?1 ORDER BY name"
            )?;
            let rows = stmt.query_map(params![run_id], |row| row.get(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        } else {
            names.to_vec()
        };

        let mut result = Vec::new();

        for name in &names_to_query {
            // Get total points for this metric
            let total_points: usize = conn.query_row(
                "SELECT COUNT(*) FROM metrics WHERE run_id = ?1 AND name = ?2",
                params![run_id, name],
                |row| row.get(0),
            )?;

            let points = if total_points <= max_points {
                // No downsampling needed
                let mut stmt = conn.prepare(
                    "SELECT step, value FROM metrics WHERE run_id = ?1 AND name = ?2 ORDER BY step"
                )?;
                let rows = stmt.query_map(params![run_id, name], |row| {
                    Ok(AggregatedPointRow {
                        step: row.get(0)?,
                        mean: row.get(1)?,
                        min: row.get::<_, f64>(1)?,
                        max: row.get::<_, f64>(1)?,
                        count: 1,
                    })
                })?;
                rows.collect::<Result<Vec<_>, _>>()?
            } else {
                // Downsampling: use bucket aggregation
                let bucket_count = max_points;

                // Get step range
                let (min_step, max_step): (i64, i64) = conn.query_row(
                    "SELECT MIN(step), MAX(step) FROM metrics WHERE run_id = ?1 AND name = ?2",
                    params![run_id, name],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;

                let step_range = (max_step - min_step).max(1) as f64;
                let bucket_size = step_range / bucket_count as f64;

                // Query with bucket aggregation
                let mut stmt = conn.prepare(&format!(
                    r#"SELECT
                        CAST((?3 + (CAST((step - ?3) / ?4 AS INTEGER) * ?4)) AS INTEGER) as bucket_step,
                        AVG(value) as mean,
                        MIN(value) as min_val,
                        MAX(value) as max_val,
                        COUNT(*) as cnt
                    FROM metrics
                    WHERE run_id = ?1 AND name = ?2
                    GROUP BY bucket_step
                    ORDER BY bucket_step
                    LIMIT ?5"#
                ))?;

                let rows = stmt.query_map(
                    params![run_id, name, min_step, bucket_size as i64, bucket_count as i64],
                    |row| {
                        Ok(AggregatedPointRow {
                            step: row.get(0)?,
                            mean: row.get(1)?,
                            min: row.get(2)?,
                            max: row.get(3)?,
                            count: row.get(4)?,
                        })
                    }
                )?;
                rows.collect::<Result<Vec<_>, _>>()?
            };

            result.push(MetricSeriesRow {
                name: name.clone(),
                points,
                total_points,
                downsampled: total_points > max_points,
            });
        }

        Ok(result)
    }

    /// Get available metric names for a run.
    pub async fn get_metric_names(&self, run_id: &str) -> Result<Vec<String>, SqliteError> {
        let conn = self.conn.lock().await;

        let mut stmt = conn.prepare(
            "SELECT DISTINCT name FROM metrics WHERE run_id = ?1 ORDER BY name"
        )?;
        let rows = stmt.query_map(params![run_id], |row| row.get(0))?;

        let names: Result<Vec<_>, _> = rows.collect();
        Ok(names?)
    }

    // =========================================================================
    // Param operations
    // =========================================================================

    /// Insert params (upsert).
    pub async fn insert_params(&self, run_id: &str, params: &[(String, String)]) -> Result<usize, SqliteError> {
        let conn = self.conn.lock().await;

        let mut count = 0;
        for (name, value) in params {
            conn.execute(
                "INSERT OR REPLACE INTO params (run_id, name, value) VALUES (?1, ?2, ?3)",
                params![run_id, name, value],
            )?;
            count += 1;
        }

        // Update params count
        conn.execute(
            "UPDATE runs SET params_count = (SELECT COUNT(*) FROM params WHERE run_id = ?1), updated_at = datetime('now') WHERE id = ?1",
            params![run_id],
        )?;

        Ok(count)
    }

    // =========================================================================
    // Idempotency operations
    // =========================================================================

    /// Check if a batch has been processed.
    pub async fn batch_exists(&self, batch_id: &str) -> Result<bool, SqliteError> {
        let conn = self.conn.lock().await;

        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM batches WHERE id = ?1",
            params![batch_id],
            |row| row.get(0),
        )?;

        Ok(count > 0)
    }

    /// Record a processed batch.
    pub async fn record_batch(
        &self,
        batch_id: &str,
        run_id: &str,
        seq: i64,
        payload_hash: &str,
    ) -> Result<(), SqliteError> {
        let conn = self.conn.lock().await;

        conn.execute(
            "INSERT INTO batches (id, run_id, seq, payload_hash) VALUES (?1, ?2, ?3, ?4)",
            params![batch_id, run_id, seq, payload_hash],
        )?;

        Ok(())
    }

    // =========================================================================
    // Share token operations
    // =========================================================================

    /// Create a share token for a run.
    pub async fn create_share_token(
        &self,
        token: &str,
        run_id: &str,
        created_by_key_prefix: Option<&str>,
        expires_at: Option<&str>,
    ) -> Result<(), SqliteError> {
        let conn = self.conn.lock().await;

        conn.execute(
            "INSERT INTO share_tokens (token, run_id, created_by_key_prefix, expires_at) VALUES (?1, ?2, ?3, ?4)",
            params![token, run_id, created_by_key_prefix, expires_at],
        )?;

        debug!(run_id = %run_id, "Created share token");
        Ok(())
    }

    /// Validate a share token and return the associated run_id if valid.
    pub async fn validate_share_token(&self, token: &str) -> Result<ShareTokenRow, SqliteError> {
        let conn = self.conn.lock().await;

        let row = conn.query_row(
            r#"SELECT token, run_id, created_by_key_prefix, created_at, expires_at, revoked_at
               FROM share_tokens WHERE token = ?1"#,
            params![token],
            |row| {
                Ok(ShareTokenRow {
                    token: row.get(0)?,
                    run_id: row.get(1)?,
                    created_by_key_prefix: row.get(2)?,
                    created_at: row.get(3)?,
                    expires_at: row.get(4)?,
                    revoked_at: row.get(5)?,
                })
            },
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                SqliteError::NotFound("Invalid or expired share link.".to_string())
            }
            _ => SqliteError::Database(e),
        })?;

        // Check if revoked
        if row.revoked_at.is_some() {
            return Err(SqliteError::NotFound("This share link has been revoked.".to_string()));
        }

        // Check if expired
        if let Some(ref expires) = row.expires_at {
            let now: String = conn.query_row(
                "SELECT datetime('now')", [], |r| r.get(0)
            )?;
            if now > *expires {
                return Err(SqliteError::NotFound("This share link has expired.".to_string()));
            }
        }

        Ok(row)
    }

    /// List share tokens for a run.
    pub async fn list_share_tokens(&self, run_id: &str) -> Result<Vec<ShareTokenRow>, SqliteError> {
        let conn = self.conn.lock().await;

        let mut stmt = conn.prepare(
            r#"SELECT token, run_id, created_by_key_prefix, created_at, expires_at, revoked_at
               FROM share_tokens WHERE run_id = ?1 ORDER BY created_at DESC"#
        )?;
        let rows = stmt.query_map(params![run_id], |row| {
            Ok(ShareTokenRow {
                token: row.get(0)?,
                run_id: row.get(1)?,
                created_by_key_prefix: row.get(2)?,
                created_at: row.get(3)?,
                expires_at: row.get(4)?,
                revoked_at: row.get(5)?,
            })
        })?;

        let tokens: Result<Vec<_>, _> = rows.collect();
        Ok(tokens?)
    }

    /// Revoke a share token.
    pub async fn revoke_share_token(&self, token: &str) -> Result<(), SqliteError> {
        let conn = self.conn.lock().await;

        let changes = conn.execute(
            "UPDATE share_tokens SET revoked_at = datetime('now') WHERE token = ?1 AND revoked_at IS NULL",
            params![token],
        )?;

        if changes == 0 {
            return Err(SqliteError::NotFound("Share token not found or already revoked.".to_string()));
        }

        Ok(())
    }
}

/// A row from the runs table.
#[derive(Debug, Clone)]
pub struct RunRow {
    pub id: String,
    pub project_id: String,
    pub name: Option<String>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub finished_at: Option<String>,
    pub metrics_count: i64,
    pub params_count: i64,
    pub duration_seconds: Option<f64>,
}

/// A share token row.
#[derive(Debug, Clone)]
pub struct ShareTokenRow {
    pub token: String,
    pub run_id: String,
    pub created_by_key_prefix: Option<String>,
    pub created_at: String,
    pub expires_at: Option<String>,
    pub revoked_at: Option<String>,
}

/// A metric data point.
#[derive(Debug, Clone)]
pub struct MetricRow {
    pub name: String,
    pub step: i64,
    pub value: f64,
    pub timestamp: Option<f64>,
}

/// An aggregated metric point (for downsampling).
#[derive(Debug, Clone)]
pub struct AggregatedPointRow {
    pub step: i64,
    pub mean: f64,
    pub min: f64,
    pub max: f64,
    pub count: usize,
}

/// A metric series with points.
#[derive(Debug, Clone)]
pub struct MetricSeriesRow {
    pub name: String,
    pub points: Vec<AggregatedPointRow>,
    pub total_points: usize,
    pub downsampled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn create_test_store() -> SqliteStore {
        SqliteStore::new_in_memory().await.unwrap()
    }

    #[tokio::test]
    async fn test_create_project() {
        let store = create_test_store().await;

        let id1 = store.get_or_create_project("test-project").await.unwrap();
        let id2 = store.get_or_create_project("test-project").await.unwrap();

        // Should return same ID (idempotent)
        assert_eq!(id1, id2);
    }

    #[tokio::test]
    async fn test_create_run() {
        let store = create_test_store().await;

        let project_id = store.get_or_create_project("test-project").await.unwrap();
        store.create_run("run-123", &project_id, Some("My Run")).await.unwrap();

        let run = store.get_run("run-123").await.unwrap();
        assert_eq!(run.id, "run-123");
        assert_eq!(run.name, Some("My Run".to_string()));
        assert_eq!(run.status, "running");
    }

    #[tokio::test]
    async fn test_insert_and_query_metrics() {
        let store = create_test_store().await;

        let project_id = store.get_or_create_project("test-project").await.unwrap();
        store.create_run("run-123", &project_id, None).await.unwrap();

        // Insert some metrics
        let metrics: Vec<MetricRow> = (0..100)
            .map(|i| MetricRow {
                name: "loss".to_string(),
                step: i,
                value: 1.0 - (i as f64 * 0.01),
                timestamp: None,
            })
            .collect();

        store.insert_metrics("run-123", &metrics).await.unwrap();

        // Query without downsampling
        let series = store.get_metrics("run-123", &[], 200).await.unwrap();
        assert_eq!(series.len(), 1);
        assert_eq!(series[0].name, "loss");
        assert_eq!(series[0].points.len(), 100);
        assert!(!series[0].downsampled);

        // Query with downsampling
        let series = store.get_metrics("run-123", &[], 10).await.unwrap();
        assert_eq!(series.len(), 1);
        assert!(series[0].points.len() <= 10);
        assert!(series[0].downsampled);
    }

    #[tokio::test]
    async fn test_tags() {
        let store = create_test_store().await;

        let project_id = store.get_or_create_project("test-project").await.unwrap();
        store.create_run("run-123", &project_id, None).await.unwrap();

        // Set tags
        store.set_tags("run-123", &[
            ("framework".to_string(), "pytorch".to_string()),
            ("task".to_string(), "classification".to_string()),
        ]).await.unwrap();

        // Get tags
        let tags = store.get_tags("run-123").await.unwrap();
        assert_eq!(tags.len(), 2);
    }
}
