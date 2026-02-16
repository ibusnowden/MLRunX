"""Tests for flush worker error handling."""

from __future__ import annotations

from unittest.mock import MagicMock

import pytest

from mlrunx.config import Config
from mlrunx.queue import Event, EventQueue, EventType
from mlrunx.transport.base import TransportError
from mlrunx.worker import FlushWorker


def _make_worker(transport: MagicMock) -> FlushWorker:
    config = Config(
        spool_enabled=False,
        max_retries=0,
    )
    return FlushWorker(
        queue=EventQueue(max_size=16),
        transport=transport,
        config=config,
    )


def _metric_event(run_id: str) -> Event:
    return Event(
        type=EventType.METRIC,
        run_id=run_id,
        data={"name": "loss", "value": 0.5, "step": 1},
    )


class TestFlushWorkerSpoolReplay:
    """Tests for spool replay terminal error behavior."""

    @pytest.mark.unit
    def test_spool_replay_drops_missing_run_404(self) -> None:
        transport = MagicMock()
        transport.send_batch.side_effect = TransportError(
            "Client error: 404 - Run not found: stale-run",
            status_code=404,
            retryable=False,
        )
        worker = _make_worker(transport)

        success = worker._send_spooled_events([_metric_event("stale-run")])

        assert success is True
        assert worker.error_count == 0

    @pytest.mark.unit
    def test_spool_replay_drops_not_running_412(self) -> None:
        transport = MagicMock()
        transport.send_batch.side_effect = TransportError(
            "Client error: 412 - Run stale-run is not running",
            status_code=412,
            retryable=False,
        )
        worker = _make_worker(transport)

        success = worker._send_spooled_events([_metric_event("stale-run")])

        assert success is True
        assert worker.error_count == 0

    @pytest.mark.unit
    def test_spool_replay_keeps_non_terminal_client_errors(self) -> None:
        transport = MagicMock()
        transport.send_batch.side_effect = TransportError(
            "Client error: 401 - unauthorized",
            status_code=401,
            retryable=False,
        )
        worker = _make_worker(transport)

        success = worker._send_spooled_events([_metric_event("run-1")])

        assert success is False
        assert worker.error_count == 1
