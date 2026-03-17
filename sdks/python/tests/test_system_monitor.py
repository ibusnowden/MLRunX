"""Tests for the SystemMonitor background metrics collector."""

from __future__ import annotations

import time
from typing import TYPE_CHECKING
from unittest.mock import MagicMock, patch

if TYPE_CHECKING:
    from mlrunx.run import Run

import pytest

from mlrunx.config import Config
from mlrunx.system_monitor import (
    SystemMonitor,
    _get_cpu_metrics,
    _get_disk_io_metrics,
    _get_gpu_metrics,
    _get_memory_metrics,
    _get_network_io_metrics,
)

TEST_PROJECT_ID = "019c55f9-084a-7511-9393-6c17ede4a70f"


def _make_run() -> MagicMock:
    """Create a mock Run with a recording _log_system_metrics."""
    run = MagicMock()
    run._finished = False
    run._log_system_metrics_calls: list[tuple[dict, int]] = []

    def record(data: dict, step: int) -> None:
        run._log_system_metrics_calls.append((dict(data), step))

    run._log_system_metrics.side_effect = record
    return run


# ---------------------------------------------------------------------------
# SystemMonitor lifecycle
# ---------------------------------------------------------------------------


class TestSystemMonitorLifecycle:
    @pytest.mark.unit
    def test_start_and_stop(self) -> None:
        run = _make_run()
        monitor = SystemMonitor(run=run, interval_secs=60.0)
        monitor.start()
        assert monitor._thread is not None
        assert monitor._thread.is_alive()
        monitor.stop(timeout=2.0)
        assert monitor._thread is None

    @pytest.mark.unit
    def test_stop_joins_thread(self) -> None:
        run = _make_run()
        monitor = SystemMonitor(run=run, interval_secs=60.0)
        monitor.start()
        thread = monitor._thread
        monitor.stop(timeout=2.0)
        assert not thread.is_alive()

    @pytest.mark.unit
    def test_thread_is_daemon(self) -> None:
        run = _make_run()
        monitor = SystemMonitor(run=run, interval_secs=60.0)
        monitor.start()
        assert monitor._thread is not None
        assert monitor._thread.daemon is True
        monitor.stop(timeout=2.0)

    @pytest.mark.unit
    def test_thread_name(self) -> None:
        run = _make_run()
        monitor = SystemMonitor(run=run, interval_secs=60.0)
        monitor.start()
        assert monitor._thread is not None
        assert monitor._thread.name == "mlrunx-system-monitor"
        monitor.stop(timeout=2.0)

    @pytest.mark.unit
    def test_stop_without_start_is_safe(self) -> None:
        run = _make_run()
        monitor = SystemMonitor(run=run, interval_secs=60.0)
        monitor.stop(timeout=1.0)  # Should not raise

    @pytest.mark.unit
    def test_metrics_enqueued_after_interval(self) -> None:
        """Monitor should call _log_system_metrics after the interval elapses."""
        run = _make_run()
        # Short interval so test doesn't take long
        monitor = SystemMonitor(
            run=run, interval_secs=0.1, gpu=False, cpu=True, memory=False
        )
        with patch(
            "mlrunx.system_monitor._get_cpu_metrics",
            return_value={"cpu/utilization_percent": 42.0},
        ):
            monitor.start()
            # Wait long enough for at least one collection
            time.sleep(0.35)
            monitor.stop(timeout=2.0)

        assert len(run._log_system_metrics_calls) >= 1
        data, step = run._log_system_metrics_calls[0]
        assert "cpu/utilization_percent" in data
        assert step == 0

    @pytest.mark.unit
    def test_step_increments(self) -> None:
        run = _make_run()
        monitor = SystemMonitor(
            run=run, interval_secs=0.05, gpu=False, cpu=True, memory=False
        )
        with patch(
            "mlrunx.system_monitor._get_cpu_metrics",
            return_value={"cpu/utilization_percent": 10.0},
        ):
            monitor.start()
            time.sleep(0.3)
            monitor.stop(timeout=2.0)

        steps = [s for _, s in run._log_system_metrics_calls]
        assert steps == list(range(len(steps))), "Steps should be 0-indexed sequential"

    @pytest.mark.unit
    def test_collect_exception_does_not_crash_loop(self) -> None:
        run = _make_run()
        monitor = SystemMonitor(
            run=run, interval_secs=0.05, gpu=False, cpu=True, memory=False
        )
        with patch(
            "mlrunx.system_monitor._get_cpu_metrics",
            side_effect=RuntimeError("boom"),
        ):
            monitor.start()
            time.sleep(0.2)
            monitor.stop(timeout=2.0)
        # Thread finished cleanly; no unhandled exception


# ---------------------------------------------------------------------------
# Run._log_system_metrics isolation
# ---------------------------------------------------------------------------


class TestLogSystemMetricsStepIsolation:
    @pytest.mark.unit
    def test_does_not_modify_run_step(self) -> None:
        """_log_system_metrics must not touch run._step."""
        from unittest.mock import patch as _patch

        from mlrunx.config import Config
        from mlrunx.run import Run

        cfg = Config(
            project_id=TEST_PROJECT_ID,
            offline_mode=True,
            monitor_system=False,
        )

        with _patch("mlrunx.run.HttpTransport") as mock_transport_cls:
            mock_transport = MagicMock()
            mock_transport.init_run.return_value = {"offline": True}
            mock_transport_cls.return_value = mock_transport

            run = Run(project_id=TEST_PROJECT_ID, sdk_config=cfg)
            run._step = 7  # Simulate mid-training

            run._log_system_metrics({"gpu/0/utilization_percent": 80.0}, step=0)

            assert run._step == 7, "_log_system_metrics must not modify run._step"
            run.finish()

    @pytest.mark.unit
    def test_does_not_log_after_finished(self) -> None:
        from unittest.mock import patch as _patch

        from mlrunx.run import Run

        cfg = Config(project_id=TEST_PROJECT_ID, offline_mode=True, monitor_system=False)

        with _patch("mlrunx.run.HttpTransport") as mock_transport_cls:
            mock_transport = MagicMock()
            mock_transport.init_run.return_value = {"offline": True}
            mock_transport_cls.return_value = mock_transport

            run = Run(project_id=TEST_PROJECT_ID, sdk_config=cfg)
            run.finish()

            # After finish, _log_system_metrics should be a no-op
            run._log_system_metrics({"cpu/utilization_percent": 50.0}, step=0)
            # No assertion on queue count — just verify it doesn't raise
            run.finish()  # Second finish should also be safe


# ---------------------------------------------------------------------------
# Run.start_system_monitor / stop_system_monitor
# ---------------------------------------------------------------------------


class TestRunSystemMonitorMethods:
    def _make_offline_run(self) -> Run:
        from unittest.mock import patch as _patch

        from mlrunx.run import Run

        cfg = Config(project_id=TEST_PROJECT_ID, offline_mode=True, monitor_system=False)

        with _patch("mlrunx.run.HttpTransport") as mock_transport_cls:
            mock_transport = MagicMock()
            mock_transport.init_run.return_value = {"offline": True}
            mock_transport_cls.return_value = mock_transport
            return Run(project_id=TEST_PROJECT_ID, sdk_config=cfg)

    @pytest.mark.unit
    def test_start_monitor_is_idempotent(self) -> None:
        run = self._make_offline_run()
        run.start_system_monitor(interval_secs=60.0)
        first_monitor = run._system_monitor
        run.start_system_monitor(interval_secs=60.0)  # second call — no-op
        assert run._system_monitor is first_monitor
        run.finish()

    @pytest.mark.unit
    def test_finish_stops_monitor(self) -> None:
        run = self._make_offline_run()
        run.start_system_monitor(interval_secs=60.0)
        thread = run._system_monitor._thread  # type: ignore[union-attr]
        run.finish()
        assert run._system_monitor is None
        assert not thread.is_alive()

    @pytest.mark.unit
    def test_stop_monitor_when_none_is_safe(self) -> None:
        run = self._make_offline_run()
        assert run._system_monitor is None
        run.stop_system_monitor()  # Should not raise
        run.finish()


# ---------------------------------------------------------------------------
# Missing optional dependencies
# ---------------------------------------------------------------------------


class TestMissingDeps:
    @pytest.mark.unit
    def test_gpu_metrics_no_torch_no_pynvml(self) -> None:
        with patch.dict("sys.modules", {"torch": None, "pynvml": None}):
            metrics = _get_gpu_metrics()
        assert metrics == {}

    @pytest.mark.unit
    def test_cpu_metrics_no_psutil(self) -> None:
        with patch.dict("sys.modules", {"psutil": None}):
            metrics = _get_cpu_metrics()
        assert metrics == {}

    @pytest.mark.unit
    def test_memory_metrics_no_psutil(self) -> None:
        with patch.dict("sys.modules", {"psutil": None}):
            metrics = _get_memory_metrics()
        assert metrics == {}

    @pytest.mark.unit
    def test_disk_io_metrics_no_psutil(self) -> None:
        with patch.dict("sys.modules", {"psutil": None}):
            metrics = _get_disk_io_metrics()
        assert metrics == {}

    @pytest.mark.unit
    def test_network_io_metrics_no_psutil(self) -> None:
        with patch.dict("sys.modules", {"psutil": None}):
            metrics = _get_network_io_metrics()
        assert metrics == {}

    @pytest.mark.unit
    def test_monitor_collects_empty_when_all_disabled(self) -> None:
        run = _make_run()
        monitor = SystemMonitor(
            run=run,
            interval_secs=60.0,
            gpu=False,
            cpu=False,
            memory=False,
            disk_io=False,
            network_io=False,
        )
        metrics = monitor._collect()
        assert metrics == {}


# ---------------------------------------------------------------------------
# Config env vars
# ---------------------------------------------------------------------------


class TestConfigMonitorFields:
    @pytest.mark.unit
    def test_defaults(self) -> None:
        cfg = Config()
        assert cfg.monitor_system is False
        assert cfg.monitor_interval_secs == 15.0
        assert cfg.monitor_gpu is True
        assert cfg.monitor_cpu is True
        assert cfg.monitor_memory is True
        assert cfg.monitor_disk_io is False
        assert cfg.monitor_network_io is False

    @pytest.mark.unit
    def test_from_env_monitor_system(self) -> None:
        with patch.dict("os.environ", {"MLRUNX_MONITOR_SYSTEM": "true"}):
            cfg = Config.from_env()
        assert cfg.monitor_system is True

    @pytest.mark.unit
    def test_from_env_monitor_interval(self) -> None:
        with patch.dict("os.environ", {"MLRUNX_MONITOR_INTERVAL_SECS": "30"}):
            cfg = Config.from_env()
        assert cfg.monitor_interval_secs == 30.0
