//! Configuration for `MLRunX` API server.
//!
//! Supports configuration via environment variables.

use std::net::SocketAddr;
use tracing::info;

/// Runtime mode determines the storage/control-plane topology.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RuntimeMode {
    /// Standalone mode keeps SQLite in the hot path and is the default local setup.
    #[default]
    Standalone,
    /// Scale-out mode enables queue-driven ingest and external data stores.
    ScaleOut,
}

impl RuntimeMode {
    pub fn from_env() -> Self {
        std::env::var("MLRUNX_RUNTIME_MODE")
            .ok()
            .and_then(|value| match value.trim().to_ascii_lowercase().as_str() {
                "standalone" => Some(Self::Standalone),
                "scaleout" | "scale-out" | "scale_out" => Some(Self::ScaleOut),
                _ => None,
            })
            .unwrap_or_default()
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Standalone => "standalone",
            Self::ScaleOut => "scaleout",
        }
    }

    pub const fn is_scale_out(self) -> bool {
        matches!(self, Self::ScaleOut)
    }
}

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
    /// Runtime mode (standalone or scale-out)
    pub runtime_mode: RuntimeMode,
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
            runtime_mode: RuntimeMode::Standalone,
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
            runtime_mode: RuntimeMode::from_env(),
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
            "  Runtime Mode: {} ({})",
            self.runtime_mode.as_str(),
            match self.runtime_mode {
                RuntimeMode::Standalone => "SQLite-first local mode",
                RuntimeMode::ScaleOut => "queue + external stores enabled",
            }
        );
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
        assert_eq!(config.runtime_mode, RuntimeMode::Standalone);
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

    #[test]
    fn test_runtime_mode_parsing() {
        assert_eq!(RuntimeMode::Standalone.as_str(), "standalone");
        assert_eq!(RuntimeMode::ScaleOut.as_str(), "scaleout");
        assert_eq!(RuntimeMode::default(), RuntimeMode::Standalone);
    }
}
