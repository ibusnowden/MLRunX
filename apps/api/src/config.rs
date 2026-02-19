//! Configuration for `MLRunX` API server.
//!
//! Supports configuration via environment variables.

use std::net::SocketAddr;
use tracing::info;

/// Ingest mode determines how data flows through the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IngestMode {
    /// Direct mode: write directly to ClickHouse/Postgres (alpha mode).
    /// No external queue dependencies, simpler setup.
    #[default]
    Direct,
    /// Queued mode: write through Redis/Kafka queue (future).
    /// Better for high throughput and horizontal scaling.
    Queued,
}

impl IngestMode {
    /// Parse from environment variable.
    pub fn from_env() -> Self {
        std::env::var("INGEST_MODE")
            .ok()
            .and_then(|s| match s.to_lowercase().as_str() {
                "direct" => Some(Self::Direct),
                "queued" => Some(Self::Queued),
                _ => None,
            })
            .unwrap_or_default()
    }

    /// Get string representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Queued => "queued",
        }
    }
}

/// Server configuration.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// HTTP server address
    pub http_addr: SocketAddr,
    /// gRPC server address
    pub grpc_addr: SocketAddr,
    /// Ingest mode (direct or queued)
    pub ingest_mode: IngestMode,
    /// Log level
    pub log_level: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            http_addr: "0.0.0.0:3001".parse().unwrap(),
            grpc_addr: "0.0.0.0:50051".parse().unwrap(),
            ingest_mode: IngestMode::Direct,
            log_level: "info,mlrunx_api=debug".to_string(),
        }
    }
}

impl ServerConfig {
    /// Load configuration from environment variables.
    pub fn from_env() -> Self {
        let http_port: u16 = std::env::var("API_HTTP_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3001);

        let grpc_port: u16 = std::env::var("API_GRPC_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(50051);

        let host = std::env::var("API_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());

        Self {
            http_addr: format!("{host}:{http_port}").parse().unwrap(),
            grpc_addr: format!("{host}:{grpc_port}").parse().unwrap(),
            ingest_mode: IngestMode::from_env(),
            log_level: std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "info,mlrunx_api=debug".to_string()),
        }
    }

    /// Log the configuration at startup.
    pub fn log_startup(&self) {
        info!("MLRunX API Configuration:");
        info!("  HTTP Server: {}", self.http_addr);
        info!("  gRPC Server: {}", self.grpc_addr);
        info!(
            "  Ingest Mode: {} ({})",
            self.ingest_mode.as_str(),
            match self.ingest_mode {
                IngestMode::Direct => "writes directly to CH/PG",
                IngestMode::Queued => "writes through queue",
            }
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ServerConfig::default();
        assert_eq!(config.http_addr.port(), 3001);
        assert_eq!(config.grpc_addr.port(), 50051);
        assert_eq!(config.ingest_mode, IngestMode::Direct);
    }

    #[test]
    fn test_ingest_mode_parsing() {
        // Direct mode
        assert_eq!(IngestMode::Direct.as_str(), "direct");
        assert_eq!(IngestMode::Queued.as_str(), "queued");

        // Default is direct
        assert_eq!(IngestMode::default(), IngestMode::Direct);
    }
}
