"""Background worker for flushing events to the server.

The worker runs in a daemon thread and periodically flushes
batched events to the MLRunX server with adaptive batching,
optional compression, and offline spooling.
"""

from __future__ import annotations

import gzip
import json
import logging
import threading
import time
from typing import TYPE_CHECKING, Any

from mlrunx.batching import AdaptiveBatcher, BatchConfig, BatchStats, FlushMetrics
from mlrunx.queue import Event, EventQueue, EventType
from mlrunx.spool import DiskSpool, SpoolConfig, SpoolSyncer
from mlrunx.transport.base import Transport, TransportError

if TYPE_CHECKING:
    from mlrunx.config import Config

logger = logging.getLogger(__name__)


class ConnectionState:
    """Tracks connection state and offline/online transitions."""

    def __init__(self) -> None:
        self._online = True
        self._last_success_time = time.time()
        self._last_failure_time = 0.0
        self._consecutive_failures = 0
        self._lock = threading.Lock()

    @property
    def is_online(self) -> bool:
        with self._lock:
            return self._online

    def record_success(self) -> None:
        """Record a successful operation."""
        with self._lock:
            was_offline = not self._online
            self._online = True
            self._last_success_time = time.time()
            self._consecutive_failures = 0

            if was_offline:
                logger.info("Connection restored - switching to online mode")

    def record_failure(self) -> None:
        """Record a failed operation."""
        with self._lock:
            self._last_failure_time = time.time()
            self._consecutive_failures += 1

            # Switch to offline after 3 consecutive failures
            if self._consecutive_failures >= 3 and self._online:
                self._online = False
                logger.warning("Connection lost - switching to offline mode")

    def set_offline(self) -> None:
        """Force connection state to offline mode."""
        with self._lock:
            self._online = False
            self._last_failure_time = time.time()
            if self._consecutive_failures < 3:
                self._consecutive_failures = 3

    @property
    def consecutive_failures(self) -> int:
        with self._lock:
            return self._consecutive_failures

    def to_dict(self) -> dict[str, Any]:
        with self._lock:
            return {
                "online": self._online,
                "last_success_time": self._last_success_time,
                "last_failure_time": self._last_failure_time,
                "consecutive_failures": self._consecutive_failures,
            }


class FlushWorker:
    """Background worker that flushes events to the server.

    Runs in a daemon thread with adaptive batching:
    - Flush triggers: max items, max bytes, max time
    - Metric coalescing: merge same (name, step)
    - Param/tag deduplication: keep last value
    - Optional gzip compression
    - Offline spooling with automatic sync

    Handles retries with exponential backoff on failures.
    Falls back to disk spool when server is unavailable.
    """

    def __init__(
        self,
        queue: EventQueue,
        transport: Transport,
        config: Config,
    ) -> None:
        """Initialize the flush worker.

        Args:
            queue: The event queue to drain
            transport: The transport to use for sending
            config: SDK configuration
        """
        self._queue = queue
        self._transport = transport
        self._config = config
        self._thread: threading.Thread | None = None
        self._stop_event = threading.Event()
        self._flush_event = threading.Event()
        self._lock = threading.Lock()

        # Initialize adaptive batcher
        batch_config = BatchConfig(
            max_items=config.batch_size,
            max_bytes=config.batch_max_bytes,
            max_age_ms=config.batch_timeout_ms,
            coalesce_metrics=config.coalesce_metrics,
            dedupe_params=config.dedupe_params,
            dedupe_tags=config.dedupe_tags,
        )
        self._batcher = AdaptiveBatcher(batch_config)

        # Connection state
        self._connection = ConnectionState()
        if config.offline_mode:
            self._connection.set_offline()
            logger.info("MLRUNX_OFFLINE enabled: worker starts in offline mode")

        # Initialize spool if enabled
        self._spool: DiskSpool | None = None
        self._syncer: SpoolSyncer | None = None

        if config.spool_enabled:
            spool_config = SpoolConfig(
                spool_dir=config.spool_dir,
                max_file_size_bytes=config.spool_max_file_size_bytes,
                max_total_size_bytes=config.spool_max_size_bytes,
                sync_interval_ms=config.spool_sync_interval_ms,
                retention_hours=config.spool_retention_hours,
            )
            self._spool = DiskSpool(spool_config)
            self._syncer = SpoolSyncer(
                spool=self._spool,
                send_func=self._send_spooled_events,
                check_online_func=lambda: self._connection.is_online,
            )

        # Metrics
        self._metrics = FlushMetrics()
        self._batch_count = 0
        self._error_count = 0
        self._spool_count = 0

    def start(self) -> None:
        """Start the background worker thread."""
        with self._lock:
            if self._thread is not None and self._thread.is_alive():
                return

            self._stop_event.clear()
            self._thread = threading.Thread(
                target=self._run,
                name="mlrunx-flush-worker",
                daemon=True,
            )
            self._thread.start()

            # Start syncer if enabled
            if self._syncer is not None:
                self._syncer.start()

            logger.debug("Flush worker started")

    def stop(self, timeout: float = 5.0) -> None:
        """Stop the worker and wait for it to finish.

        Args:
            timeout: Maximum time to wait for the worker to stop
        """
        with self._lock:
            if self._thread is None or not self._thread.is_alive():
                return

            self._stop_event.set()
            self._flush_event.set()  # Wake up the worker

        # Stop syncer first
        if self._syncer is not None:
            self._syncer.stop(timeout=timeout / 2)

        self._thread.join(timeout=timeout)
        if self._thread.is_alive():
            logger.warning("Flush worker did not stop cleanly")

        # Flush spool to disk
        if self._spool is not None:
            self._spool.flush_all()

    def flush(self) -> None:
        """Trigger an immediate flush."""
        self._flush_event.set()

    def set_offline(self) -> None:
        """Force worker into offline mode (events will be spooled if enabled)."""
        self._connection.set_offline()

    def _run(self) -> None:
        """Main worker loop."""
        logger.debug("Flush worker running")

        while not self._stop_event.is_set():
            try:
                # Check for flush trigger from batcher
                check_interval = min(
                    self._config.batch_timeout_ms / 1000.0,
                    0.1,  # Check at least every 100ms
                )
                self._flush_event.wait(timeout=check_interval)
                self._flush_event.clear()

                # Drain events from queue into batcher until empty to avoid idle gaps.
                while True:
                    events = self._queue.get_batch(
                        max_items=self._config.batch_size,
                        timeout_ms=50,  # Short timeout
                    )

                    if not events:
                        break

                    for event in events:
                        should_flush = self._batcher.add(event)
                        if should_flush:
                            self._do_flush(trigger=self._get_trigger())

                    if self._queue.is_empty():
                        break

                # Check if time trigger should fire
                if not self._batcher.is_empty() and self._batcher.should_flush():
                    self._do_flush(trigger=self._get_trigger())

            except Exception as e:
                logger.exception(f"Error in flush worker: {e}")
                self._error_count += 1
                time.sleep(1)  # Back off on errors

        # Final drain on shutdown
        self._drain_remaining()
        logger.debug("Flush worker stopped")

    def _get_trigger(self) -> str:
        """Determine which trigger caused the flush."""
        stats = self._batcher.stats
        config = self._batcher.config

        if stats.event_count >= config.max_items:
            return "size"
        if stats.estimated_bytes >= config.max_bytes:
            return "bytes"
        if stats.age_ms >= config.max_age_ms:
            return "time"
        return "manual"

    def _drain_remaining(self) -> None:
        """Drain and send all remaining events on shutdown."""
        # First, drain queue into batcher
        events = self._queue.drain()
        for event in events:
            self._batcher.add(event)

        # Then flush the batcher
        if not self._batcher.is_empty():
            logger.debug(f"Draining {self._batcher.stats.event_count} remaining events")
            self._do_flush(trigger="shutdown")

    def _do_flush(self, trigger: str) -> None:
        """Flush the batcher and send to server."""
        events, stats = self._batcher.flush()
        if not events:
            return

        start_time = time.perf_counter()
        success = self._send_batch(events, stats)
        duration_ms = (time.perf_counter() - start_time) * 1000

        # If send failed and spool is enabled, spool to disk
        if not success and self._spool is not None:
            self._spool_events(events)

        # Record metrics
        self._metrics.record_flush(
            events=len(events),
            bytes_est=stats.estimated_bytes,
            coalesced=stats.coalesced_count,
            duration_ms=duration_ms,
            trigger=trigger,
        )

        if stats.coalesced_count > 0:
            logger.debug(f"Coalesced {stats.coalesced_count} events")

    def _send_batch(self, events: list[Event], stats: BatchStats) -> bool:
        """Send a batch of events to the server.

        Args:
            events: List of events to send
            stats: Batch statistics

        Returns:
            True if send succeeded, False otherwise
        """
        if not events:
            return True

        # If we're offline, don't try sending.
        if not self._connection.is_online:
            return False

        # Group events by type
        metrics = []
        params = []
        tags = []
        events_data = []

        for event in events:
            if event.type == EventType.METRIC:
                metrics.append(event.data)
            elif event.type == EventType.PARAM:
                params.append(event.data)
            elif event.type == EventType.TAG:
                tags.append(event.data)
            elif event.type == EventType.EVENT:
                events_data.append(event.data)

        # Build batch payload
        batch: dict[str, Any] = {
            "run_id": events[0].run_id,
            "metrics": metrics,
            "params": params,
            "tags": tags,
            "events": events_data,
            "timestamp": time.time(),
            "stats": {
                "metric_count": stats.metric_count,
                "param_count": stats.param_count,
                "tag_count": stats.tag_count,
                "coalesced_count": stats.coalesced_count,
            },
        }

        # Apply compression if enabled and payload is large enough
        payload = json.dumps(batch).encode("utf-8")
        compressed = False

        if (
            self._config.compression_enabled
            and len(payload) >= self._config.compression_min_bytes
        ):
            payload = gzip.compress(payload, compresslevel=self._config.compression_level)
            compressed = True
            logger.debug(
                f"Compressed batch: {stats.estimated_bytes} -> {len(payload)} bytes"
            )

        # Send with retries and exponential backoff
        retries = 0
        delay = self._config.retry_delay_ms / 1000.0
        max_delay = self._config.retry_max_delay_ms / 1000.0

        while retries <= self._config.max_retries:
            try:
                # Pass compression info to transport
                self._transport.send_batch(
                    batch,
                    compressed=compressed,
                    raw_payload=payload if compressed else None,
                )
                self._batch_count += 1
                self._connection.record_success()
                logger.debug(
                    f"Sent batch: {len(metrics)} metrics, "
                    f"{len(params)} params, {len(tags)} tags, {len(events_data)} events"
                )
                return True

            except TransportError as e:
                self._connection.record_failure()

                if not e.retryable or retries >= self._config.max_retries:
                    logger.error(f"Failed to send batch: {e}")
                    self._error_count += 1
                    return False

                logger.warning(f"Retrying batch send ({retries + 1}): {e}")
                retries += 1
                time.sleep(delay)
                delay = min(delay * self._config.retry_backoff, max_delay)

        return False

    def _spool_events(self, events: list[Event]) -> None:
        """Spool events to disk when offline.

        Args:
            events: Events to spool
        """
        if self._spool is None:
            return

        spooled = 0
        for event in events:
            if self._spool.spool(event):
                spooled += 1

        if spooled > 0:
            self._spool_count += spooled
            logger.info(f"Spooled {spooled} events to disk (offline mode)")

        # Flush spool file
        self._spool.flush_all()

    def _send_spooled_events(self, events: list[Event]) -> bool:
        """Send events from spool (called by syncer).

        Args:
            events: Events to send

        Returns:
            True if send succeeded
        """
        if not events:
            return True

        # Create stats for the batch
        stats = BatchStats(
            event_count=len(events),
            metric_count=sum(1 for e in events if e.type == EventType.METRIC),
            param_count=sum(1 for e in events if e.type == EventType.PARAM),
            tag_count=sum(1 for e in events if e.type == EventType.TAG),
        )

        return self._send_batch(events, stats)

    @property
    def batch_count(self) -> int:
        """Number of batches successfully sent."""
        return self._batch_count

    @property
    def error_count(self) -> int:
        """Number of errors encountered."""
        return self._error_count

    @property
    def spool_count(self) -> int:
        """Number of events spooled to disk."""
        return self._spool_count

    @property
    def is_running(self) -> bool:
        """Whether the worker is currently running."""
        return self._thread is not None and self._thread.is_alive()

    @property
    def is_online(self) -> bool:
        """Whether we're currently online."""
        return self._connection.is_online

    @property
    def metrics(self) -> FlushMetrics:
        """Flush metrics for monitoring."""
        return self._metrics

    def get_stats(self) -> dict[str, Any]:
        """Get worker statistics.

        Returns:
            Dictionary with worker stats
        """
        stats: dict[str, Any] = {
            "batch_count": self._batch_count,
            "error_count": self._error_count,
            "spool_count": self._spool_count,
            "queue_size": self._queue.size,
            "queue_dropped": self._queue.dropped_count,
            "batcher": {
                "pending_events": self._batcher.stats.event_count,
                "pending_bytes": self._batcher.stats.estimated_bytes,
            },
            "connection": self._connection.to_dict(),
            "flush_metrics": self._metrics.to_dict(),
        }

        if self._spool is not None:
            stats["spool"] = {
                "pending_files": self._spool.stats.pending_files,
                "pending_events": self._spool.stats.pending_events,
                "pending_bytes": self._spool.stats.pending_bytes,
                "total_synced": self._spool.stats.total_synced,
            }

        return stats

    def trigger_sync(self) -> None:
        """Manually trigger a sync attempt for spooled data."""
        if self._syncer is not None:
            self._syncer.trigger_sync()
