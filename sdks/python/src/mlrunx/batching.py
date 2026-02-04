"""Adaptive batching for MLRunX SDK.

This module provides intelligent batching with:
- Multiple flush triggers (size, bytes, time)
- Metric coalescing (combine duplicate metrics)
- Parameter/tag deduplication (keep last value)
"""

from __future__ import annotations

import time
from dataclasses import dataclass, field
from typing import Any

from mlrunx.queue import Event, EventType


@dataclass
class BatchConfig:
    """Configuration for adaptive batching."""

    # Flush triggers
    max_items: int = 1000  # Max events before flush
    max_bytes: int = 1_000_000  # Max estimated batch size (1MB)
    max_age_ms: int = 1000  # Max time since first event (milliseconds)

    # Coalescing
    coalesce_metrics: bool = True  # Combine same metric at same step
    dedupe_params: bool = True  # Keep only last value for params
    dedupe_tags: bool = True  # Keep only last value for tags


@dataclass
class BatchStats:
    """Statistics for a batch."""

    event_count: int = 0
    metric_count: int = 0
    param_count: int = 0
    tag_count: int = 0
    estimated_bytes: int = 0
    coalesced_count: int = 0  # Events merged by coalescing
    created_at: float = field(default_factory=time.time)

    @property
    def age_ms(self) -> float:
        """Age of the batch in milliseconds."""
        return (time.time() - self.created_at) * 1000


class AdaptiveBatcher:
    """Batches events with adaptive flush triggers and coalescing.

    Events are accumulated until one of the flush triggers fires:
    - max_items: Maximum number of events
    - max_bytes: Maximum estimated batch size
    - max_age_ms: Maximum time since first event

    Coalescing reduces batch size by:
    - Merging metrics with same (name, step) - keeps latest value
    - Keeping only latest value for each param/tag key
    """

    def __init__(self, config: BatchConfig | None = None) -> None:
        """Initialize the batcher.

        Args:
            config: Batching configuration
        """
        self._config = config or BatchConfig()
        self._events: list[Event] = []
        self._stats = BatchStats()

        # Coalescing indexes
        # metric key: (name, step) -> index in _events
        self._metric_index: dict[tuple[str, int], int] = {}
        # param key: name -> index in _events
        self._param_index: dict[str, int] = {}
        # tag key: key -> index in _events
        self._tag_index: dict[str, int] = {}

    def add(self, event: Event) -> bool:
        """Add an event to the batch.

        Args:
            event: The event to add

        Returns:
            True if the batch should be flushed after this add
        """
        if self._stats.event_count == 0:
            self._stats.created_at = time.time()

        # Apply coalescing based on event type
        if event.type == EventType.METRIC and self._config.coalesce_metrics:
            self._add_metric_coalesced(event)
        elif event.type == EventType.PARAM and self._config.dedupe_params:
            self._add_param_deduped(event)
        elif event.type == EventType.TAG and self._config.dedupe_tags:
            self._add_tag_deduped(event)
        else:
            self._events.append(event)
            self._update_stats(event)

        return self.should_flush()

    def _add_metric_coalesced(self, event: Event) -> None:
        """Add a metric with coalescing."""
        name = event.data.get("name", "")
        step = event.data.get("step", 0)
        key = (name, step)

        if key in self._metric_index:
            # Replace existing metric at same (name, step)
            idx = self._metric_index[key]
            old_event = self._events[idx]
            self._events[idx] = event
            self._stats.coalesced_count += 1
            # Update byte estimate
            self._stats.estimated_bytes -= self._estimate_event_size(old_event)
            self._stats.estimated_bytes += self._estimate_event_size(event)
        else:
            # New metric
            self._metric_index[key] = len(self._events)
            self._events.append(event)
            self._update_stats(event)

    def _add_param_deduped(self, event: Event) -> None:
        """Add a param with deduplication."""
        name = event.data.get("name", "")

        if name in self._param_index:
            # Replace existing param
            idx = self._param_index[name]
            old_event = self._events[idx]
            self._events[idx] = event
            self._stats.coalesced_count += 1
            self._stats.estimated_bytes -= self._estimate_event_size(old_event)
            self._stats.estimated_bytes += self._estimate_event_size(event)
        else:
            # New param
            self._param_index[name] = len(self._events)
            self._events.append(event)
            self._update_stats(event)

    def _add_tag_deduped(self, event: Event) -> None:
        """Add a tag with deduplication."""
        key = event.data.get("key", "")

        if key in self._tag_index:
            # Replace existing tag
            idx = self._tag_index[key]
            old_event = self._events[idx]
            self._events[idx] = event
            self._stats.coalesced_count += 1
            self._stats.estimated_bytes -= self._estimate_event_size(old_event)
            self._stats.estimated_bytes += self._estimate_event_size(event)
        else:
            # New tag
            self._tag_index[key] = len(self._events)
            self._events.append(event)
            self._update_stats(event)

    def _update_stats(self, event: Event) -> None:
        """Update statistics after adding an event."""
        self._stats.event_count += 1
        self._stats.estimated_bytes += self._estimate_event_size(event)

        if event.type == EventType.METRIC:
            self._stats.metric_count += 1
        elif event.type == EventType.PARAM:
            self._stats.param_count += 1
        elif event.type == EventType.TAG:
            self._stats.tag_count += 1

    def _estimate_event_size(self, event: Event) -> int:
        """Estimate the serialized size of an event in bytes."""
        # Rough estimate: JSON overhead + data
        base_size = 50  # JSON structure overhead
        data_size = sum(
            len(str(k)) + len(str(v)) + 10
            for k, v in event.data.items()
        )
        return base_size + data_size

    def should_flush(self) -> bool:
        """Check if the batch should be flushed.

        Returns:
            True if any flush trigger has fired
        """
        if self._stats.event_count >= self._config.max_items:
            return True
        if self._stats.estimated_bytes >= self._config.max_bytes:
            return True
        return self._stats.age_ms >= self._config.max_age_ms

    def flush(self) -> tuple[list[Event], BatchStats]:
        """Flush the batch and return events with stats.

        Returns:
            Tuple of (events, stats)
        """
        # Filter out None entries (from coalescing)
        events = [e for e in self._events if e is not None]
        stats = self._stats

        # Reset state
        self._events = []
        self._stats = BatchStats()
        self._metric_index.clear()
        self._param_index.clear()
        self._tag_index.clear()

        return events, stats

    def is_empty(self) -> bool:
        """Check if the batch is empty."""
        return self._stats.event_count == 0

    @property
    def stats(self) -> BatchStats:
        """Current batch statistics."""
        return self._stats

    @property
    def config(self) -> BatchConfig:
        """Batch configuration."""
        return self._config


@dataclass
class FlushMetrics:
    """Metrics about flush operations for monitoring."""

    total_flushes: int = 0
    total_events_sent: int = 0
    total_bytes_sent: int = 0
    total_coalesced: int = 0
    last_flush_time: float = 0.0
    last_flush_duration_ms: float = 0.0
    last_batch_size: int = 0

    # Trigger counts
    size_triggered: int = 0
    bytes_triggered: int = 0
    time_triggered: int = 0
    manual_triggered: int = 0

    def record_flush(
        self,
        events: int,
        bytes_est: int,
        coalesced: int,
        duration_ms: float,
        trigger: str,
    ) -> None:
        """Record a flush operation."""
        self.total_flushes += 1
        self.total_events_sent += events
        self.total_bytes_sent += bytes_est
        self.total_coalesced += coalesced
        self.last_flush_time = time.time()
        self.last_flush_duration_ms = duration_ms
        self.last_batch_size = events

        if trigger == "size":
            self.size_triggered += 1
        elif trigger == "bytes":
            self.bytes_triggered += 1
        elif trigger == "time":
            self.time_triggered += 1
        else:
            self.manual_triggered += 1

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary for logging/export."""
        return {
            "total_flushes": self.total_flushes,
            "total_events_sent": self.total_events_sent,
            "total_bytes_sent": self.total_bytes_sent,
            "total_coalesced": self.total_coalesced,
            "last_flush_duration_ms": self.last_flush_duration_ms,
            "last_batch_size": self.last_batch_size,
            "triggers": {
                "size": self.size_triggered,
                "bytes": self.bytes_triggered,
                "time": self.time_triggered,
                "manual": self.manual_triggered,
            },
        }
