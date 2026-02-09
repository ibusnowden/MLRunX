//! Storage layer implementations for MLRunX.
//!
//! This module provides storage backends for metrics, metadata, and artifacts.

pub mod clickhouse;
pub mod minio;
pub mod postgres;
pub mod sqlite;

pub use clickhouse::{ClickHouseClient, MetricsRepository};
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
    ProjectMembershipRow, RunRow, SqliteConfig, SqliteError, SqliteStore,
};
