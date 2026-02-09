//! PostgreSQL storage implementation for metadata.
//!
//! Provides relational storage for projects, runs, parameters, and artifacts.
//! See: /migrations/postgres/001_metadata_schema.sql for schema.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio_postgres::NoTls;
use tracing::{instrument, warn};
use uuid::Uuid;

/// Errors that can occur in PostgreSQL operations.
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

/// Configuration for PostgreSQL connection.
#[derive(Debug, Clone)]
pub struct PostgresConfig {
    /// Connection URL (e.g., "postgres://user:pass@localhost:5432/mlrunx")
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

/// Run status enum matching PostgreSQL enum.
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

/// Artifact type enum matching PostgreSQL enum.
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
                .map(|v| v.to_string())
                .unwrap_or_default(),
            _ => String::new(),
        }
    }
}

/// An artifact (file/model) produced by a run.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub name: String,
    pub description: Option<String>,
    pub owner_id: Option<Uuid>,
    pub settings: Option<serde_json::Value>,
}

/// Input for creating a new run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRunInput {
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

/// Repository for projects.
pub struct ProjectRepository;

impl ProjectRepository {
    /// Create a new project.
    #[instrument(skip_all)]
    pub async fn create(_input: CreateProjectInput) -> Result<Project, PostgresError> {
        // TODO: Implement with actual database connection
        Err(PostgresError::Config(
            "Database connection not implemented".to_string(),
        ))
    }

    /// Get a project by ID.
    #[instrument]
    pub async fn get_by_id(_id: Uuid) -> Result<Project, PostgresError> {
        Err(PostgresError::Config(
            "Database connection not implemented".to_string(),
        ))
    }

    /// Get a project by name.
    #[instrument]
    pub async fn get_by_name(_name: &str) -> Result<Project, PostgresError> {
        Err(PostgresError::Config(
            "Database connection not implemented".to_string(),
        ))
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
    pub async fn create(_input: CreateRunInput) -> Result<Run, PostgresError> {
        Err(PostgresError::Config(
            "Database connection not implemented".to_string(),
        ))
    }

    /// Get a run by ID.
    #[instrument]
    pub async fn get_by_id(_id: Uuid) -> Result<Run, PostgresError> {
        Err(PostgresError::Config(
            "Database connection not implemented".to_string(),
        ))
    }

    /// List runs with filters.
    #[instrument(skip_all)]
    pub async fn list(_filter: ListRunsFilter) -> Result<Vec<Run>, PostgresError> {
        Err(PostgresError::Config(
            "Database connection not implemented".to_string(),
        ))
    }

    /// Update run status.
    #[instrument]
    pub async fn update_status(
        _id: Uuid,
        _status: RunStatus,
        _error_message: Option<String>,
    ) -> Result<Run, PostgresError> {
        Err(PostgresError::Config(
            "Database connection not implemented".to_string(),
        ))
    }

    /// Update run tags.
    #[instrument]
    pub async fn update_tags(_id: Uuid, _tags: serde_json::Value) -> Result<Run, PostgresError> {
        Err(PostgresError::Config(
            "Database connection not implemented".to_string(),
        ))
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
    pub async fn get_for_run(_run_id: Uuid) -> Result<Vec<Parameter>, PostgresError> {
        Err(PostgresError::Config(
            "Database connection not implemented".to_string(),
        ))
    }
}

/// Repository for artifacts.
pub struct ArtifactRepository;

impl ArtifactRepository {
    /// Create a new artifact.
    #[instrument(skip_all)]
    pub async fn create(_input: CreateArtifactInput) -> Result<Artifact, PostgresError> {
        Err(PostgresError::Config(
            "Database connection not implemented".to_string(),
        ))
    }

    /// Get an artifact by ID.
    #[instrument]
    pub async fn get_by_id(_id: Uuid) -> Result<Artifact, PostgresError> {
        Err(PostgresError::Config(
            "Database connection not implemented".to_string(),
        ))
    }

    /// Get artifacts for a run.
    #[instrument]
    pub async fn get_for_run(_run_id: Uuid) -> Result<Vec<Artifact>, PostgresError> {
        Err(PostgresError::Config(
            "Database connection not implemented".to_string(),
        ))
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
}
