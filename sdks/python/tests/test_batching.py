"""Tests for adaptive batching."""

from __future__ import annotations

import time

import pytest

from mlrunx.batching import AdaptiveBatcher, BatchConfig, BatchStats, FlushMetrics
from mlrunx.queue import Event, EventType


class TestBatchConfig:
    """Tests for BatchConfig."""

    @pytest.mark.unit
    def test_default_config(self) -> None:
        """Test default configuration values."""
        config = BatchConfig()
        assert config.max_items == 1000
        assert config.max_bytes == 1_000_000
        assert config.max_age_ms == 1000
        assert config.coalesce_metrics is True
        assert config.dedupe_params is True
        assert config.dedupe_tags is True


class TestBatchStats:
    """Tests for BatchStats."""

    @pytest.mark.unit
    def test_age_ms(self) -> None:
        """Test age calculation."""
        stats = BatchStats(created_at=time.time() - 0.5)  # 500ms ago
        assert 450 < stats.age_ms < 550  # Allow some tolerance


class TestAdaptiveBatcher:
    """Tests for AdaptiveBatcher."""

    @pytest.mark.unit
    def test_add_events(self) -> None:
        """Test adding events to batcher."""
        batcher = AdaptiveBatcher(BatchConfig(max_items=10))

        event = Event(
            type=EventType.METRIC,
            run_id="test-run",
            data={"name": "loss", "value": 0.5, "step": 0},
        )

        should_flush = batcher.add(event)
        assert not should_flush
        assert batcher.stats.event_count == 1
        assert batcher.stats.metric_count == 1

    @pytest.mark.unit
    def test_flush_trigger_size(self) -> None:
        """Test flush triggered by max items."""
        batcher = AdaptiveBatcher(BatchConfig(max_items=3))

        for i in range(3):
            event = Event(
                type=EventType.METRIC,
                run_id="test-run",
                data={"name": f"metric_{i}", "value": i, "step": i},
            )
            should_flush = batcher.add(event)

        # Should trigger flush after reaching max_items
        assert should_flush

    @pytest.mark.unit
    def test_metric_coalescing(self) -> None:
        """Test that metrics with same (name, step) are coalesced."""
        batcher = AdaptiveBatcher(
            BatchConfig(max_items=100, coalesce_metrics=True)
        )

        # Add same metric multiple times at same step
        for value in [0.5, 0.4, 0.3]:
            event = Event(
                type=EventType.METRIC,
                run_id="test-run",
                data={"name": "loss", "value": value, "step": 0},
            )
            batcher.add(event)

        # Should have only 1 event (coalesced), keeping last value
        assert batcher.stats.event_count == 1
        assert batcher.stats.coalesced_count == 2

        events, stats = batcher.flush()
        assert len(events) == 1
        assert events[0].data["value"] == 0.3  # Last value

    @pytest.mark.unit
    def test_metric_no_coalescing_different_step(self) -> None:
        """Test that metrics at different steps are not coalesced."""
        batcher = AdaptiveBatcher(
            BatchConfig(max_items=100, coalesce_metrics=True)
        )

        # Add same metric at different steps
        for step in range(3):
            event = Event(
                type=EventType.METRIC,
                run_id="test-run",
                data={"name": "loss", "value": 0.5 - step * 0.1, "step": step},
            )
            batcher.add(event)

        # Should have 3 separate events
        assert batcher.stats.event_count == 3
        assert batcher.stats.coalesced_count == 0

    @pytest.mark.unit
    def test_param_deduplication(self) -> None:
        """Test that params with same name are deduplicated."""
        batcher = AdaptiveBatcher(
            BatchConfig(max_items=100, dedupe_params=True)
        )

        # Add same param multiple times
        for value in ["0.001", "0.01", "0.1"]:
            event = Event(
                type=EventType.PARAM,
                run_id="test-run",
                data={"name": "lr", "value": value},
            )
            batcher.add(event)

        # Should have only 1 event (deduplicated), keeping last value
        events, stats = batcher.flush()
        assert len(events) == 1
        assert events[0].data["value"] == "0.1"  # Last value

    @pytest.mark.unit
    def test_tag_deduplication(self) -> None:
        """Test that tags with same key are deduplicated."""
        batcher = AdaptiveBatcher(
            BatchConfig(max_items=100, dedupe_tags=True)
        )

        # Add same tag multiple times
        for value in ["resnet18", "resnet34", "resnet50"]:
            event = Event(
                type=EventType.TAG,
                run_id="test-run",
                data={"key": "model", "value": value},
            )
            batcher.add(event)

        # Should have only 1 event (deduplicated), keeping last value
        events, stats = batcher.flush()
        assert len(events) == 1
        assert events[0].data["value"] == "resnet50"  # Last value

    @pytest.mark.unit
    def test_flush_returns_events_and_stats(self) -> None:
        """Test flush returns events and statistics."""
        batcher = AdaptiveBatcher(BatchConfig(max_items=100))

        # Add mixed events
        batcher.add(Event(
            type=EventType.METRIC,
            run_id="test-run",
            data={"name": "loss", "value": 0.5, "step": 0},
        ))
        batcher.add(Event(
            type=EventType.PARAM,
            run_id="test-run",
            data={"name": "lr", "value": "0.001"},
        ))
        batcher.add(Event(
            type=EventType.TAG,
            run_id="test-run",
            data={"key": "model", "value": "resnet"},
        ))

        events, stats = batcher.flush()

        assert len(events) == 3
        assert stats.metric_count == 1
        assert stats.param_count == 1
        assert stats.tag_count == 1

        # Batcher should be empty after flush
        assert batcher.is_empty()

    @pytest.mark.unit
    def test_flush_resets_state(self) -> None:
        """Test that flush resets batcher state."""
        batcher = AdaptiveBatcher(BatchConfig(max_items=100))

        batcher.add(Event(
            type=EventType.METRIC,
            run_id="test-run",
            data={"name": "loss", "value": 0.5, "step": 0},
        ))

        batcher.flush()

        # State should be reset
        assert batcher.is_empty()
        assert batcher.stats.event_count == 0
        assert batcher.stats.estimated_bytes == 0

    @pytest.mark.unit
    def test_coalescing_disabled(self) -> None:
        """Test behavior when coalescing is disabled."""
        batcher = AdaptiveBatcher(
            BatchConfig(
                max_items=100,
                coalesce_metrics=False,
                dedupe_params=False,
                dedupe_tags=False,
            )
        )

        # Add same metric multiple times
        for value in [0.5, 0.4, 0.3]:
            event = Event(
                type=EventType.METRIC,
                run_id="test-run",
                data={"name": "loss", "value": value, "step": 0},
            )
            batcher.add(event)

        # All events should be kept
        events, stats = batcher.flush()
        assert len(events) == 3


class TestFlushMetrics:
    """Tests for FlushMetrics."""

    @pytest.mark.unit
    def test_record_flush(self) -> None:
        """Test recording flush metrics."""
        metrics = FlushMetrics()

        metrics.record_flush(
            events=100,
            bytes_est=5000,
            coalesced=10,
            duration_ms=50.0,
            trigger="size",
        )

        assert metrics.total_flushes == 1
        assert metrics.total_events_sent == 100
        assert metrics.total_bytes_sent == 5000
        assert metrics.total_coalesced == 10
        assert metrics.size_triggered == 1
        assert metrics.last_flush_duration_ms == 50.0

    @pytest.mark.unit
    def test_trigger_counts(self) -> None:
        """Test that different triggers are tracked."""
        metrics = FlushMetrics()

        metrics.record_flush(
            events=10,
            bytes_est=100,
            coalesced=0,
            duration_ms=10,
            trigger="size",
        )
        metrics.record_flush(
            events=10,
            bytes_est=100,
            coalesced=0,
            duration_ms=10,
            trigger="bytes",
        )
        metrics.record_flush(
            events=10,
            bytes_est=100,
            coalesced=0,
            duration_ms=10,
            trigger="time",
        )
        metrics.record_flush(
            events=10,
            bytes_est=100,
            coalesced=0,
            duration_ms=10,
            trigger="manual",
        )

        assert metrics.size_triggered == 1
        assert metrics.bytes_triggered == 1
        assert metrics.time_triggered == 1
        assert metrics.manual_triggered == 1

    @pytest.mark.unit
    def test_to_dict(self) -> None:
        """Test conversion to dictionary."""
        metrics = FlushMetrics()
        metrics.record_flush(
            events=50,
            bytes_est=2500,
            coalesced=5,
            duration_ms=25.0,
            trigger="size",
        )

        d = metrics.to_dict()

        assert d["total_flushes"] == 1
        assert d["total_events_sent"] == 50
        assert d["triggers"]["size"] == 1
