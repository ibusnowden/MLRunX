"""Disk spool for offline event persistence.

This module provides durable event storage for offline mode:
- Write-ahead logging for crash safety
- Atomic file operations
- Automatic recovery on startup
- Background sync when connection restored
"""

from __future__ import annotations

import json
import logging
import os
import threading
import time
import uuid
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Literal

from mlrunx.queue import Event, EventType

logger = logging.getLogger(__name__)

# Spool file format version
SPOOL_VERSION = 1

# File extensions
SPOOL_EXT = ".spool"
PENDING_EXT = ".pending"
COMPLETED_EXT = ".done"

SpoolReplayOutcome = Literal["synced", "dropped_terminal", "failed"]


@dataclass(frozen=True)
class SpoolReplaySendResult:
    """Structured outcome for a spool replay send attempt."""

    success: bool
    outcome: SpoolReplayOutcome
    run_id: str | None
    event_count: int
    error_message: str | None = None
    error_status_code: int | None = None
    error_retryable: bool | None = None

    @classmethod
    def synced(cls, run_id: str | None, event_count: int) -> "SpoolReplaySendResult":
        return cls(
            success=True,
            outcome="synced",
            run_id=run_id,
            event_count=event_count,
        )

    @classmethod
    def dropped_terminal(
        cls,
        run_id: str | None,
        event_count: int,
        *,
        error_message: str,
        error_status_code: int | None,
        error_retryable: bool,
    ) -> "SpoolReplaySendResult":
        return cls(
            success=True,
            outcome="dropped_terminal",
            run_id=run_id,
            event_count=event_count,
            error_message=error_message,
            error_status_code=error_status_code,
            error_retryable=error_retryable,
        )

    @classmethod
    def failed(
        cls,
        run_id: str | None,
        event_count: int,
        *,
        error_message: str | None,
        error_status_code: int | None,
        error_retryable: bool | None,
    ) -> "SpoolReplaySendResult":
        return cls(
            success=False,
            outcome="failed",
            run_id=run_id,
            event_count=event_count,
            error_message=error_message,
            error_status_code=error_status_code,
            error_retryable=error_retryable,
        )


def _normalize_spool_replay_send_result(
    result: bool | SpoolReplaySendResult,
    *,
    run_id: str | None,
    event_count: int,
) -> SpoolReplaySendResult:
    if isinstance(result, SpoolReplaySendResult):
        return result
    if result:
        return SpoolReplaySendResult.synced(run_id, event_count)
    return SpoolReplaySendResult.failed(
        run_id,
        event_count,
        error_message=None,
        error_status_code=None,
        error_retryable=None,
    )


@dataclass
class SpoolConfig:
    """Configuration for the disk spool."""

    spool_dir: Path = field(default_factory=lambda: Path.home() / ".mlrunx" / "spool")
    max_file_size_bytes: int = 10_000_000  # 10MB per spool file
    max_total_size_bytes: int = 100_000_000  # 100MB total spool size
    max_files: int = 100  # Max spool files to keep
    sync_interval_ms: int = 5000  # How often to check for sync (5s)
    retention_hours: int = 72  # Keep completed files for 72h


@dataclass
class SpoolStats:
    """Statistics about the spool."""

    pending_files: int = 0
    pending_events: int = 0
    pending_bytes: int = 0
    completed_files: int = 0
    total_synced: int = 0
    last_sync_time: float = 0.0


class SpoolFile:
    """A single spool file containing batched events."""

    def __init__(self, path: Path, run_id: str) -> None:
        """Initialize a spool file.

        Args:
            path: Path to the spool file
            run_id: Run ID this spool belongs to
        """
        self._path = path
        self._run_id = run_id
        self._events: list[dict[str, Any]] = []
        self._size_bytes = 0
        self._created_at = time.time()
        self._lock = threading.Lock()

    @property
    def path(self) -> Path:
        return self._path

    @property
    def run_id(self) -> str:
        return self._run_id

    @property
    def size_bytes(self) -> int:
        return self._size_bytes

    @property
    def event_count(self) -> int:
        return len(self._events)

    def append(self, event: Event) -> None:
        """Append an event to the spool file.

        Args:
            event: Event to append
        """
        with self._lock:
            event_dict = {
                "type": event.type.value,
                "run_id": event.run_id,
                "timestamp": event.timestamp,
                "data": event.data,
            }
            self._events.append(event_dict)
            self._size_bytes += len(json.dumps(event_dict))

    def flush(self) -> None:
        """Flush events to disk atomically."""
        with self._lock:
            if not self._events:
                return

            # Write to temp file first
            temp_path = self._path.with_suffix(PENDING_EXT)
            self._path.parent.mkdir(parents=True, exist_ok=True)

            content = {
                "version": SPOOL_VERSION,
                "run_id": self._run_id,
                "created_at": self._created_at,
                "events": self._events,
            }

            with open(temp_path, "w") as f:
                json.dump(content, f)

            # Atomic rename
            os.replace(temp_path, self._path)
            logger.debug(f"Flushed {len(self._events)} events to {self._path}")

    def read_events(self) -> list[Event]:
        """Read events from the spool file.

        Returns:
            List of events
        """
        if not self._path.exists():
            return []

        with open(self._path) as f:
            content = json.load(f)

        events = []
        for event_dict in content.get("events", []):
            event = Event(
                type=EventType(event_dict["type"]),
                run_id=event_dict["run_id"],
                timestamp=event_dict.get("timestamp", time.time()),
                data=event_dict["data"],
            )
            events.append(event)

        return events

    def mark_completed(self) -> None:
        """Mark the spool file as completed (synced)."""
        if self._path.exists():
            completed_path = self._path.with_suffix(COMPLETED_EXT)
            os.replace(self._path, completed_path)
            logger.debug(f"Marked spool file as completed: {completed_path}")

    def delete(self) -> None:
        """Delete the spool file."""
        if self._path.exists():
            self._path.unlink()


class DiskSpool:
    """Manages disk-based event spooling for offline mode.

    Events are written to spool files when the server is unavailable.
    When connection is restored, events are synced in order.
    """

    def __init__(self, config: SpoolConfig | None = None) -> None:
        """Initialize the disk spool.

        Args:
            config: Spool configuration
        """
        self._config = config or SpoolConfig()
        self._config.spool_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
        self._lock = threading.Lock()
        self._active_files: dict[str, SpoolFile] = {}  # run_id -> SpoolFile
        self._stats = SpoolStats()

    @property
    def config(self) -> SpoolConfig:
        return self._config

    @property
    def stats(self) -> SpoolStats:
        """Get current spool statistics."""
        self._update_stats()
        return self._stats

    def _update_stats(self) -> None:
        """Update statistics from disk."""
        pending_files = list(self._config.spool_dir.glob(f"*{SPOOL_EXT}"))
        completed_files = list(self._config.spool_dir.glob(f"*{COMPLETED_EXT}"))

        self._stats.pending_files = len(pending_files)
        self._stats.completed_files = len(completed_files)

        total_bytes = 0
        total_events = 0
        for f in pending_files:
            total_bytes += f.stat().st_size
            try:
                with open(f) as fp:
                    content = json.load(fp)
                    total_events += len(content.get("events", []))
            except Exception:
                pass

        self._stats.pending_bytes = total_bytes
        self._stats.pending_events = total_events

    def spool(self, event: Event) -> bool:
        """Spool an event to disk.

        Args:
            event: Event to spool

        Returns:
            True if successfully spooled
        """
        with self._lock:
            # Check size limits
            if self._stats.pending_bytes >= self._config.max_total_size_bytes:
                logger.warning("Spool size limit reached, dropping event")
                return False

            # Get or create spool file for this run
            spool_file = self._get_or_create_spool_file(event.run_id)

            # Append event
            spool_file.append(event)

            # Flush if file is getting large
            if spool_file.size_bytes >= self._config.max_file_size_bytes:
                spool_file.flush()
                # Create new file for subsequent events
                del self._active_files[event.run_id]

            return True

    def _get_or_create_spool_file(self, run_id: str) -> SpoolFile:
        """Get or create a spool file for a run."""
        if run_id not in self._active_files:
            filename = f"{run_id}_{int(time.time() * 1000)}_{uuid.uuid4().hex[:8]}{SPOOL_EXT}"
            path = self._config.spool_dir / filename
            self._active_files[run_id] = SpoolFile(path, run_id)

        return self._active_files[run_id]

    def flush_all(self) -> None:
        """Flush all active spool files to disk."""
        with self._lock:
            for spool_file in self._active_files.values():
                spool_file.flush()

    def get_pending_files(self) -> list[Path]:
        """Get list of pending spool files, oldest first.

        Returns:
            List of pending spool file paths
        """
        files = list(self._config.spool_dir.glob(f"*{SPOOL_EXT}"))
        # Sort by creation time (encoded in filename)
        files.sort(key=lambda f: f.stat().st_mtime)
        return files

    def read_spool_file(self, path: Path) -> list[Event]:
        """Read events from a spool file.

        Args:
            path: Path to spool file

        Returns:
            List of events
        """
        spool_file = SpoolFile(path, "")
        spool_file._path = path
        return spool_file.read_events()

    def mark_synced(self, path: Path) -> None:
        """Mark a spool file as synced.

        Args:
            path: Path to spool file
        """
        spool_file = SpoolFile(path, "")
        spool_file._path = path
        spool_file.mark_completed()
        self._stats.total_synced += 1
        self._stats.last_sync_time = time.time()

    def cleanup_old_files(self) -> int:
        """Clean up old completed spool files.

        Returns:
            Number of files cleaned up
        """
        cutoff_time = time.time() - (self._config.retention_hours * 3600)
        cleaned = 0

        for f in self._config.spool_dir.glob(f"*{COMPLETED_EXT}"):
            if f.stat().st_mtime < cutoff_time:
                f.unlink()
                cleaned += 1

        if cleaned > 0:
            logger.info(f"Cleaned up {cleaned} old spool files")

        return cleaned

    def recover(self) -> int:
        """Recover from pending files on startup.

        Returns:
            Number of pending files found
        """
        pending = self.get_pending_files()
        if pending:
            logger.info(f"Found {len(pending)} pending spool files for recovery")
        return len(pending)


class SpoolSyncer:
    """Background syncer that uploads spooled events when online."""

    def __init__(
        self,
        spool: DiskSpool,
        send_func: Any,
        check_online_func: Any,
    ) -> None:
        """Initialize the syncer.

        Args:
            spool: The disk spool to sync from
            send_func: Function to send a batch of events
            check_online_func: Function to check if we're online
        """
        self._spool = spool
        self._send_func = send_func
        self._check_online = check_online_func
        self._thread: threading.Thread | None = None
        self._stop_event = threading.Event()
        self._sync_event = threading.Event()

    def start(self) -> None:
        """Start the background sync thread."""
        if self._thread is not None and self._thread.is_alive():
            return

        self._stop_event.clear()
        self._thread = threading.Thread(
            target=self._run,
            name="mlrunx-spool-syncer",
            daemon=True,
        )
        self._thread.start()
        logger.debug("Spool syncer started")

    def stop(self, timeout: float = 5.0) -> None:
        """Stop the syncer.

        Args:
            timeout: Max time to wait for stop
        """
        if self._thread is None or not self._thread.is_alive():
            return

        self._stop_event.set()
        self._sync_event.set()  # Wake up
        self._thread.join(timeout=timeout)

    def trigger_sync(self) -> None:
        """Trigger an immediate sync attempt."""
        self._sync_event.set()

    def _run(self) -> None:
        """Main sync loop."""
        # First, recover any pending files
        self._spool.recover()

        interval = self._spool.config.sync_interval_ms / 1000.0

        while not self._stop_event.is_set():
            try:
                # Wait for interval or trigger
                self._sync_event.wait(timeout=interval)
                self._sync_event.clear()

                if self._stop_event.is_set():
                    break

                # Check if we're online
                if not self._check_online():
                    continue

                # Sync pending files
                self._sync_pending()

                # Cleanup old files periodically
                self._spool.cleanup_old_files()

            except Exception as e:
                logger.exception(f"Error in spool syncer: {e}")
                time.sleep(1)

    def _sync_pending(self) -> None:
        """Sync all pending spool files."""
        pending = self._spool.get_pending_files()

        for spool_path in pending:
            if self._stop_event.is_set():
                break

            try:
                events = self._spool.read_spool_file(spool_path)
                if not events:
                    # Empty file, just mark as done
                    self._spool.mark_synced(spool_path)
                    continue

                # Send events
                replay_result = _normalize_spool_replay_send_result(
                    self._send_func(events),
                    run_id=events[0].run_id if events else None,
                    event_count=len(events),
                )

                if replay_result.success:
                    self._spool.mark_synced(spool_path)
                    if replay_result.outcome == "dropped_terminal":
                        logger.warning(
                            "Discarded stale spool file %s for run %s (%s events): %s",
                            spool_path.name,
                            replay_result.run_id or "<unknown>",
                            replay_result.event_count,
                            replay_result.error_message or "terminal replay error",
                        )
                    else:
                        logger.info(
                            "Synced spool file %s for run %s (%s events)",
                            spool_path.name,
                            replay_result.run_id or "<unknown>",
                            replay_result.event_count,
                        )
                else:
                    # Stop syncing if send failed (we're probably offline again)
                    logger.warning(
                        "Spool sync failed for run %s from %s (%s events); will retry later: %s",
                        replay_result.run_id or "<unknown>",
                        spool_path.name,
                        replay_result.event_count,
                        replay_result.error_message or "send returned failure",
                    )
                    break

            except Exception as e:
                logger.exception(f"Error syncing spool file {spool_path}: {e}")
                # Continue to next file


def get_spool_dir() -> Path:
    """Get the default spool directory.

    Returns:
        Path to spool directory
    """
    spool_dir = Path.home() / ".mlrunx" / "spool"
    spool_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
    return spool_dir
