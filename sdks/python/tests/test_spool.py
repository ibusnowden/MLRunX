"""Tests for offline spool functionality."""

from __future__ import annotations

import json
import tempfile
import time
from pathlib import Path
from unittest.mock import MagicMock

import pytest

from mlrunx.queue import Event, EventType
from mlrunx.spool import (
    COMPLETED_EXT,
    SPOOL_EXT,
    DiskSpool,
    SpoolConfig,
    SpoolFile,
    SpoolSyncer,
)


class TestSpoolConfig:
    """Tests for SpoolConfig."""

    @pytest.mark.unit
    def test_default_config(self) -> None:
        """Test default configuration values."""
        config = SpoolConfig()
        assert config.max_file_size_bytes == 10_000_000
        assert config.max_total_size_bytes == 100_000_000
        assert config.max_files == 100
        assert config.sync_interval_ms == 5000
        assert config.retention_hours == 72


class TestSpoolFile:
    """Tests for SpoolFile."""

    @pytest.mark.unit
    def test_append_and_flush(self) -> None:
        """Test appending events and flushing to disk."""
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "test.spool"
            spool_file = SpoolFile(path, "run-123")

            event = Event(
                type=EventType.METRIC,
                run_id="run-123",
                data={"name": "loss", "value": 0.5, "step": 0},
            )
            spool_file.append(event)

            assert spool_file.event_count == 1
            assert spool_file.size_bytes > 0

            spool_file.flush()

            # File should exist
            assert path.exists()

            # Read back and verify
            with open(path) as f:
                content = json.load(f)

            assert content["run_id"] == "run-123"
            assert len(content["events"]) == 1
            assert content["events"][0]["data"]["name"] == "loss"

    @pytest.mark.unit
    def test_read_events(self) -> None:
        """Test reading events from a spool file."""
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "test.spool"

            # Create file manually
            content = {
                "version": 1,
                "run_id": "run-123",
                "created_at": time.time(),
                "events": [
                    {
                        "type": "metric",
                        "run_id": "run-123",
                        "timestamp": time.time(),
                        "data": {"name": "loss", "value": 0.5, "step": 0},
                    },
                    {
                        "type": "param",
                        "run_id": "run-123",
                        "timestamp": time.time(),
                        "data": {"name": "lr", "value": "0.001"},
                    },
                ],
            }
            with open(path, "w") as f:
                json.dump(content, f)

            spool_file = SpoolFile(path, "run-123")
            events = spool_file.read_events()

            assert len(events) == 2
            assert events[0].type == EventType.METRIC
            assert events[0].data["name"] == "loss"
            assert events[1].type == EventType.PARAM
            assert events[1].data["name"] == "lr"

    @pytest.mark.unit
    def test_mark_completed(self) -> None:
        """Test marking a spool file as completed."""
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "test.spool"
            spool_file = SpoolFile(path, "run-123")
            spool_file.append(Event(
                type=EventType.METRIC,
                run_id="run-123",
                data={"name": "loss", "value": 0.5},
            ))
            spool_file.flush()

            assert path.exists()

            spool_file.mark_completed()

            # Original should be gone
            assert not path.exists()

            # Completed file should exist
            completed_path = path.with_suffix(COMPLETED_EXT)
            assert completed_path.exists()


class TestDiskSpool:
    """Tests for DiskSpool."""

    @pytest.mark.unit
    def test_spool_events(self) -> None:
        """Test spooling events to disk."""
        with tempfile.TemporaryDirectory() as tmpdir:
            config = SpoolConfig(spool_dir=Path(tmpdir))
            spool = DiskSpool(config)

            event = Event(
                type=EventType.METRIC,
                run_id="run-123",
                data={"name": "loss", "value": 0.5, "step": 0},
            )

            assert spool.spool(event)

            # Flush to disk
            spool.flush_all()

            # Check that file was created
            files = list(Path(tmpdir).glob(f"*{SPOOL_EXT}"))
            assert len(files) == 1

    @pytest.mark.unit
    def test_get_pending_files(self) -> None:
        """Test getting pending spool files."""
        with tempfile.TemporaryDirectory() as tmpdir:
            config = SpoolConfig(spool_dir=Path(tmpdir))
            spool = DiskSpool(config)

            # Spool some events
            for i in range(3):
                event = Event(
                    type=EventType.METRIC,
                    run_id=f"run-{i}",
                    data={"name": "loss", "value": 0.5 + i * 0.1},
                )
                spool.spool(event)

            spool.flush_all()

            pending = spool.get_pending_files()
            assert len(pending) == 3

    @pytest.mark.unit
    def test_spool_size_limit(self) -> None:
        """Test that spool respects size limits."""
        with tempfile.TemporaryDirectory() as tmpdir:
            config = SpoolConfig(
                spool_dir=Path(tmpdir),
                max_total_size_bytes=100,  # Very small limit
            )
            spool = DiskSpool(config)

            # Force stats update to show we're over limit
            spool._stats.pending_bytes = 101

            event = Event(
                type=EventType.METRIC,
                run_id="run-123",
                data={"name": "loss", "value": 0.5},
            )

            # Should fail due to size limit
            assert not spool.spool(event)

    @pytest.mark.unit
    def test_read_and_mark_synced(self) -> None:
        """Test reading spool file and marking as synced."""
        with tempfile.TemporaryDirectory() as tmpdir:
            config = SpoolConfig(spool_dir=Path(tmpdir))
            spool = DiskSpool(config)

            event = Event(
                type=EventType.METRIC,
                run_id="run-123",
                data={"name": "loss", "value": 0.5},
            )
            spool.spool(event)
            spool.flush_all()

            # Get pending files
            pending = spool.get_pending_files()
            assert len(pending) == 1

            # Read events
            events = spool.read_spool_file(pending[0])
            assert len(events) == 1
            assert events[0].data["name"] == "loss"

            # Mark as synced
            spool.mark_synced(pending[0])

            # Should be no more pending files
            pending = spool.get_pending_files()
            assert len(pending) == 0

            # Completed file should exist
            completed = list(Path(tmpdir).glob(f"*{COMPLETED_EXT}"))
            assert len(completed) == 1

    @pytest.mark.unit
    def test_cleanup_old_files(self) -> None:
        """Test cleaning up old completed files."""
        with tempfile.TemporaryDirectory() as tmpdir:
            config = SpoolConfig(
                spool_dir=Path(tmpdir),
                retention_hours=0,  # Immediate expiration
            )
            spool = DiskSpool(config)

            # Create a completed file
            completed_path = Path(tmpdir) / f"old{COMPLETED_EXT}"
            with open(completed_path, "w") as f:
                json.dump({"events": []}, f)

            # Make it old
            import os
            old_time = time.time() - 3600  # 1 hour ago
            os.utime(completed_path, (old_time, old_time))

            # Cleanup should remove it
            cleaned = spool.cleanup_old_files()
            assert cleaned == 1
            assert not completed_path.exists()

    @pytest.mark.unit
    def test_recover(self) -> None:
        """Test recovery finds pending files."""
        with tempfile.TemporaryDirectory() as tmpdir:
            # Create some pending spool files
            for i in range(3):
                path = Path(tmpdir) / f"run-{i}{SPOOL_EXT}"
                with open(path, "w") as f:
                    json.dump({"events": []}, f)

            config = SpoolConfig(spool_dir=Path(tmpdir))
            spool = DiskSpool(config)

            count = spool.recover()
            assert count == 3


class TestSpoolSyncer:
    """Tests for SpoolSyncer."""

    @pytest.mark.unit
    def test_syncer_lifecycle(self) -> None:
        """Test syncer start and stop."""
        with tempfile.TemporaryDirectory() as tmpdir:
            config = SpoolConfig(spool_dir=Path(tmpdir))
            spool = DiskSpool(config)

            send_func = MagicMock(return_value=True)
            check_online = MagicMock(return_value=True)

            syncer = SpoolSyncer(spool, send_func, check_online)

            syncer.start()
            time.sleep(0.1)  # Let thread start

            syncer.stop(timeout=1.0)

    @pytest.mark.unit
    def test_syncer_syncs_pending(self) -> None:
        """Test that syncer syncs pending files."""
        with tempfile.TemporaryDirectory() as tmpdir:
            config = SpoolConfig(
                spool_dir=Path(tmpdir),
                sync_interval_ms=100,  # Fast sync for testing
            )
            spool = DiskSpool(config)

            # Create a pending spool file
            event = Event(
                type=EventType.METRIC,
                run_id="run-123",
                data={"name": "loss", "value": 0.5},
            )
            spool.spool(event)
            spool.flush_all()

            send_func = MagicMock(return_value=True)
            check_online = MagicMock(return_value=True)

            syncer = SpoolSyncer(spool, send_func, check_online)
            syncer.start()

            # Trigger sync
            syncer.trigger_sync()
            time.sleep(0.2)  # Give time to sync

            syncer.stop(timeout=1.0)

            # Send should have been called
            assert send_func.called

            # File should be marked as completed
            pending = spool.get_pending_files()
            assert len(pending) == 0

    @pytest.mark.unit
    def test_syncer_respects_offline(self) -> None:
        """Test that syncer doesn't sync when offline."""
        with tempfile.TemporaryDirectory() as tmpdir:
            config = SpoolConfig(
                spool_dir=Path(tmpdir),
                sync_interval_ms=100,
            )
            spool = DiskSpool(config)

            # Create a pending spool file
            event = Event(
                type=EventType.METRIC,
                run_id="run-123",
                data={"name": "loss", "value": 0.5},
            )
            spool.spool(event)
            spool.flush_all()

            send_func = MagicMock(return_value=True)
            check_online = MagicMock(return_value=False)  # Offline

            syncer = SpoolSyncer(spool, send_func, check_online)
            syncer.start()

            syncer.trigger_sync()
            time.sleep(0.2)

            syncer.stop(timeout=1.0)

            # Send should NOT have been called
            assert not send_func.called

            # File should still be pending
            pending = spool.get_pending_files()
            assert len(pending) == 1
