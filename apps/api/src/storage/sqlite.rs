//! SQLite storage implementation for local development.
//!
//! Provides persistent storage without requiring Docker/ClickHouse/PostgreSQL.
//! Data is stored in a single SQLite file for easy development and testing.

use std::path::Path;
use std::sync::Arc;

use rusqlite::{Connection, OptionalExtension, params};
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
            path: std::env::var("MLRUNX_SQLITE_PATH").unwrap_or_else(|_| "mlrunx.db".to_string()),
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

            -- Users table (Option-2 auth foundation)
            CREATE TABLE IF NOT EXISTS users (
                id TEXT PRIMARY KEY,
                email TEXT,
                display_name TEXT,
                auth_provider TEXT NOT NULL DEFAULT 'local',
                external_subject TEXT,
                is_service_account INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                disabled_at TEXT,
                UNIQUE(auth_provider, external_subject)
            );
            CREATE INDEX IF NOT EXISTS idx_users_email ON users(email);

            -- Project memberships table (owner/editor/viewer)
            CREATE TABLE IF NOT EXISTS project_memberships (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                user_id TEXT NOT NULL,
                role TEXT NOT NULL CHECK(role IN ('owner', 'editor', 'viewer')),
                granted_by_user_id TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                revoked_at TEXT,
                FOREIGN KEY (project_id) REFERENCES projects(id),
                FOREIGN KEY (user_id) REFERENCES users(id),
                FOREIGN KEY (granted_by_user_id) REFERENCES users(id)
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_project_memberships_active_unique
                ON project_memberships(project_id, user_id)
                WHERE revoked_at IS NULL;
            CREATE INDEX IF NOT EXISTS idx_project_memberships_user
                ON project_memberships(user_id, project_id);

            -- API keys table (persistent backing store for PR2)
            CREATE TABLE IF NOT EXISTS api_keys (
                id TEXT PRIMARY KEY,
                key_hash TEXT NOT NULL UNIQUE,
                key_prefix TEXT NOT NULL,
                project_id TEXT,
                created_by_user_id TEXT,
                name TEXT,
                description TEXT,
                scopes TEXT NOT NULL DEFAULT '[]',
                metadata TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                last_used_at TEXT,
                revoked_at TEXT,
                expires_at TEXT,
                FOREIGN KEY (project_id) REFERENCES projects(id),
                FOREIGN KEY (created_by_user_id) REFERENCES users(id)
            );
            CREATE INDEX IF NOT EXISTS idx_api_keys_project ON api_keys(project_id);
            CREATE INDEX IF NOT EXISTS idx_api_keys_created_by_user ON api_keys(created_by_user_id);

            -- Audit events table
            CREATE TABLE IF NOT EXISTS audit_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                occurred_at TEXT NOT NULL DEFAULT (datetime('now')),
                actor_user_id TEXT,
                actor_key_id TEXT,
                project_id TEXT,
                run_id TEXT,
                action TEXT NOT NULL,
                resource_type TEXT NOT NULL,
                resource_id TEXT,
                outcome TEXT NOT NULL DEFAULT 'success',
                request_id TEXT,
                client_ip TEXT,
                user_agent TEXT,
                metadata TEXT NOT NULL DEFAULT '{}',
                FOREIGN KEY (actor_user_id) REFERENCES users(id),
                FOREIGN KEY (actor_key_id) REFERENCES api_keys(id),
                FOREIGN KEY (project_id) REFERENCES projects(id),
                FOREIGN KEY (run_id) REFERENCES runs(id)
            );
            CREATE INDEX IF NOT EXISTS idx_audit_events_occurred ON audit_events(occurred_at DESC);
            CREATE INDEX IF NOT EXISTS idx_audit_events_project ON audit_events(project_id, occurred_at DESC);

            -- UI auth sessions table (feature-flagged JWT/session auth for UI).
            CREATE TABLE IF NOT EXISTS auth_sessions (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                token_hash TEXT NOT NULL UNIQUE,
                csrf_hash TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                last_seen_at TEXT,
                expires_at TEXT NOT NULL,
                revoked_at TEXT,
                replaced_by_session_id TEXT,
                user_agent TEXT,
                client_ip TEXT,
                FOREIGN KEY (user_id) REFERENCES users(id),
                FOREIGN KEY (replaced_by_session_id) REFERENCES auth_sessions(id)
            );
            CREATE INDEX IF NOT EXISTS idx_auth_sessions_user ON auth_sessions(user_id, expires_at);
            CREATE INDEX IF NOT EXISTS idx_auth_sessions_token_hash ON auth_sessions(token_hash);

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
    // User / membership operations
    // =========================================================================

    /// Get or create a user identity by provider + subject.
    pub async fn get_or_create_user_identity(
        &self,
        auth_provider: &str,
        external_subject: &str,
        email: Option<&str>,
        display_name: Option<&str>,
    ) -> Result<String, SqliteError> {
        let conn = self.conn.lock().await;

        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM users WHERE auth_provider = ?1 AND external_subject = ?2",
                params![auth_provider, external_subject],
                |row| row.get(0),
            )
            .optional()?;

        if let Some(user_id) = existing {
            conn.execute(
                "UPDATE users SET email = COALESCE(?1, email), display_name = COALESCE(?2, display_name), updated_at = datetime('now') WHERE id = ?3",
                params![email, display_name, user_id],
            )?;
            return Ok(user_id);
        }

        let user_id = uuid::Uuid::now_v7().to_string();
        conn.execute(
            "INSERT INTO users (id, email, display_name, auth_provider, external_subject) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![user_id, email, display_name, auth_provider, external_subject],
        )?;

        Ok(user_id)
    }

    /// Grant a project membership role to a user.
    pub async fn grant_project_membership(
        &self,
        project_id: &str,
        user_id: &str,
        role: &str,
        granted_by_user_id: Option<&str>,
    ) -> Result<(), SqliteError> {
        let conn = self.conn.lock().await;

        conn.execute(
            "UPDATE project_memberships SET revoked_at = datetime('now'), updated_at = datetime('now') WHERE project_id = ?1 AND user_id = ?2 AND revoked_at IS NULL",
            params![project_id, user_id],
        )?;

        conn.execute(
            "INSERT INTO project_memberships (id, project_id, user_id, role, granted_by_user_id) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![uuid::Uuid::now_v7().to_string(), project_id, user_id, role, granted_by_user_id],
        )?;

        Ok(())
    }

    /// List active project memberships for a user.
    pub async fn list_active_project_memberships(
        &self,
        user_id: &str,
    ) -> Result<Vec<ProjectMembershipRow>, SqliteError> {
        let conn = self.conn.lock().await;

        let mut stmt = conn.prepare(
            r#"SELECT project_id, role
               FROM project_memberships
               WHERE user_id = ?1 AND revoked_at IS NULL
               ORDER BY created_at DESC"#,
        )?;
        let rows = stmt.query_map(params![user_id], |row| {
            Ok(ProjectMembershipRow {
                project_id: row.get(0)?,
                role: row.get(1)?,
            })
        })?;

        let memberships: Result<Vec<_>, _> = rows.collect();
        Ok(memberships?)
    }

    /// Insert a security/audit event.
    pub async fn insert_audit_event(
        &self,
        actor_user_id: Option<&str>,
        actor_key_id: Option<&str>,
        project_id: Option<&str>,
        run_id: Option<&str>,
        action: &str,
        resource_type: &str,
        resource_id: Option<&str>,
        outcome: &str,
        metadata_json: Option<&str>,
    ) -> Result<(), SqliteError> {
        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO audit_events (actor_user_id, actor_key_id, project_id, run_id, action, resource_type, resource_id, outcome, metadata) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, COALESCE(?9, '{}'))",
            params![actor_user_id, actor_key_id, project_id, run_id, action, resource_type, resource_id, outcome, metadata_json],
        )?;
        Ok(())
    }

    /// Create a UI auth session.
    pub async fn insert_auth_session(
        &self,
        id: &str,
        user_id: &str,
        token_hash: &str,
        csrf_hash: &str,
        expires_at: &str,
        user_agent: Option<&str>,
        client_ip: Option<&str>,
    ) -> Result<(), SqliteError> {
        let conn = self.conn.lock().await;

        conn.execute(
            "INSERT INTO auth_sessions (id, user_id, token_hash, csrf_hash, expires_at, user_agent, client_ip) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, user_id, token_hash, csrf_hash, expires_at, user_agent, client_ip],
        )?;

        Ok(())
    }

    /// Get an active (not revoked, not expired) auth session by token hash.
    pub async fn get_active_auth_session_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<AuthSessionRow>, SqliteError> {
        let conn = self.conn.lock().await;

        conn.query_row(
            r#"
            SELECT id, user_id, token_hash, csrf_hash, expires_at, revoked_at
            FROM auth_sessions
            WHERE token_hash = ?1
              AND revoked_at IS NULL
              AND expires_at > datetime('now')
            LIMIT 1
            "#,
            params![token_hash],
            |row| {
                Ok(AuthSessionRow {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    token_hash: row.get(2)?,
                    csrf_hash: row.get(3)?,
                    expires_at: row.get(4)?,
                    revoked_at: row.get(5)?,
                })
            },
        )
        .optional()
        .map_err(SqliteError::from)
    }

    /// Update an auth session last-seen timestamp.
    pub async fn touch_auth_session(&self, session_id: &str) -> Result<(), SqliteError> {
        let conn = self.conn.lock().await;
        conn.execute(
            "UPDATE auth_sessions SET last_seen_at = datetime('now'), updated_at = datetime('now') WHERE id = ?1",
            params![session_id],
        )?;
        Ok(())
    }

    /// Revoke an auth session by token hash.
    pub async fn revoke_auth_session_by_token_hash(
        &self,
        token_hash: &str,
    ) -> Result<bool, SqliteError> {
        let conn = self.conn.lock().await;
        let changed = conn.execute(
            "UPDATE auth_sessions SET revoked_at = datetime('now'), updated_at = datetime('now') WHERE token_hash = ?1 AND revoked_at IS NULL",
            params![token_hash],
        )?;
        Ok(changed > 0)
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
            let params_refs: Vec<&dyn rusqlite::ToSql> =
                params_vec.iter().map(|p| p.as_ref()).collect();
            conn.query_row(&count_sql, params_refs.as_slice(), |row| row.get(0))?
        };

        // Add limit and offset
        params_vec.push(Box::new(limit as i64));
        params_vec.push(Box::new(offset as i64));

        // Get runs
        let params_refs: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();
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
    pub async fn increment_metrics_count(
        &self,
        run_id: &str,
        count: i64,
    ) -> Result<(), SqliteError> {
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
    pub async fn set_tags(
        &self,
        run_id: &str,
        tags: &[(String, String)],
    ) -> Result<(), SqliteError> {
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
    pub async fn insert_metrics(
        &self,
        run_id: &str,
        metrics: &[MetricRow],
    ) -> Result<usize, SqliteError> {
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
            let mut stmt =
                conn.prepare("SELECT DISTINCT name FROM metrics WHERE run_id = ?1 ORDER BY name")?;
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
                    "SELECT step, value FROM metrics WHERE run_id = ?1 AND name = ?2 ORDER BY step",
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
                    params![
                        run_id,
                        name,
                        min_step,
                        bucket_size as i64,
                        bucket_count as i64
                    ],
                    |row| {
                        Ok(AggregatedPointRow {
                            step: row.get(0)?,
                            mean: row.get(1)?,
                            min: row.get(2)?,
                            max: row.get(3)?,
                            count: row.get(4)?,
                        })
                    },
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

        let mut stmt =
            conn.prepare("SELECT DISTINCT name FROM metrics WHERE run_id = ?1 ORDER BY name")?;
        let rows = stmt.query_map(params![run_id], |row| row.get(0))?;

        let names: Result<Vec<_>, _> = rows.collect();
        Ok(names?)
    }

    // =========================================================================
    // Param operations
    // =========================================================================

    /// Insert params (upsert).
    pub async fn insert_params(
        &self,
        run_id: &str,
        params: &[(String, String)],
    ) -> Result<usize, SqliteError> {
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
    // API key operations
    // =========================================================================

    /// Insert a new API key record.
    pub async fn insert_api_key(
        &self,
        id: &str,
        key_hash: &str,
        key_prefix: &str,
        project_id: Option<&str>,
        name: Option<&str>,
        scopes_json: &str,
    ) -> Result<(), SqliteError> {
        let conn = self.conn.lock().await;

        conn.execute(
            r#"INSERT INTO api_keys (id, key_hash, key_prefix, project_id, name, scopes)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6)"#,
            params![id, key_hash, key_prefix, project_id, name, scopes_json],
        )?;

        Ok(())
    }

    /// Ensure a bootstrap admin key exists and is active.
    pub async fn upsert_bootstrap_api_key(
        &self,
        id: &str,
        key_hash: &str,
        key_prefix: &str,
    ) -> Result<(), SqliteError> {
        let conn = self.conn.lock().await;

        conn.execute(
            r#"INSERT INTO api_keys (id, key_hash, key_prefix, project_id, name, scopes, revoked_at)
               VALUES (?1, ?2, ?3, NULL, 'bootstrap', '["admin"]', NULL)
               ON CONFLICT(key_hash) DO UPDATE SET
                   key_prefix = excluded.key_prefix,
                   project_id = NULL,
                   scopes = '["admin"]',
                   revoked_at = NULL,
                   updated_at = datetime('now')"#,
            params![id, key_hash, key_prefix],
        )?;

        Ok(())
    }

    /// Fetch an API key by its hash.
    pub async fn get_api_key_by_hash(
        &self,
        key_hash: &str,
    ) -> Result<Option<ApiKeyRow>, SqliteError> {
        let conn = self.conn.lock().await;

        let row = conn
            .query_row(
                r#"SELECT id, key_hash, key_prefix, project_id, name, scopes, created_at, last_used_at, revoked_at
                   FROM api_keys
                   WHERE key_hash = ?1"#,
                params![key_hash],
                |row| {
                    Ok(ApiKeyRow {
                        id: row.get(0)?,
                        key_hash: row.get(1)?,
                        key_prefix: row.get(2)?,
                        project_id: row.get(3)?,
                        name: row.get(4)?,
                        scopes_json: row.get(5)?,
                        created_at: row.get(6)?,
                        last_used_at: row.get(7)?,
                        revoked_at: row.get(8)?,
                    })
                },
            )
            .optional()?;

        Ok(row)
    }

    /// Update last-used timestamp for an API key.
    pub async fn touch_api_key_last_used(&self, key_hash: &str) -> Result<(), SqliteError> {
        let conn = self.conn.lock().await;

        conn.execute(
            "UPDATE api_keys SET last_used_at = datetime('now'), updated_at = datetime('now') WHERE key_hash = ?1",
            params![key_hash],
        )?;

        Ok(())
    }

    /// Revoke an API key by hash.
    pub async fn revoke_api_key_by_hash(&self, key_hash: &str) -> Result<bool, SqliteError> {
        let conn = self.conn.lock().await;

        let changes = conn.execute(
            "UPDATE api_keys SET revoked_at = datetime('now'), updated_at = datetime('now') WHERE key_hash = ?1 AND revoked_at IS NULL",
            params![key_hash],
        )?;

        Ok(changes > 0)
    }

    /// List API keys, optionally filtered by project.
    pub async fn list_api_keys(
        &self,
        project_id: Option<&str>,
    ) -> Result<Vec<ApiKeyRow>, SqliteError> {
        let conn = self.conn.lock().await;

        let mut keys = Vec::new();

        match project_id {
            Some(project_id) => {
                let mut stmt = conn.prepare(
                    r#"SELECT id, key_hash, key_prefix, project_id, name, scopes, created_at, last_used_at, revoked_at
                       FROM api_keys
                       WHERE project_id = ?1
                       ORDER BY created_at DESC"#,
                )?;
                let rows = stmt.query_map(params![project_id], |row| {
                    Ok(ApiKeyRow {
                        id: row.get(0)?,
                        key_hash: row.get(1)?,
                        key_prefix: row.get(2)?,
                        project_id: row.get(3)?,
                        name: row.get(4)?,
                        scopes_json: row.get(5)?,
                        created_at: row.get(6)?,
                        last_used_at: row.get(7)?,
                        revoked_at: row.get(8)?,
                    })
                })?;
                for row in rows {
                    keys.push(row?);
                }
            }
            None => {
                let mut stmt = conn.prepare(
                    r#"SELECT id, key_hash, key_prefix, project_id, name, scopes, created_at, last_used_at, revoked_at
                       FROM api_keys
                       ORDER BY created_at DESC"#,
                )?;
                let rows = stmt.query_map([], |row| {
                    Ok(ApiKeyRow {
                        id: row.get(0)?,
                        key_hash: row.get(1)?,
                        key_prefix: row.get(2)?,
                        project_id: row.get(3)?,
                        name: row.get(4)?,
                        scopes_json: row.get(5)?,
                        created_at: row.get(6)?,
                        last_used_at: row.get(7)?,
                        revoked_at: row.get(8)?,
                    })
                })?;
                for row in rows {
                    keys.push(row?);
                }
            }
        }

        Ok(keys)
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

        let row = conn
            .query_row(
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
            return Err(SqliteError::NotFound(
                "This share link has been revoked.".to_string(),
            ));
        }

        // Check if expired
        if let Some(ref expires) = row.expires_at {
            let now: String = conn.query_row("SELECT datetime('now')", [], |r| r.get(0))?;
            if now > *expires {
                return Err(SqliteError::NotFound(
                    "This share link has expired.".to_string(),
                ));
            }
        }

        Ok(row)
    }

    /// List share tokens for a run.
    pub async fn list_share_tokens(&self, run_id: &str) -> Result<Vec<ShareTokenRow>, SqliteError> {
        let conn = self.conn.lock().await;

        let mut stmt = conn.prepare(
            r#"SELECT token, run_id, created_by_key_prefix, created_at, expires_at, revoked_at
               FROM share_tokens WHERE run_id = ?1 ORDER BY created_at DESC"#,
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
            return Err(SqliteError::NotFound(
                "Share token not found or already revoked.".to_string(),
            ));
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

/// An API key row from sqlite.
#[derive(Debug, Clone)]
pub struct ApiKeyRow {
    pub id: String,
    pub key_hash: String,
    pub key_prefix: String,
    pub project_id: Option<String>,
    pub name: Option<String>,
    pub scopes_json: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub revoked_at: Option<String>,
}

/// A project membership row.
#[derive(Debug, Clone)]
pub struct ProjectMembershipRow {
    pub project_id: String,
    pub role: String,
}

/// A UI auth session row.
#[derive(Debug, Clone)]
pub struct AuthSessionRow {
    pub id: String,
    pub user_id: String,
    pub token_hash: String,
    pub csrf_hash: String,
    pub expires_at: String,
    pub revoked_at: Option<String>,
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
        store
            .create_run("run-123", &project_id, Some("My Run"))
            .await
            .unwrap();

        let run = store.get_run("run-123").await.unwrap();
        assert_eq!(run.id, "run-123");
        assert_eq!(run.name, Some("My Run".to_string()));
        assert_eq!(run.status, "running");
    }

    #[tokio::test]
    async fn test_insert_and_query_metrics() {
        let store = create_test_store().await;

        let project_id = store.get_or_create_project("test-project").await.unwrap();
        store
            .create_run("run-123", &project_id, None)
            .await
            .unwrap();

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
        store
            .create_run("run-123", &project_id, None)
            .await
            .unwrap();

        // Set tags
        store
            .set_tags(
                "run-123",
                &[
                    ("framework".to_string(), "pytorch".to_string()),
                    ("task".to_string(), "classification".to_string()),
                ],
            )
            .await
            .unwrap();

        // Get tags
        let tags = store.get_tags("run-123").await.unwrap();
        assert_eq!(tags.len(), 2);
    }

    #[tokio::test]
    async fn test_auth_foundation_tables_read_write() {
        let store = create_test_store().await;

        let project_id = store.get_or_create_project("auth-project").await.unwrap();
        store
            .create_run("run-auth-123", &project_id, Some("Auth Run"))
            .await
            .unwrap();

        let user_id = uuid::Uuid::now_v7().to_string();
        let admin_user_id = uuid::Uuid::now_v7().to_string();
        let key_id = uuid::Uuid::now_v7().to_string();

        {
            let conn = store.conn.lock().await;

            conn.execute(
                "INSERT INTO users (id, email, display_name, auth_provider, external_subject) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![admin_user_id, "admin@example.com", "Admin User", "local", "admin-sub"],
            ).unwrap();
            conn.execute(
                "INSERT INTO users (id, email, display_name, auth_provider, external_subject) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![user_id, "user@example.com", "Regular User", "local", "user-sub"],
            ).unwrap();

            conn.execute(
                "INSERT INTO project_memberships (id, project_id, user_id, role, granted_by_user_id) VALUES (?1, ?2, ?3, 'owner', ?4)",
                params![uuid::Uuid::now_v7().to_string(), project_id, user_id, admin_user_id],
            ).unwrap();

            conn.execute(
                "INSERT INTO api_keys (id, key_hash, key_prefix, project_id, created_by_user_id, name, scopes) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![key_id, "hash123", "mlrunx_a", project_id, user_id, "project-key", "[\"read\",\"write\"]"],
            ).unwrap();

            conn.execute(
                "INSERT INTO audit_events (actor_user_id, actor_key_id, project_id, run_id, action, resource_type, resource_id, outcome) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![user_id, key_id, project_id, "run-auth-123", "run.init", "run", "run-auth-123", "success"],
            ).unwrap();

            let role: String = conn
                .query_row(
                    "SELECT role FROM project_memberships WHERE project_id = ?1 AND user_id = ?2",
                    params![project_id, user_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(role, "owner");

            let audit_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM audit_events WHERE actor_key_id = ?1",
                    params![key_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(audit_count, 1);
        }

        let run = store.get_run("run-auth-123").await.unwrap();
        assert_eq!(run.id, "run-auth-123");
    }

    #[tokio::test]
    async fn test_insert_audit_event_method() {
        let store = create_test_store().await;
        let project_id = store.get_or_create_project("audit-project").await.unwrap();
        store
            .create_run("run-audit-123", &project_id, Some("Audit Run"))
            .await
            .unwrap();

        let user_id = uuid::Uuid::now_v7().to_string();
        {
            let conn = store.conn.lock().await;
            conn.execute(
                "INSERT INTO users (id, email, display_name, auth_provider, external_subject) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![user_id, "audit@example.com", "Audit User", "local", "audit-sub"],
            )
            .unwrap();
        }

        store
            .insert_audit_event(
                Some(&user_id),
                None,
                Some(&project_id),
                Some("run-audit-123"),
                "run.delete",
                "run",
                Some("run-audit-123"),
                "denied",
                Some(r#"{"reason":"rbac_denied"}"#),
            )
            .await
            .unwrap();

        let conn = store.conn.lock().await;
        let (outcome, metadata): (String, String) = conn
            .query_row(
                "SELECT outcome, metadata FROM audit_events WHERE action = 'run.delete' ORDER BY id DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(outcome, "denied");
        assert!(metadata.contains("rbac_denied"));
    }

    #[tokio::test]
    async fn test_api_key_storage_methods() {
        let store = create_test_store().await;
        let project_id = store.get_or_create_project("key-project").await.unwrap();

        let key_id = uuid::Uuid::now_v7().to_string();
        let key_hash = "hash-abc";
        let key_prefix = "mlrunx_a";
        let scopes_json = r#"["read","write"]"#;

        store
            .insert_api_key(
                &key_id,
                key_hash,
                key_prefix,
                Some(&project_id),
                Some("key-one"),
                scopes_json,
            )
            .await
            .unwrap();

        let fetched = store
            .get_api_key_by_hash(key_hash)
            .await
            .unwrap()
            .expect("key should exist");
        assert_eq!(fetched.id, key_id);
        assert_eq!(fetched.scopes_json, scopes_json);
        assert!(fetched.last_used_at.is_none());
        assert!(fetched.revoked_at.is_none());

        store.touch_api_key_last_used(key_hash).await.unwrap();
        let touched = store
            .get_api_key_by_hash(key_hash)
            .await
            .unwrap()
            .expect("key should exist");
        assert!(touched.last_used_at.is_some());

        let listed = store.list_api_keys(Some(&project_id)).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].key_hash, key_hash);

        let revoked = store.revoke_api_key_by_hash(key_hash).await.unwrap();
        assert!(revoked);

        let revoked_row = store
            .get_api_key_by_hash(key_hash)
            .await
            .unwrap()
            .expect("key should exist");
        assert!(revoked_row.revoked_at.is_some());
    }

    #[tokio::test]
    async fn test_user_identity_and_memberships() {
        let store = create_test_store().await;
        let project_id = store.get_or_create_project("jwt-project").await.unwrap();

        let user_id = store
            .get_or_create_user_identity(
                "jwt",
                "subject-123",
                Some("jwt@example.com"),
                Some("JWT User"),
            )
            .await
            .unwrap();

        store
            .grant_project_membership(&project_id, &user_id, "viewer", None)
            .await
            .unwrap();

        let memberships = store
            .list_active_project_memberships(&user_id)
            .await
            .unwrap();

        assert_eq!(memberships.len(), 1);
        assert_eq!(memberships[0].project_id, project_id);
        assert_eq!(memberships[0].role, "viewer");
    }
}
