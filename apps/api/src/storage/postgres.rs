//! `PostgreSQL` storage implementation for metadata.
//!
//! Provides relational storage for projects, runs, parameters, and artifacts.
//! See: /`migrations/postgres/001_metadata_schema.sql` for schema.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio_postgres::{NoTls, Row, types::ToSql};
use tracing::{instrument, warn};
use uuid::Uuid;

/// Errors that can occur in `PostgreSQL` operations.
#[derive(Error, Debug)]
pub enum PostgresError {
    #[error("Database error: {0}")]
    Database(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Validation error: {0}")]
    Validation(String),
}

/// Configuration for `PostgreSQL` connection.
#[derive(Debug, Clone)]
pub struct PostgresConfig {
    /// Connection URL (e.g., "<postgres://user:pass@localhost:5432/mlrunx>")
    pub url: String,
    /// Maximum connections in pool
    pub max_connections: u32,
    /// Minimum connections in pool
    pub min_connections: u32,
}

impl Default for PostgresConfig {
    fn default() -> Self {
        Self {
            url: "postgres://mlrunx:mlrunx_dev@localhost:5432/mlrunx".to_string(),
            max_connections: 10,
            min_connections: 2,
        }
    }
}

impl PostgresConfig {
    /// Create config from environment variables.
    pub fn from_env() -> Self {
        Self {
            url: std::env::var("DATABASE_URL").unwrap_or_else(|_| {
                "postgres://mlrunx:mlrunx_dev@localhost:5432/mlrunx".to_string()
            }),
            max_connections: std::env::var("PG_MAX_CONNECTIONS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(10),
            min_connections: std::env::var("PG_MIN_CONNECTIONS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(2),
        }
    }
}

/// Run status enum matching `PostgreSQL` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunStatus {
    Pending,
    Running,
    Finished,
    Failed,
    Killed,
}

impl std::fmt::Display for RunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Running => write!(f, "running"),
            Self::Finished => write!(f, "finished"),
            Self::Failed => write!(f, "failed"),
            Self::Killed => write!(f, "killed"),
        }
    }
}

/// Artifact type enum matching `PostgreSQL` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactType {
    Model,
    Dataset,
    Plot,
    Table,
    File,
    Directory,
    Other,
}

impl std::fmt::Display for ArtifactType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Model => write!(f, "model"),
            Self::Dataset => write!(f, "dataset"),
            Self::Plot => write!(f, "plot"),
            Self::Table => write!(f, "table"),
            Self::File => write!(f, "file"),
            Self::Directory => write!(f, "directory"),
            Self::Other => write!(f, "other"),
        }
    }
}

/// A project in the system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub owner_id: Option<Uuid>,
    pub settings: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// A run (experiment) in the system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: Option<String>,
    pub description: Option<String>,
    pub status: RunStatus,
    pub exit_code: Option<i32>,
    pub error_message: Option<String>,
    pub parent_run_id: Option<Uuid>,
    pub resume_token: Option<String>,
    pub tags: serde_json::Value,
    pub system_info: serde_json::Value,
    pub git_info: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    pub duration_seconds: Option<f64>,
}

/// A parameter (hyperparameter) for a run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Parameter {
    pub id: Uuid,
    pub run_id: Uuid,
    pub name: String,
    pub value_string: Option<String>,
    pub value_float: Option<f64>,
    pub value_int: Option<i64>,
    pub value_bool: Option<bool>,
    pub value_json: Option<serde_json::Value>,
    pub value_type: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl Parameter {
    /// Get the parameter value as a string representation.
    pub fn value_as_string(&self) -> String {
        match self.value_type.as_str() {
            "string" => self.value_string.clone().unwrap_or_default(),
            "float" => self.value_float.map(|v| v.to_string()).unwrap_or_default(),
            "int" => self.value_int.map(|v| v.to_string()).unwrap_or_default(),
            "bool" => self.value_bool.map(|v| v.to_string()).unwrap_or_default(),
            "json" => self
                .value_json
                .as_ref()
                .map(std::string::ToString::to_string)
                .unwrap_or_default(),
            _ => String::new(),
        }
    }
}

/// An artifact (file/model) produced by a run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_field_names)]
pub struct Artifact {
    pub id: Uuid,
    pub run_id: Uuid,
    pub name: String,
    #[serde(rename = "type")]
    pub artifact_type: ArtifactType,
    pub description: Option<String>,
    pub storage_path: String,
    pub storage_type: String,
    pub size_bytes: Option<i64>,
    pub mime_type: Option<String>,
    pub checksum_md5: Option<String>,
    pub checksum_sha256: Option<String>,
    pub metadata: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Summary statistics for a run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSummary {
    pub run_id: Uuid,
    pub total_metrics: i64,
    pub total_params: i32,
    pub total_artifacts: i32,
    pub best_metrics: serde_json::Value,
    pub last_metrics: serde_json::Value,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Input for creating a new project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProjectInput {
    pub id: Option<Uuid>,
    pub name: String,
    pub description: Option<String>,
    pub owner_id: Option<Uuid>,
    pub settings: Option<serde_json::Value>,
}

/// Input for creating a new run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRunInput {
    pub id: Option<Uuid>,
    pub project_id: Uuid,
    pub name: Option<String>,
    pub description: Option<String>,
    pub parent_run_id: Option<Uuid>,
    pub tags: Option<serde_json::Value>,
    pub system_info: Option<serde_json::Value>,
    pub git_info: Option<serde_json::Value>,
}

/// Input for creating a new parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateParameterInput {
    pub run_id: Uuid,
    pub name: String,
    pub value: ParameterValue,
}

/// Parameter value variants.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParameterValue {
    String(String),
    Float(f64),
    Int(i64),
    Bool(bool),
    Json(serde_json::Value),
}

/// Input for creating a new artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateArtifactInput {
    pub run_id: Uuid,
    pub name: String,
    pub artifact_type: ArtifactType,
    pub description: Option<String>,
    pub storage_path: String,
    pub storage_type: Option<String>,
    pub size_bytes: Option<i64>,
    pub mime_type: Option<String>,
    pub checksum_md5: Option<String>,
    pub checksum_sha256: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

/// Query filters for listing runs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListRunsFilter {
    pub project_id: Option<Uuid>,
    pub status: Option<RunStatus>,
    pub parent_run_id: Option<Uuid>,
    pub tags: Option<serde_json::Value>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

async fn connect_client() -> Result<tokio_postgres::Client, PostgresError> {
    let config = PostgresConfig::from_env();
    let (client, connection) = tokio_postgres::connect(&config.url, NoTls)
        .await
        .map_err(|e| PostgresError::Database(format!("Failed to connect to PostgreSQL: {e}")))?;

    tokio::spawn(async move {
        if let Err(err) = connection.await {
            warn!(error = %err, "PostgreSQL connection task ended");
        }
    });

    Ok(client)
}

fn parse_run_status(raw: &str) -> Result<RunStatus, PostgresError> {
    match raw {
        "pending" => Ok(RunStatus::Pending),
        "running" => Ok(RunStatus::Running),
        "finished" => Ok(RunStatus::Finished),
        "failed" => Ok(RunStatus::Failed),
        "killed" => Ok(RunStatus::Killed),
        _ => Err(PostgresError::Validation(format!(
            "Unknown run status from PostgreSQL: {raw}"
        ))),
    }
}

fn parse_artifact_type(raw: &str) -> Result<ArtifactType, PostgresError> {
    match raw {
        "model" => Ok(ArtifactType::Model),
        "dataset" => Ok(ArtifactType::Dataset),
        "plot" => Ok(ArtifactType::Plot),
        "table" => Ok(ArtifactType::Table),
        "file" => Ok(ArtifactType::File),
        "directory" => Ok(ArtifactType::Directory),
        "other" => Ok(ArtifactType::Other),
        _ => Err(PostgresError::Validation(format!(
            "Unknown artifact type from PostgreSQL: {raw}"
        ))),
    }
}

fn map_project_row(row: &Row) -> Project {
    let created_at: std::time::SystemTime = row.get("created_at");
    let updated_at: std::time::SystemTime = row.get("updated_at");
    Project {
        id: row.get("id"),
        name: row.get("name"),
        description: row.get("description"),
        owner_id: row.get("owner_id"),
        settings: row.get("settings"),
        created_at: chrono::DateTime::<chrono::Utc>::from(created_at),
        updated_at: chrono::DateTime::<chrono::Utc>::from(updated_at),
    }
}

fn map_run_row(row: &Row) -> Result<Run, PostgresError> {
    let status_text: String = row.get("status");
    let created_at: std::time::SystemTime = row.get("created_at");
    let updated_at: std::time::SystemTime = row.get("updated_at");
    let started_at: Option<std::time::SystemTime> = row.get("started_at");
    let finished_at: Option<std::time::SystemTime> = row.get("finished_at");
    Ok(Run {
        id: row.get("id"),
        project_id: row.get("project_id"),
        name: row.get("name"),
        description: row.get("description"),
        status: parse_run_status(status_text.as_str())?,
        exit_code: row.get("exit_code"),
        error_message: row.get("error_message"),
        parent_run_id: row.get("parent_run_id"),
        resume_token: row.get("resume_token"),
        tags: row.get("tags"),
        system_info: row.get("system_info"),
        git_info: row.get("git_info"),
        created_at: chrono::DateTime::<chrono::Utc>::from(created_at),
        updated_at: chrono::DateTime::<chrono::Utc>::from(updated_at),
        started_at: started_at.map(chrono::DateTime::<chrono::Utc>::from),
        finished_at: finished_at.map(chrono::DateTime::<chrono::Utc>::from),
        duration_seconds: row.get("duration_seconds"),
    })
}

fn map_parameter_row(row: &Row) -> Parameter {
    let created_at: std::time::SystemTime = row.get("created_at");
    Parameter {
        id: row.get("id"),
        run_id: row.get("run_id"),
        name: row.get("name"),
        value_string: row.get("value_string"),
        value_float: row.get("value_float"),
        value_int: row.get("value_int"),
        value_bool: row.get("value_bool"),
        value_json: row.get("value_json"),
        value_type: row.get("value_type"),
        created_at: chrono::DateTime::<chrono::Utc>::from(created_at),
    }
}

fn map_artifact_row(row: &Row) -> Result<Artifact, PostgresError> {
    let artifact_type_text: String = row.get("artifact_type");
    let created_at: std::time::SystemTime = row.get("created_at");
    Ok(Artifact {
        id: row.get("id"),
        run_id: row.get("run_id"),
        name: row.get("name"),
        artifact_type: parse_artifact_type(artifact_type_text.as_str())?,
        description: row.get("description"),
        storage_path: row.get("storage_path"),
        storage_type: row.get("storage_type"),
        size_bytes: row.get("size_bytes"),
        mime_type: row.get("mime_type"),
        checksum_md5: row.get("checksum_md5"),
        checksum_sha256: row.get("checksum_sha256"),
        metadata: row.get("metadata"),
        created_at: chrono::DateTime::<chrono::Utc>::from(created_at),
    })
}

/// Repository for projects.
pub struct ProjectRepository;

impl ProjectRepository {
    /// Create a new project.
    #[instrument(skip_all)]
    pub async fn create(input: CreateProjectInput) -> Result<Project, PostgresError> {
        let client = connect_client().await?;
        let row = client
            .query_one(
                r"
                INSERT INTO projects (id, name, description, owner_id, settings)
                VALUES (COALESCE($1, uuid_generate_v4()), $2, $3, $4, COALESCE($5, '{}'::jsonb))
                RETURNING id, name, description, owner_id, settings, created_at, updated_at
                ",
                &[
                    &input.id,
                    &input.name,
                    &input.description,
                    &input.owner_id,
                    &input.settings,
                ],
            )
            .await
            .map_err(|e| {
                if let Some(code) = e.code() {
                    if *code == tokio_postgres::error::SqlState::UNIQUE_VIOLATION {
                        return PostgresError::Validation(format!(
                            "Project '{}' already exists",
                            input.name
                        ));
                    }
                }
                PostgresError::Database(format!("Failed to create project: {e}"))
            })?;

        Ok(map_project_row(&row))
    }

    /// Get a project by ID.
    #[instrument]
    pub async fn get_by_id(id: Uuid) -> Result<Project, PostgresError> {
        let client = connect_client().await?;
        let row = client
            .query_opt(
                r"
                SELECT id, name, description, owner_id, settings, created_at, updated_at
                FROM projects
                WHERE id = $1 AND deleted_at IS NULL
                ",
                &[&id],
            )
            .await
            .map_err(|e| PostgresError::Database(format!("Failed to query project by id: {e}")))?;

        row.map(|r| map_project_row(&r))
            .ok_or_else(|| PostgresError::NotFound(format!("Project not found: {id}")))
    }

    /// Get a project by name.
    #[instrument]
    pub async fn get_by_name(name: &str) -> Result<Project, PostgresError> {
        let client = connect_client().await?;
        let row = client
            .query_opt(
                r"
                SELECT id, name, description, owner_id, settings, created_at, updated_at
                FROM projects
                WHERE name = $1 AND deleted_at IS NULL
                ",
                &[&name],
            )
            .await
            .map_err(|e| {
                PostgresError::Database(format!("Failed to query project by name: {e}"))
            })?;

        row.map(|r| map_project_row(&r))
            .ok_or_else(|| PostgresError::NotFound(format!("Project not found: {name}")))
    }

    /// Get or create a project by name.
    #[instrument]
    pub async fn get_or_create(name: &str) -> Result<Project, PostgresError> {
        // Try to get existing
        match Self::get_by_name(name).await {
            Ok(project) => Ok(project),
            Err(PostgresError::NotFound(_)) => {
                // Create new
                Self::create(CreateProjectInput {
                    id: None,
                    name: name.to_string(),
                    description: None,
                    owner_id: None,
                    settings: None,
                })
                .await
            }
            Err(e) => Err(e),
        }
    }
}

/// Repository for runs.
pub struct RunRepository;

impl RunRepository {
    /// Create a new run.
    #[instrument(skip_all)]
    pub async fn create(input: CreateRunInput) -> Result<Run, PostgresError> {
        let client = connect_client().await?;
        let status = RunStatus::Running.to_string();
        let row = client
            .query_one(
                r"
                INSERT INTO runs
                    (id, project_id, name, description, status, parent_run_id, tags, system_info, git_info)
                VALUES
                    (COALESCE($1, uuid_generate_v4()), $2, $3, $4, $5::run_status, $6, COALESCE($7, '{}'::jsonb), COALESCE($8, '{}'::jsonb), COALESCE($9, '{}'::jsonb))
                RETURNING
                    id, project_id, name, description, status::text AS status, exit_code, error_message,
                    parent_run_id, resume_token, tags, system_info, git_info,
                    created_at, updated_at, started_at, finished_at, duration_seconds
                ",
                &[
                    &input.id,
                    &input.project_id,
                    &input.name,
                    &input.description,
                    &status,
                    &input.parent_run_id,
                    &input.tags,
                    &input.system_info,
                    &input.git_info,
                ],
            )
            .await
            .map_err(|e| {
                if let Some(code) = e.code() {
                    if *code == tokio_postgres::error::SqlState::FOREIGN_KEY_VIOLATION {
                        return PostgresError::NotFound(format!(
                            "Project not found for run create: {}",
                            input.project_id
                        ));
                    }
                }
                PostgresError::Database(format!("Failed to create run: {e}"))
            })?;

        map_run_row(&row)
    }

    /// Get a run by ID.
    #[instrument]
    pub async fn get_by_id(id: Uuid) -> Result<Run, PostgresError> {
        let client = connect_client().await?;
        let row = client
            .query_opt(
                r"
                SELECT
                    id, project_id, name, description, status::text AS status, exit_code, error_message,
                    parent_run_id, resume_token, tags, system_info, git_info,
                    created_at, updated_at, started_at, finished_at, duration_seconds
                FROM runs
                WHERE id = $1 AND deleted_at IS NULL
                ",
                &[&id],
            )
            .await
            .map_err(|e| PostgresError::Database(format!("Failed to query run by id: {e}")))?;

        row.ok_or_else(|| PostgresError::NotFound(format!("Run not found: {id}")))
            .and_then(|r| map_run_row(&r))
    }

    /// List runs with filters.
    #[instrument(skip_all)]
    pub async fn list(filter: ListRunsFilter) -> Result<Vec<Run>, PostgresError> {
        let client = connect_client().await?;
        let mut sql = String::from(
            r"
            SELECT
                id, project_id, name, description, status::text AS status, exit_code, error_message,
                parent_run_id, resume_token, tags, system_info, git_info,
                created_at, updated_at, started_at, finished_at, duration_seconds
            FROM runs
            WHERE deleted_at IS NULL
            ",
        );
        let mut params: Vec<Box<dyn ToSql + Sync>> = Vec::new();

        if let Some(project_id) = filter.project_id {
            write!(&mut sql, " AND project_id = ${}", params.len() + 1)
                .expect("writing to a String should not fail");
            params.push(Box::new(project_id));
        }
        if let Some(status) = filter.status {
            write!(&mut sql, " AND status = ${}::run_status", params.len() + 1)
                .expect("writing to a String should not fail");
            params.push(Box::new(status.to_string()));
        }
        if let Some(parent_run_id) = filter.parent_run_id {
            write!(&mut sql, " AND parent_run_id = ${}", params.len() + 1)
                .expect("writing to a String should not fail");
            params.push(Box::new(parent_run_id));
        }
        if let Some(tags) = filter.tags {
            write!(&mut sql, " AND tags @> ${}::jsonb", params.len() + 1)
                .expect("writing to a String should not fail");
            params.push(Box::new(tags));
        }

        sql.push_str(" ORDER BY created_at DESC");

        if let Some(limit) = filter.limit {
            write!(&mut sql, " LIMIT ${}", params.len() + 1)
                .expect("writing to a String should not fail");
            params.push(Box::new(limit));
        }
        if let Some(offset) = filter.offset {
            write!(&mut sql, " OFFSET ${}", params.len() + 1)
                .expect("writing to a String should not fail");
            params.push(Box::new(offset));
        }

        let params_refs: Vec<&(dyn ToSql + Sync)> =
            params.iter().map(std::convert::AsRef::as_ref).collect();
        let rows = client
            .query(sql.as_str(), params_refs.as_slice())
            .await
            .map_err(|e| PostgresError::Database(format!("Failed to list runs: {e}")))?;

        rows.iter().map(map_run_row).collect()
    }

    /// Update run status.
    #[instrument]
    pub async fn update_status(
        id: Uuid,
        status: RunStatus,
        error_message: Option<String>,
    ) -> Result<Run, PostgresError> {
        let client = connect_client().await?;
        let status_text = status.to_string();
        let row = client
            .query_opt(
                r"
                UPDATE runs
                SET status = $2::run_status, error_message = $3
                WHERE id = $1 AND deleted_at IS NULL
                RETURNING
                    id, project_id, name, description, status::text AS status, exit_code, error_message,
                    parent_run_id, resume_token, tags, system_info, git_info,
                    created_at, updated_at, started_at, finished_at, duration_seconds
                ",
                &[&id, &status_text, &error_message],
            )
            .await
            .map_err(|e| PostgresError::Database(format!("Failed to update run status: {e}")))?;

        row.ok_or_else(|| PostgresError::NotFound(format!("Run not found: {id}")))
            .and_then(|r| map_run_row(&r))
    }

    /// Update run tags.
    #[instrument]
    pub async fn update_tags(id: Uuid, tags: serde_json::Value) -> Result<Run, PostgresError> {
        let client = connect_client().await?;
        let row = client
            .query_opt(
                r"
                UPDATE runs
                SET tags = $2
                WHERE id = $1 AND deleted_at IS NULL
                RETURNING
                    id, project_id, name, description, status::text AS status, exit_code, error_message,
                    parent_run_id, resume_token, tags, system_info, git_info,
                    created_at, updated_at, started_at, finished_at, duration_seconds
                ",
                &[&id, &tags],
            )
            .await
            .map_err(|e| PostgresError::Database(format!("Failed to update run tags: {e}")))?;

        row.ok_or_else(|| PostgresError::NotFound(format!("Run not found: {id}")))
            .and_then(|r| map_run_row(&r))
    }
}

/// Repository for parameters.
pub struct ParameterRepository;

impl ParameterRepository {
    /// Create or update parameters.
    #[instrument(skip_all)]
    pub async fn upsert_batch(inputs: Vec<CreateParameterInput>) -> Result<usize, PostgresError> {
        if inputs.is_empty() {
            return Ok(0);
        }

        let mut client = connect_client().await?;
        let transaction = client
            .transaction()
            .await
            .map_err(|e| PostgresError::Database(format!("Failed to start transaction: {e}")))?;

        let statement = transaction
            .prepare(
                "INSERT INTO parameters
                    (run_id, name, value_string, value_float, value_int, value_bool, value_json, value_type)
                 VALUES
                    ($1, $2, $3, $4, $5, $6, $7, $8)
                 ON CONFLICT (run_id, name) DO UPDATE SET
                    value_string = EXCLUDED.value_string,
                    value_float = EXCLUDED.value_float,
                    value_int = EXCLUDED.value_int,
                    value_bool = EXCLUDED.value_bool,
                    value_json = EXCLUDED.value_json,
                    value_type = EXCLUDED.value_type",
            )
            .await
            .map_err(|e| PostgresError::Database(format!("Failed to prepare parameter upsert statement: {e}")))?;

        let mut affected = 0usize;
        for input in inputs {
            let (value_string, value_float, value_int, value_bool, value_json, value_type) =
                match input.value {
                    ParameterValue::String(value) => {
                        (Some(value), None, None, None, None, "string".to_string())
                    }
                    ParameterValue::Float(value) => {
                        (None, Some(value), None, None, None, "float".to_string())
                    }
                    ParameterValue::Int(value) => {
                        (None, None, Some(value), None, None, "int".to_string())
                    }
                    ParameterValue::Bool(value) => {
                        (None, None, None, Some(value), None, "bool".to_string())
                    }
                    ParameterValue::Json(value) => {
                        (None, None, None, None, Some(value), "json".to_string())
                    }
                };

            transaction
                .execute(
                    &statement,
                    &[
                        &input.run_id,
                        &input.name,
                        &value_string,
                        &value_float,
                        &value_int,
                        &value_bool,
                        &value_json,
                        &value_type,
                    ],
                )
                .await
                .map_err(|e| {
                    if let Some(code) = e.code() {
                        if *code == tokio_postgres::error::SqlState::FOREIGN_KEY_VIOLATION {
                            return PostgresError::NotFound(format!(
                                "Run not found in PostgreSQL for parameter upsert: {}",
                                input.run_id
                            ));
                        }
                    }
                    PostgresError::Database(format!(
                        "Failed to upsert parameter '{}' for run '{}': {e}",
                        input.name, input.run_id
                    ))
                })?;
            affected += 1;
        }

        transaction.commit().await.map_err(|e| {
            PostgresError::Database(format!(
                "Failed to commit parameter upsert transaction: {e}"
            ))
        })?;

        Ok(affected)
    }

    /// Get parameters for a run.
    #[instrument]
    pub async fn get_for_run(run_id: Uuid) -> Result<Vec<Parameter>, PostgresError> {
        let client = connect_client().await?;
        let rows = client
            .query(
                r"
                SELECT id, run_id, name, value_string, value_float, value_int, value_bool, value_json, value_type, created_at
                FROM parameters
                WHERE run_id = $1
                ORDER BY name ASC
                ",
                &[&run_id],
            )
            .await
            .map_err(|e| {
                PostgresError::Database(format!("Failed to query parameters for run: {e}"))
            })?;

        Ok(rows.iter().map(map_parameter_row).collect())
    }
}

/// Repository for artifacts.
pub struct ArtifactRepository;

impl ArtifactRepository {
    /// Create a new artifact.
    #[instrument(skip_all)]
    pub async fn create(input: CreateArtifactInput) -> Result<Artifact, PostgresError> {
        let client = connect_client().await?;
        let artifact_type = input.artifact_type.to_string();
        let row = client
            .query_one(
                r"
                INSERT INTO artifacts
                    (run_id, name, type, description, storage_path, storage_type, size_bytes, mime_type, checksum_md5, checksum_sha256, metadata)
                VALUES
                    ($1, $2, $3::artifact_type, $4, $5, COALESCE($6, 'minio'), $7, $8, $9, $10, COALESCE($11, '{}'::jsonb))
                RETURNING
                    id, run_id, name, type::text AS artifact_type, description, storage_path, storage_type,
                    size_bytes, mime_type, checksum_md5, checksum_sha256, metadata, created_at
                ",
                &[
                    &input.run_id,
                    &input.name,
                    &artifact_type,
                    &input.description,
                    &input.storage_path,
                    &input.storage_type,
                    &input.size_bytes,
                    &input.mime_type,
                    &input.checksum_md5,
                    &input.checksum_sha256,
                    &input.metadata,
                ],
            )
            .await
            .map_err(|e| {
                if let Some(code) = e.code() {
                    if *code == tokio_postgres::error::SqlState::FOREIGN_KEY_VIOLATION {
                        return PostgresError::NotFound(format!(
                            "Run not found for artifact create: {}",
                            input.run_id
                        ));
                    }
                    if *code == tokio_postgres::error::SqlState::UNIQUE_VIOLATION {
                        return PostgresError::Validation(format!(
                            "Artifact '{}' already exists for run '{}'",
                            input.name, input.run_id
                        ));
                    }
                }
                PostgresError::Database(format!("Failed to create artifact: {e}"))
            })?;

        map_artifact_row(&row)
    }

    /// Get an artifact by ID.
    #[instrument]
    pub async fn get_by_id(id: Uuid) -> Result<Artifact, PostgresError> {
        let client = connect_client().await?;
        let row = client
            .query_opt(
                r"
                SELECT
                    id, run_id, name, type::text AS artifact_type, description, storage_path, storage_type,
                    size_bytes, mime_type, checksum_md5, checksum_sha256, metadata, created_at
                FROM artifacts
                WHERE id = $1
                ",
                &[&id],
            )
            .await
            .map_err(|e| PostgresError::Database(format!("Failed to query artifact by id: {e}")))?;

        row.ok_or_else(|| PostgresError::NotFound(format!("Artifact not found: {id}")))
            .and_then(|r| map_artifact_row(&r))
    }

    /// Get artifacts for a run.
    #[instrument]
    pub async fn get_for_run(run_id: Uuid) -> Result<Vec<Artifact>, PostgresError> {
        let client = connect_client().await?;
        let rows = client
            .query(
                r"
                SELECT
                    id, run_id, name, type::text AS artifact_type, description, storage_path, storage_type,
                    size_bytes, mime_type, checksum_md5, checksum_sha256, metadata, created_at
                FROM artifacts
                WHERE run_id = $1
                ORDER BY created_at DESC
                ",
                &[&run_id],
            )
            .await
            .map_err(|e| {
                PostgresError::Database(format!("Failed to query artifacts for run: {e}"))
            })?;

        rows.iter().map(map_artifact_row).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = PostgresConfig::default();
        assert!(config.url.contains("mlrunx"));
        assert_eq!(config.max_connections, 10);
    }

    #[test]
    fn test_run_status_display() {
        assert_eq!(RunStatus::Running.to_string(), "running");
        assert_eq!(RunStatus::Finished.to_string(), "finished");
    }

    #[test]
    fn test_artifact_type_display() {
        assert_eq!(ArtifactType::Model.to_string(), "model");
        assert_eq!(ArtifactType::Dataset.to_string(), "dataset");
    }

    #[test]
    fn test_parameter_value_as_string() {
        let param = Parameter {
            id: Uuid::now_v7(),
            run_id: Uuid::now_v7(),
            name: "learning_rate".to_string(),
            value_string: None,
            value_float: Some(0.001),
            value_int: None,
            value_bool: None,
            value_json: None,
            value_type: "float".to_string(),
            created_at: chrono::Utc::now(),
        };

        assert_eq!(param.value_as_string(), "0.001");
    }

    #[test]
    fn test_parse_run_status_values() {
        assert_eq!(
            parse_run_status("running").expect("running should parse"),
            RunStatus::Running
        );
        assert!(parse_run_status("unknown-status").is_err());
    }

    #[test]
    fn test_parse_artifact_type_values() {
        assert_eq!(
            parse_artifact_type("model").expect("model should parse"),
            ArtifactType::Model
        );
        assert!(parse_artifact_type("unknown-type").is_err());
    }
}
