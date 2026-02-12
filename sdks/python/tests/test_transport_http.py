"""Tests for HTTP transport response handling."""

from __future__ import annotations

import httpx
import pytest

from mlrunx.transport.base import TransportError
from mlrunx.transport.http import HttpTransport


def _response(
    status_code: int,
    body: str,
    content_type: str = "application/json",
) -> httpx.Response:
    request = httpx.Request("POST", "http://localhost:3001/api/v1/ingest/batch")
    return httpx.Response(
        status_code=status_code,
        content=body.encode("utf-8"),
        headers={"content-type": content_type},
        request=request,
    )


class TestHttpTransportResponseHandling:
    """Validate strict, explicit response parsing behavior."""

    @pytest.mark.unit
    def test_handle_response_success_json(self) -> None:
        transport = HttpTransport()
        response = _response(200, '{"status":"ok","accepted":3}')

        parsed = transport._handle_response(response)

        assert parsed["status"] == "ok"
        assert parsed["accepted"] == 3

    @pytest.mark.unit
    def test_handle_response_server_error_is_retryable(self) -> None:
        transport = HttpTransport()
        response = _response(500, '{"error":"boom"}')

        with pytest.raises(TransportError, match="Server error: 500") as err:
            transport._handle_response(response)

        assert err.value.retryable is True
        assert err.value.status_code == 500

    @pytest.mark.unit
    def test_handle_response_client_error_is_not_retryable(self) -> None:
        transport = HttpTransport()
        response = _response(401, '{"error":"unauthorized"}')

        with pytest.raises(TransportError, match="Client error: 401") as err:
            transport._handle_response(response)

        assert err.value.retryable is False
        assert err.value.status_code == 401

    @pytest.mark.unit
    def test_handle_response_invalid_json_raises(self) -> None:
        transport = HttpTransport()
        response = _response(200, "not-json", content_type="text/plain")

        with pytest.raises(TransportError, match="Invalid JSON response from server"):
            transport._handle_response(response)
