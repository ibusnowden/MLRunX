"""Background system metrics monitor for MLRunX.

Collects GPU, CPU, memory, disk I/O, and network I/O metrics on a
daemon thread at a configurable interval and logs them to the run.
"""

from __future__ import annotations

import logging
import threading
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from mlrunx.run import Run

logger = logging.getLogger(__name__)


def _get_gpu_metrics() -> dict[str, float]:
    """Collect GPU metrics via PyTorch or pynvml."""
    metrics: dict[str, float] = {}
    try:
        import torch  # type: ignore[import-not-found]

        if torch.cuda.is_available():
            for i in range(torch.cuda.device_count()):
                allocated = torch.cuda.memory_allocated(i)
                reserved = torch.cuda.memory_reserved(i)
                total = torch.cuda.get_device_properties(i).total_memory
                prefix = f"gpu/{i}"
                metrics[f"{prefix}/memory_allocated_mb"] = allocated / 1e6
                metrics[f"{prefix}/memory_reserved_mb"] = reserved / 1e6
                metrics[f"{prefix}/memory_total_mb"] = total / 1e6
                if total > 0:
                    metrics[f"{prefix}/memory_utilization_percent"] = (
                        allocated / total * 100
                    )
    except ImportError:
        pass
    except Exception as e:
        logger.debug("GPU metrics (torch) collection failed: %s", e)

    # Try pynvml for utilization %
    try:
        import pynvml  # type: ignore[import-not-found]

        pynvml.nvmlInit()
        count = pynvml.nvmlDeviceGetCount()
        for i in range(count):
            handle = pynvml.nvmlDeviceGetHandleByIndex(i)
            util = pynvml.nvmlDeviceGetUtilizationRates(handle)
            metrics[f"gpu/{i}/utilization_percent"] = float(util.gpu)
            metrics[f"gpu/{i}/memory_util_percent"] = float(util.memory)
            mem_info = pynvml.nvmlDeviceGetMemoryInfo(handle)
            metrics[f"gpu/{i}/memory_used_mb"] = mem_info.used / 1e6
            metrics[f"gpu/{i}/memory_total_mb"] = mem_info.total / 1e6
        pynvml.nvmlShutdown()
    except ImportError:
        pass
    except Exception as e:
        logger.debug("GPU metrics (pynvml) collection failed: %s", e)

    return metrics


def _get_cpu_metrics() -> dict[str, float]:
    """Collect CPU utilization and frequency metrics via psutil."""
    metrics: dict[str, float] = {}
    try:
        import psutil  # type: ignore[import-untyped]

        metrics["cpu/utilization_percent"] = psutil.cpu_percent(interval=None)
        freq = psutil.cpu_freq()
        if freq is not None:
            metrics["cpu/frequency_mhz"] = freq.current
        metrics["cpu/count_logical"] = float(psutil.cpu_count(logical=True) or 0)
    except ImportError:
        pass
    except Exception as e:
        logger.debug("CPU metrics collection failed: %s", e)
    return metrics


def _get_memory_metrics() -> dict[str, float]:
    """Collect system RAM metrics via psutil."""
    metrics: dict[str, float] = {}
    try:
        import psutil

        vm = psutil.virtual_memory()
        metrics["memory/total_mb"] = vm.total / 1e6
        metrics["memory/used_mb"] = vm.used / 1e6
        metrics["memory/available_mb"] = vm.available / 1e6
        metrics["memory/utilization_percent"] = vm.percent
    except ImportError:
        pass
    except Exception as e:
        logger.debug("Memory metrics collection failed: %s", e)
    return metrics


def _get_disk_io_metrics() -> dict[str, float]:
    """Collect disk I/O counters via psutil."""
    metrics: dict[str, float] = {}
    try:
        import psutil

        io = psutil.disk_io_counters()
        if io is not None:
            metrics["disk_io/read_bytes"] = float(io.read_bytes)
            metrics["disk_io/write_bytes"] = float(io.write_bytes)
            metrics["disk_io/read_count"] = float(io.read_count)
            metrics["disk_io/write_count"] = float(io.write_count)
    except ImportError:
        pass
    except Exception as e:
        logger.debug("Disk I/O metrics collection failed: %s", e)
    return metrics


def _get_network_io_metrics() -> dict[str, float]:
    """Collect network I/O counters via psutil."""
    metrics: dict[str, float] = {}
    try:
        import psutil

        net = psutil.net_io_counters()
        if net is not None:
            metrics["network_io/bytes_sent"] = float(net.bytes_sent)
            metrics["network_io/bytes_recv"] = float(net.bytes_recv)
            metrics["network_io/packets_sent"] = float(net.packets_sent)
            metrics["network_io/packets_recv"] = float(net.packets_recv)
    except ImportError:
        pass
    except Exception as e:
        logger.debug("Network I/O metrics collection failed: %s", e)
    return metrics


class SystemMonitor:
    """Background thread that periodically collects hardware metrics.

    Runs as a daemon thread so it does not prevent interpreter exit.
    Uses threading.Event.wait() instead of time.sleep() for clean shutdown.

    Example:
        monitor = SystemMonitor(run=run, interval_secs=15.0)
        monitor.start()
        # ... training loop ...
        monitor.stop()
    """

    def __init__(
        self,
        run: Run,
        interval_secs: float = 15.0,
        gpu: bool = True,
        cpu: bool = True,
        memory: bool = True,
        disk_io: bool = False,
        network_io: bool = False,
    ) -> None:
        self._run = run
        self._interval = interval_secs
        self._gpu = gpu
        self._cpu = cpu
        self._memory = memory
        self._disk_io = disk_io
        self._network_io = network_io
        self._stop_event = threading.Event()
        self._thread: threading.Thread | None = None

    def start(self) -> None:
        """Start the background monitoring thread."""
        self._stop_event.clear()
        self._thread = threading.Thread(
            target=self._loop,
            daemon=True,
            name="mlrunx-system-monitor",
        )
        self._thread.start()
        logger.debug(
            "SystemMonitor started (interval=%.1fs, gpu=%s, cpu=%s, memory=%s)",
            self._interval,
            self._gpu,
            self._cpu,
            self._memory,
        )

    def stop(self, timeout: float = 5.0) -> None:
        """Signal the monitor to stop and wait for the thread to join."""
        self._stop_event.set()
        if self._thread is not None:
            self._thread.join(timeout=timeout)
            self._thread = None
        logger.debug("SystemMonitor stopped")

    def _loop(self) -> None:
        step = 0
        while not self._stop_event.wait(timeout=self._interval):
            metrics = self._collect()
            if metrics:
                self._run._log_system_metrics(metrics, step=step)
            step += 1

    def _collect(self) -> dict[str, float]:
        """Collect all enabled metrics, swallowing any individual failures."""
        metrics: dict[str, float] = {}
        if self._gpu:
            try:
                metrics.update(_get_gpu_metrics())
            except Exception as e:
                logger.debug("GPU collection error: %s", e)
        if self._cpu:
            try:
                metrics.update(_get_cpu_metrics())
            except Exception as e:
                logger.debug("CPU collection error: %s", e)
        if self._memory:
            try:
                metrics.update(_get_memory_metrics())
            except Exception as e:
                logger.debug("Memory collection error: %s", e)
        if self._disk_io:
            try:
                metrics.update(_get_disk_io_metrics())
            except Exception as e:
                logger.debug("Disk I/O collection error: %s", e)
        if self._network_io:
            try:
                metrics.update(_get_network_io_metrics())
            except Exception as e:
                logger.debug("Network I/O collection error: %s", e)
        return metrics
