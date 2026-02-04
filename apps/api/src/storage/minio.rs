//! MinIO/S3 storage implementation for artifacts.
//!
//! Provides presigned URL generation and artifact management.
//! Compatible with MinIO, AWS S3, and other S3-compatible storage.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::instrument;

/// Errors that can occur in MinIO operations.
#[derive(Error, Debug)]
pub enum MinioError {
    #[error("MinIO client error: {0}")]
    Client(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Object not found: {0}")]
    NotFound(String),

    #[error("Invalid presign request: {0}")]
    InvalidPresign(String),
}

/// Configuration for MinIO/S3 connection.
#[derive(Debug, Clone)]
pub struct MinioConfig {
    /// MinIO/S3 endpoint URL (e.g., "http://localhost:9000")
    pub endpoint: String,
    /// Access key ID
    pub access_key: String,
    /// Secret access key
    pub secret_key: String,
    /// Default bucket for artifacts
    pub bucket: String,
    /// Use path-style URLs (required for MinIO)
    pub path_style: bool,
    /// AWS region (for S3)
    pub region: String,
    /// Presigned URL expiry in seconds
    pub presign_expiry_secs: u64,
}

impl Default for MinioConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:9000".to_string(),
            access_key: "mlrunx".to_string(),
            secret_key: "mlrunx_dev".to_string(),
            bucket: "mlrunx-artifacts".to_string(),
            path_style: true,
            region: "us-east-1".to_string(),
            presign_expiry_secs: 3600, // 1 hour
        }
    }
}

impl MinioConfig {
    /// Create config from environment variables.
    pub fn from_env() -> Self {
        Self {
            endpoint: std::env::var("MINIO_ENDPOINT")
                .unwrap_or_else(|_| "http://localhost:9000".to_string()),
            access_key: std::env::var("MINIO_ACCESS_KEY")
                .or_else(|_| std::env::var("AWS_ACCESS_KEY_ID"))
                .unwrap_or_else(|_| "mlrunx".to_string()),
            secret_key: std::env::var("MINIO_SECRET_KEY")
                .or_else(|_| std::env::var("AWS_SECRET_ACCESS_KEY"))
                .unwrap_or_else(|_| "mlrunx_dev".to_string()),
            bucket: std::env::var("MINIO_BUCKET").unwrap_or_else(|_| "mlrunx-artifacts".to_string()),
            path_style: std::env::var("MINIO_PATH_STYLE")
                .map(|v| v.to_lowercase() == "true")
                .unwrap_or(true),
            region: std::env::var("MINIO_REGION")
                .or_else(|_| std::env::var("AWS_REGION"))
                .unwrap_or_else(|_| "us-east-1".to_string()),
            presign_expiry_secs: std::env::var("MINIO_PRESIGN_EXPIRY")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3600),
        }
    }
}

/// Artifact location in object storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactLocation {
    /// Bucket name
    pub bucket: String,
    /// Object key (path within bucket)
    pub key: String,
    /// Full storage URL
    pub storage_url: String,
}

impl ArtifactLocation {
    /// Create a new artifact location.
    pub fn new(bucket: &str, run_id: &str, artifact_name: &str) -> Self {
        let key = format!("runs/{}/{}", run_id, artifact_name);
        let storage_url = format!("minio://{}/{}", bucket, key);

        Self {
            bucket: bucket.to_string(),
            key,
            storage_url,
        }
    }
}

/// Presigned URL response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresignedUrl {
    /// The presigned URL for upload/download
    pub url: String,
    /// HTTP method to use (PUT for upload, GET for download)
    pub method: String,
    /// Expiry time in seconds
    pub expires_in_secs: u64,
    /// Required headers for the request
    pub headers: std::collections::HashMap<String, String>,
}

/// MinIO/S3 client wrapper.
#[derive(Clone)]
pub struct MinioClient {
    config: MinioConfig,
}

impl MinioClient {
    /// Create a new MinIO client.
    pub fn new(config: MinioConfig) -> Self {
        Self { config }
    }

    /// Get the storage URL for an artifact.
    pub fn get_artifact_location(&self, run_id: &str, artifact_name: &str) -> ArtifactLocation {
        ArtifactLocation::new(&self.config.bucket, run_id, artifact_name)
    }

    /// Generate a presigned URL for uploading an artifact.
    ///
    /// Note: This is a placeholder implementation. In production, use the
    /// aws-sdk-s3 crate or similar to generate proper presigned URLs.
    #[instrument(skip(self))]
    pub fn presign_upload(
        &self,
        run_id: &str,
        artifact_name: &str,
        _content_type: Option<&str>,
        _content_length: Option<u64>,
    ) -> Result<PresignedUrl, MinioError> {
        let location = self.get_artifact_location(run_id, artifact_name);

        // Placeholder: In production, use proper S3 signing
        // This generates a URL that would need the actual presigning logic
        let url = format!(
            "{}/{}/{}",
            self.config.endpoint, location.bucket, location.key
        );

        let mut headers = std::collections::HashMap::new();
        headers.insert("x-amz-acl".to_string(), "private".to_string());

        Ok(PresignedUrl {
            url,
            method: "PUT".to_string(),
            expires_in_secs: self.config.presign_expiry_secs,
            headers,
        })
    }

    /// Generate a presigned URL for downloading an artifact.
    #[instrument(skip(self))]
    pub fn presign_download(
        &self,
        run_id: &str,
        artifact_name: &str,
    ) -> Result<PresignedUrl, MinioError> {
        let location = self.get_artifact_location(run_id, artifact_name);

        // Placeholder: In production, use proper S3 signing
        let url = format!(
            "{}/{}/{}",
            self.config.endpoint, location.bucket, location.key
        );

        Ok(PresignedUrl {
            url,
            method: "GET".to_string(),
            expires_in_secs: self.config.presign_expiry_secs,
            headers: std::collections::HashMap::new(),
        })
    }

    /// Check if an artifact exists.
    #[instrument(skip(self))]
    pub async fn artifact_exists(
        &self,
        _run_id: &str,
        _artifact_name: &str,
    ) -> Result<bool, MinioError> {
        // Placeholder: In production, use HEAD request to check existence
        Ok(false)
    }

    /// Delete an artifact.
    #[instrument(skip(self))]
    pub async fn delete_artifact(
        &self,
        _run_id: &str,
        _artifact_name: &str,
    ) -> Result<(), MinioError> {
        // Placeholder: In production, use DELETE request
        Err(MinioError::Config("Delete not implemented".to_string()))
    }

    /// List artifacts for a run.
    #[instrument(skip(self))]
    pub async fn list_artifacts(&self, _run_id: &str) -> Result<Vec<ArtifactLocation>, MinioError> {
        // Placeholder: In production, use LIST request with prefix
        Ok(vec![])
    }

    /// Ensure the bucket exists, creating it if necessary.
    #[instrument(skip(self))]
    pub async fn ensure_bucket(&self) -> Result<(), MinioError> {
        // Placeholder: In production, check and create bucket
        Ok(())
    }

    /// Get bucket name.
    pub fn bucket(&self) -> &str {
        &self.config.bucket
    }

    /// Get endpoint URL.
    pub fn endpoint(&self) -> &str {
        &self.config.endpoint
    }
}

/// Repository for artifact storage operations.
pub struct ArtifactStore {
    client: MinioClient,
}

impl ArtifactStore {
    /// Create a new artifact store.
    pub fn new(client: MinioClient) -> Self {
        Self { client }
    }

    /// Generate presigned upload URL for a new artifact.
    #[instrument(skip(self))]
    pub fn create_upload_url(
        &self,
        run_id: &str,
        artifact_name: &str,
        content_type: Option<&str>,
        content_length: Option<u64>,
    ) -> Result<(ArtifactLocation, PresignedUrl), MinioError> {
        let location = self.client.get_artifact_location(run_id, artifact_name);
        let presigned =
            self.client
                .presign_upload(run_id, artifact_name, content_type, content_length)?;
        Ok((location, presigned))
    }

    /// Generate presigned download URL for an artifact.
    #[instrument(skip(self))]
    pub fn create_download_url(
        &self,
        run_id: &str,
        artifact_name: &str,
    ) -> Result<PresignedUrl, MinioError> {
        self.client.presign_download(run_id, artifact_name)
    }

    /// Get artifact location info.
    pub fn get_location(&self, run_id: &str, artifact_name: &str) -> ArtifactLocation {
        self.client.get_artifact_location(run_id, artifact_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = MinioConfig::default();
        assert_eq!(config.endpoint, "http://localhost:9000");
        assert_eq!(config.bucket, "mlrunx-artifacts");
        assert!(config.path_style);
    }

    #[test]
    fn test_artifact_location() {
        let location = ArtifactLocation::new("mlrunx-artifacts", "run-123", "model.pt");
        assert_eq!(location.bucket, "mlrunx-artifacts");
        assert_eq!(location.key, "runs/run-123/model.pt");
        assert_eq!(
            location.storage_url,
            "minio://mlrunx-artifacts/runs/run-123/model.pt"
        );
    }

    #[test]
    fn test_presign_upload() {
        let config = MinioConfig::default();
        let client = MinioClient::new(config);

        let result = client.presign_upload(
            "run-123",
            "model.pt",
            Some("application/octet-stream"),
            Some(1024),
        );
        assert!(result.is_ok());

        let presigned = result.unwrap();
        assert_eq!(presigned.method, "PUT");
        assert!(presigned.url.contains("run-123"));
        assert!(presigned.url.contains("model.pt"));
    }

    #[test]
    fn test_presign_download() {
        let config = MinioConfig::default();
        let client = MinioClient::new(config);

        let result = client.presign_download("run-123", "model.pt");
        assert!(result.is_ok());

        let presigned = result.unwrap();
        assert_eq!(presigned.method, "GET");
    }

    #[test]
    fn test_artifact_store() {
        let config = MinioConfig::default();
        let client = MinioClient::new(config);
        let store = ArtifactStore::new(client);

        let result = store.create_upload_url("run-123", "checkpoint.pt", None, None);
        assert!(result.is_ok());

        let (location, presigned) = result.unwrap();
        assert_eq!(location.bucket, "mlrunx-artifacts");
        assert_eq!(presigned.method, "PUT");
    }
}
