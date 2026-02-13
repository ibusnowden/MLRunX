//! Storage layer implementations for MLRunX.
//!
//! This module provides storage backends for metrics, metadata, and artifacts.

#[cfg(feature = "experimental-clickhouse")]
pub mod clickhouse;
#[cfg(feature = "experimental-minio")]
pub mod minio;
pub mod postgres;
pub mod sqlite;

#[cfg(feature = "experimental-clickhouse")]
pub use clickhouse::{ClickHouseClient, MetricsRepository};
#[cfg(feature = "experimental-minio")]
pub use minio::{
    ArtifactLocation, ArtifactStore, MinioClient, MinioConfig, MinioError, PresignedUrl,
};
pub use postgres::{
    Artifact, ArtifactRepository, ArtifactType, CreateArtifactInput, CreateParameterInput,
    CreateProjectInput, CreateRunInput, ListRunsFilter, Parameter, ParameterRepository,
    ParameterValue, PostgresConfig, PostgresError, Project, ProjectRepository, Run, RunRepository,
    RunStatus, RunSummary,
};
pub use sqlite::{
    AggregatedPointRow, ApiKeyRow, AuthSessionRow, MetricRow, MetricSeriesRow,
    ProjectMembershipRow, ProjectRow, RunRow, SqliteConfig, SqliteError, SqliteStore,
};
