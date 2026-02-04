"""HTTP transport implementation for MLRunX SDK.

Uses httpx for async-friendly HTTP requests.
This is the default transport for alpha releases.
"""

from __future__ import annotations

import logging
from typing import Any

import httpx

from mlrunx.transport.base import Transport, TransportError

logger = logging.getLogger(__name__)


class HttpTransport(Transport):
    """HTTP transport using httpx.

    Sends batches to the MLRunX API server via HTTP POST requests.
    """

    def __init__(
        self,
        base_url: str = "http://localhost:3001",
        api_key: str | None = None,
        timeout: float = 30.0,
    ) -> None:
        """Initialize the HTTP transport.

        Args:
            base_url: Base URL of the MLRunX API server
            api_key: Optional API key for authentication
            timeout: Request timeout in seconds
        """
        self._base_url = base_url.rstrip("/")
        self._api_key = api_key
        self._timeout = timeout
        self._client: httpx.Client | None = None

    def _get_client(self) -> httpx.Client:
        """Get or create the HTTP client."""
        if self._client is None:
            headers = {"Content-Type": "application/json"}
            if self._api_key:
                headers["Authorization"] = f"Bearer {self._api_key}"

            self._client = httpx.Client(
                base_url=self._base_url,
                headers=headers,
                timeout=self._timeout,
            )
        return self._client

    def _handle_response(self, response: httpx.Response) -> dict[str, Any]:
        """Handle the HTTP response.

        Args:
            response: The HTTP response

        Returns:
            Parsed JSON response

        Raises:
            TransportError: If the response indicates an error
        """
        if response.status_code >= 500:
            raise TransportError(
                f"Server error: {response.status_code}",
                status_code=response.status_code,
                retryable=True,
            )

        if response.status_code >= 400:
            raise TransportError(
                f"Client error: {response.status_code} - {response.text}",
                status_code=response.status_code,
                retryable=False,
            )

        try:
            return response.json()
        except Exception:
            return {"status": "ok"}

    def send_batch(
        self,
        batch: dict[str, Any],
        compressed: bool = False,
        raw_payload: bytes | None = None,
    ) -> dict[str, Any]:
        """Send a batch of events to the server.

        Args:
            batch: The batch payload containing events
            compressed: Whether raw_payload is gzip compressed
            raw_payload: Pre-serialized/compressed payload (if None, serialize batch as JSON)

        Returns:
            Server response

        Raises:
            TransportError: If the request fails
        """
        try:
            client = self._get_client()

            if raw_payload is not None and compressed:
                # Send pre-compressed payload with appropriate headers
                headers = {
                    "Content-Type": "application/json",
                    "Content-Encoding": "gzip",
                }
                response = client.post(
                    "/api/v1/ingest/batch",
                    content=raw_payload,
                    headers=headers,
                )
            else:
                # Send as regular JSON
                response = client.post("/api/v1/ingest/batch", json=batch)

            return self._handle_response(response)
        except httpx.ConnectError as e:
            raise TransportError(
                f"Connection failed: {e}",
                retryable=True,
            ) from e
        except httpx.TimeoutException as e:
            raise TransportError(
                f"Request timed out: {e}",
                retryable=True,
            ) from e
        except TransportError:
            raise
        except Exception as e:
            raise TransportError(f"Unexpected error: {e}") from e

    def init_run(self, run_data: dict[str, Any]) -> dict[str, Any]:
        """Initialize a new run on the server.

        Args:
            run_data: Run initialization data

        Returns:
            Server response with run_id

        Raises:
            TransportError: If the request fails
        """
        try:
            client = self._get_client()
            response = client.post("/api/v1/runs", json=run_data)
            return self._handle_response(response)
        except httpx.ConnectError as e:
            # In offline mode, generate a local run ID
            logger.warning(f"Server unavailable, running in offline mode: {e}")
            import uuid

            return {
                "run_id": str(uuid.uuid4()),
                "offline": True,
            }
        except httpx.TimeoutException as e:
            raise TransportError(
                f"Request timed out: {e}",
                retryable=True,
            ) from e
        except TransportError:
            raise
        except Exception as e:
            raise TransportError(f"Unexpected error: {e}") from e

    def finish_run(self, run_id: str, status: str) -> dict[str, Any]:
        """Mark a run as finished.

        Args:
            run_id: The run to finish
            status: Final status

        Returns:
            Server response

        Raises:
            TransportError: If the request fails
        """
        try:
            client = self._get_client()
            response = client.post(
                f"/api/v1/runs/{run_id}/finish",
                json={"status": status},
            )
            return self._handle_response(response)
        except httpx.ConnectError:
            logger.warning("Server unavailable, run finish will be synced later")
            return {"status": "pending_sync"}
        except TransportError:
            raise
        except Exception as e:
            raise TransportError(f"Unexpected error: {e}") from e

    def close(self) -> None:
        """Close the HTTP client."""
        if self._client is not None:
            self._client.close()
            self._client = None
