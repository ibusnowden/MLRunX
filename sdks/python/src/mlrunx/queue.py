"""Thread-safe event queue for MLRunX SDK.

Events are queued for batch processing by the background worker.
The queue is bounded to prevent unbounded memory growth.
"""

from __future__ import annotations

import queue
import time
from dataclasses import dataclass, field
from enum import Enum
from typing import Any


class EventType(Enum):
    """Types of events that can be queued."""

    METRIC = "metric"
    PARAM = "param"
    TAG = "tag"
    ARTIFACT = "artifact"
    RUN_START = "run_start"
    RUN_FINISH = "run_finish"


@dataclass
class Event:
    """A single event to be sent to the server."""

    type: EventType
    run_id: str
    timestamp: float = field(default_factory=time.time)
    data: dict[str, Any] = field(default_factory=dict)


class EventQueue:
    """Thread-safe bounded event queue.

    Uses a standard queue.Queue which is thread-safe for
    multi-producer, multi-consumer scenarios.
    """

    def __init__(self, max_size: int = 10000) -> None:
        """Initialize the event queue.

        Args:
            max_size: Maximum number of events to hold. Events are dropped
                     when the queue is full.
        """
        self._queue: queue.Queue[Event] = queue.Queue(maxsize=max_size)
        self._dropped_count = 0

    def put(self, event: Event) -> bool:
        """Add an event to the queue (non-blocking).

        Args:
            event: The event to queue

        Returns:
            True if the event was queued, False if dropped due to full queue
        """
        try:
            self._queue.put_nowait(event)
            return True
        except queue.Full:
            self._dropped_count += 1
            return False

    def get_batch(self, max_items: int, timeout_ms: int) -> list[Event]:
        """Get a batch of events from the queue.

        Blocks until either:
        - max_items events are available
        - timeout_ms milliseconds have passed
        - The queue is empty after getting at least one item

        Args:
            max_items: Maximum number of events to return
            timeout_ms: Maximum time to wait in milliseconds

        Returns:
            List of events (may be empty if timeout with no events)
        """
        events: list[Event] = []
        deadline = time.time() + (timeout_ms / 1000.0)

        while len(events) < max_items:
            remaining = deadline - time.time()
            if remaining <= 0:
                break

            try:
                event = self._queue.get(timeout=min(remaining, 0.1))
                events.append(event)
            except queue.Empty:
                # If we have some events, return them
                if events:
                    break
                # Otherwise, keep waiting until deadline

        return events

    def drain(self) -> list[Event]:
        """Drain all events from the queue (non-blocking).

        Returns:
            List of all events currently in the queue
        """
        events: list[Event] = []
        while True:
            try:
                event = self._queue.get_nowait()
                events.append(event)
            except queue.Empty:
                break
        return events

    @property
    def size(self) -> int:
        """Current number of events in the queue."""
        return self._queue.qsize()

    @property
    def dropped_count(self) -> int:
        """Number of events dropped due to full queue."""
        return self._dropped_count

    def is_empty(self) -> bool:
        """Check if the queue is empty."""
        return self._queue.empty()
