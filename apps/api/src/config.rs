//! Configuration for `MLRunX` API server.
//!
//! Supports configuration via environment variables.

use std::net::SocketAddr;
use tracing::info;

const DEFAULT_HTTP_PORT: u16 = 3001;
const DEFAULT_GRPC_PORT: u16 = 50051;
const DEFAULT_API_HOST: &str = "0.0.0.0";
const DEFAULT_RUST_LOG: &str = "info,mlrunx_api=debug";

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

fn parse_optional_value<T>(name: &str, value: Option<&str>) -> Result<Option<T>, String>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .parse::<T>()
                .map_err(|err| format!("invalid {name} value '{value}': {err}"))
        })
        .transpose()
}

fn parse_bind_addr(host: &str, port: u16, label: &str) -> Result<SocketAddr, String> {
    format!("{host}:{port}")
        .parse()
        .map_err(|err| format!("invalid {label} address '{host}:{port}': {err}"))
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            http_addr: SocketAddr::from(([0, 0, 0, 0], DEFAULT_HTTP_PORT)),
            grpc_addr: SocketAddr::from(([0, 0, 0, 0], DEFAULT_GRPC_PORT)),
            runtime_mode: RuntimeMode::Standalone,
            ingest_mode: IngestMode::Direct,
            log_level: DEFAULT_RUST_LOG.to_string(),
        }
    }
}

impl ServerConfig {
    fn build(
        host: Option<String>,
        http_port: Option<u16>,
        grpc_port: Option<u16>,
        runtime_mode: RuntimeMode,
        ingest_mode: IngestMode,
        log_level: Option<String>,
    ) -> Result<Self, String> {
        let resolved_host = host
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(DEFAULT_API_HOST);
        let resolved_http_port = http_port.unwrap_or(DEFAULT_HTTP_PORT);
        let resolved_grpc_port = grpc_port.unwrap_or(DEFAULT_GRPC_PORT);

        Ok(Self {
            http_addr: parse_bind_addr(resolved_host, resolved_http_port, "HTTP bind")?,
            grpc_addr: parse_bind_addr(resolved_host, resolved_grpc_port, "gRPC bind")?,
            runtime_mode,
            ingest_mode,
            log_level: log_level.unwrap_or_else(|| DEFAULT_RUST_LOG.to_string()),
        })
    }

    /// Load configuration from environment variables.
    pub fn from_env() -> Result<Self, String> {
        let host = std::env::var("API_HOST").ok();
        let http_port_raw = std::env::var("API_HTTP_PORT").ok();
        let grpc_port_raw = std::env::var("API_GRPC_PORT").ok();
        let log_level = std::env::var("RUST_LOG").ok();

        Self::build(
            host,
            parse_optional_value("API_HTTP_PORT", http_port_raw.as_deref())?,
            parse_optional_value("API_GRPC_PORT", grpc_port_raw.as_deref())?,
            RuntimeMode::from_env(),
            IngestMode::from_env(),
            log_level,
        )
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
        assert_eq!(config.http_addr.port(), DEFAULT_HTTP_PORT);
        assert_eq!(config.grpc_addr.port(), DEFAULT_GRPC_PORT);
        assert_eq!(config.runtime_mode, RuntimeMode::Standalone);
        assert_eq!(config.ingest_mode, IngestMode::Direct);
    }

    #[test]
    fn test_build_uses_defaults() {
        let config = ServerConfig::build(
            None,
            None,
            None,
            RuntimeMode::Standalone,
            IngestMode::Direct,
            None,
        )
        .expect("expected default config");
        assert_eq!(config.http_addr, SocketAddr::from(([0, 0, 0, 0], DEFAULT_HTTP_PORT)));
        assert_eq!(config.grpc_addr, SocketAddr::from(([0, 0, 0, 0], DEFAULT_GRPC_PORT)));
        assert_eq!(config.log_level, DEFAULT_RUST_LOG);
    }

    #[test]
    fn test_parse_optional_value_rejects_invalid_port() {
        let err = parse_optional_value::<u16>("API_HTTP_PORT", Some("not-a-port"))
            .expect_err("expected invalid API_HTTP_PORT");
        assert!(err.contains("API_HTTP_PORT"));
    }

    #[test]
    fn test_build_rejects_invalid_host() {
        let err = ServerConfig::build(
            Some("bad host name".to_string()),
            Some(DEFAULT_HTTP_PORT),
            None,
            RuntimeMode::Standalone,
            IngestMode::Direct,
            None,
        )
        .expect_err("expected invalid API_HOST");
        assert!(err.contains("HTTP bind"));
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
