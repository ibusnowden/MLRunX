"""MLRunX SDK Configuration.

Configuration can be set via:
1. Environment variables (MLRUNX_*)
2. Config file (~/.mlrunx/config.toml)
3. Programmatic initialization
"""

from __future__ import annotations

import os
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any


@dataclass
class Config:
    """SDK configuration settings."""

    # Server connection
    server_url: str = "http://localhost:3001"
    api_key: str | None = None
    project_id: str | None = None  # Preferred project identifier for scoped keys

    # Batching settings (adaptive)
    batch_size: int = 1000  # Max items per batch
    batch_max_bytes: int = 1_000_000  # Max batch size in bytes (1MB)
    batch_timeout_ms: int = 1000  # Max time before flush (milliseconds)
    queue_size: int = 10000  # Max queue size before dropping

    # Coalescing settings
    coalesce_metrics: bool = True  # Merge same metric at same step
    dedupe_params: bool = True  # Keep only last value for params
    dedupe_tags: bool = True  # Keep only last value for tags

    # Compression settings
    compression_enabled: bool = True  # Enable gzip compression
    compression_level: int = 6  # gzip compression level (1-9)
    compression_min_bytes: int = 1000  # Min size before compressing

    # Retry settings
    max_retries: int = 3
    retry_delay_ms: int = 1000
    retry_backoff: float = 2.0
    retry_max_delay_ms: int = 30000  # Max retry delay (30s)

    # Offline mode and spool settings
    offline_mode: bool = False
    spool_enabled: bool = True  # Enable disk spooling when offline
    spool_dir: Path = field(default_factory=lambda: Path.home() / ".mlrunx" / "spool")
    spool_max_size_bytes: int = 100_000_000  # 100MB max spool size
    spool_max_file_size_bytes: int = 10_000_000  # 10MB per spool file
    spool_sync_interval_ms: int = 5000  # Sync check interval (5s)
    spool_retention_hours: int = 72  # Keep completed files for 72h

    # Connection health
    health_check_interval_ms: int = 10000  # Health check interval (10s)
    connection_timeout_ms: int = 5000  # Connection timeout (5s)

    # Debug
    debug: bool = False

    @classmethod
    def from_env(cls) -> Config:
        """Load configuration from environment variables."""
        truthy = ("true", "1", "yes")
        return cls(
            server_url=os.getenv("MLRUNX_SERVER_URL", "http://localhost:3001"),
            api_key=os.getenv("MLRUNX_API_KEY"),
            project_id=os.getenv("MLRUNX_PROJECT_ID"),
            batch_size=int(os.getenv("MLRUNX_BATCH_SIZE", "1000")),
            batch_max_bytes=int(os.getenv("MLRUNX_BATCH_MAX_BYTES", "1000000")),
            batch_timeout_ms=int(os.getenv("MLRUNX_BATCH_TIMEOUT_MS", "1000")),
            queue_size=int(os.getenv("MLRUNX_QUEUE_SIZE", "10000")),
            coalesce_metrics=os.getenv("MLRUNX_COALESCE_METRICS", "true").lower()
            in truthy,
            dedupe_params=os.getenv("MLRUNX_DEDUPE_PARAMS", "true").lower() in truthy,
            dedupe_tags=os.getenv("MLRUNX_DEDUPE_TAGS", "true").lower() in truthy,
            compression_enabled=os.getenv("MLRUNX_COMPRESSION", "true").lower()
            in truthy,
            compression_level=int(os.getenv("MLRUNX_COMPRESSION_LEVEL", "6")),
            compression_min_bytes=int(os.getenv("MLRUNX_COMPRESSION_MIN_BYTES", "1000")),
            max_retries=int(os.getenv("MLRUNX_MAX_RETRIES", "3")),
            retry_delay_ms=int(os.getenv("MLRUNX_RETRY_DELAY_MS", "1000")),
            retry_max_delay_ms=int(os.getenv("MLRUNX_RETRY_MAX_DELAY_MS", "30000")),
            offline_mode=os.getenv("MLRUNX_OFFLINE", "").lower() in truthy,
            spool_enabled=os.getenv("MLRUNX_SPOOL_ENABLED", "true").lower()
            in truthy,
            spool_dir=Path(
                os.getenv("MLRUNX_SPOOL_DIR", str(Path.home() / ".mlrunx" / "spool"))
            ),
            spool_max_size_bytes=int(os.getenv("MLRUNX_SPOOL_MAX_SIZE", "100000000")),
            spool_max_file_size_bytes=int(
                os.getenv("MLRUNX_SPOOL_MAX_FILE_SIZE", "10000000")
            ),
            spool_sync_interval_ms=int(
                os.getenv("MLRUNX_SPOOL_SYNC_INTERVAL_MS", "5000")
            ),
            spool_retention_hours=int(
                os.getenv("MLRUNX_SPOOL_RETENTION_HOURS", "72")
            ),
            health_check_interval_ms=int(
                os.getenv("MLRUNX_HEALTH_CHECK_INTERVAL_MS", "10000")
            ),
            connection_timeout_ms=int(
                os.getenv("MLRUNX_CONNECTION_TIMEOUT_MS", "5000")
            ),
            debug=os.getenv("MLRUNX_DEBUG", "").lower() in truthy,
        )


# Global configuration instance
_config: Config | None = None


def get_config() -> Config:
    """Get the global configuration."""
    global _config
    if _config is None:
        _config = Config.from_env()
    return _config


def configure(**kwargs: Any) -> None:
    """Update global configuration.

    Args:
        **kwargs: Configuration options to update
    """
    global _config
    if _config is None:
        _config = Config.from_env()

    for key, value in kwargs.items():
        if hasattr(_config, key):
            setattr(_config, key, value)
        else:
            raise ValueError(f"Unknown configuration option: {key}")
