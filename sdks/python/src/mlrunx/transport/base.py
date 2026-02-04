"""Base transport protocol for MLRunX SDK."""

from __future__ import annotations

from abc import ABC, abstractmethod
from typing import Any


class TransportError(Exception):
    """Error during transport operation."""

    def __init__(
        self,
        message: str,
        status_code: int | None = None,
        retryable: bool = False,
    ) -> None:
        super().__init__(message)
        self.status_code = status_code
        self.retryable = retryable


class Transport(ABC):
    """Abstract base class for transport implementations."""

    @abstractmethod
    def send_batch(
        self,
        batch: dict[str, Any],
        compressed: bool = False,
        raw_payload: bytes | None = None,
    ) -> dict[str, Any]:
        """Send a batch of events to the server.

        Args:
            batch: The batch payload to send
            compressed: Whether the raw_payload is gzip compressed
            raw_payload: Pre-serialized/compressed payload (if None, serialize batch)

        Returns:
            Server response as a dictionary

        Raises:
            TransportError: If the request fails
        """
        ...

    @abstractmethod
    def init_run(self, run_data: dict[str, Any]) -> dict[str, Any]:
        """Initialize a new run on the server.

        Args:
            run_data: Run initialization data

        Returns:
            Server response with run_id and other metadata

        Raises:
            TransportError: If the request fails
        """
        ...

    @abstractmethod
    def finish_run(self, run_id: str, status: str) -> dict[str, Any]:
        """Mark a run as finished on the server.

        Args:
            run_id: The run to finish
            status: Final status ("finished", "failed", etc.)

        Returns:
            Server response

        Raises:
            TransportError: If the request fails
        """
        ...

    @abstractmethod
    def close(self) -> None:
        """Close the transport and release resources."""
        ...
