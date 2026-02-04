"""System metrics collection utility for MLRunX examples.

This module provides functions to collect and log system metrics including:
- GPU metrics (memory, utilization, temperature)
- CPU utilization
- RAM usage

Usage:
    from system_metrics import get_system_metrics, log_system_metrics

    # In your training loop:
    metrics = get_system_metrics()
    run.log(metrics, step=step)

    # Or use the helper:
    log_system_metrics(run, step)
"""

from __future__ import annotations

import os
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    import mlrunx

# Try to import optional dependencies
try:
    import torch
    HAS_TORCH = True
except ImportError:
    HAS_TORCH = False

try:
    import psutil
    HAS_PSUTIL = True
except ImportError:
    HAS_PSUTIL = False


def get_gpu_metrics() -> dict[str, float]:
    """Get GPU metrics if available.

    Returns metrics for CUDA or MPS devices.
    """
    metrics = {}

    if not HAS_TORCH:
        return metrics

    # CUDA GPU metrics
    if torch.cuda.is_available():
        try:
            device_count = torch.cuda.device_count()
            metrics["gpu/device_count"] = device_count

            for i in range(device_count):
                # Memory metrics
                mem_allocated = torch.cuda.memory_allocated(i) / (1024 ** 3)  # GB
                mem_reserved = torch.cuda.memory_reserved(i) / (1024 ** 3)  # GB
                mem_total = torch.cuda.get_device_properties(i).total_memory / (1024 ** 3)  # GB

                prefix = f"gpu/{i}" if device_count > 1 else "gpu"
                metrics[f"{prefix}/memory_allocated_gb"] = mem_allocated
                metrics[f"{prefix}/memory_reserved_gb"] = mem_reserved
                metrics[f"{prefix}/memory_total_gb"] = mem_total
                metrics[f"{prefix}/memory_used_percent"] = (mem_allocated / mem_total) * 100

                # Try to get utilization via nvidia-smi (if pynvml available)
                try:
                    import pynvml
                    pynvml.nvmlInit()
                    handle = pynvml.nvmlDeviceGetHandleByIndex(i)

                    # Utilization
                    util = pynvml.nvmlDeviceGetUtilizationRates(handle)
                    metrics[f"{prefix}/utilization_percent"] = util.gpu
                    metrics[f"{prefix}/memory_utilization_percent"] = util.memory

                    # Temperature
                    temp = pynvml.nvmlDeviceGetTemperature(handle, pynvml.NVML_TEMPERATURE_GPU)
                    metrics[f"{prefix}/temperature_c"] = temp

                    # Power
                    try:
                        power = pynvml.nvmlDeviceGetPowerUsage(handle) / 1000  # W
                        metrics[f"{prefix}/power_w"] = power
                    except pynvml.NVMLError:
                        pass

                    pynvml.nvmlShutdown()
                except (ImportError, Exception):
                    pass

        except Exception:
            pass

    # MPS (Apple Silicon) metrics
    elif torch.backends.mps.is_available():
        try:
            # MPS doesn't expose detailed metrics like CUDA
            # But we can indicate it's being used (1 = available)
            metrics["gpu/mps_available"] = 1

            # Try to get memory info via system
            if HAS_PSUTIL:
                # On macOS, GPU shares system memory
                vm = psutil.virtual_memory()
                metrics["gpu/shared_memory_total_gb"] = vm.total / (1024 ** 3)
        except Exception:
            pass

    return metrics


def get_cpu_metrics() -> dict[str, float]:
    """Get CPU utilization metrics."""
    metrics = {}

    if not HAS_PSUTIL:
        return metrics

    try:
        # CPU utilization (percentage)
        cpu_percent = psutil.cpu_percent(interval=None)
        metrics["cpu/utilization_percent"] = cpu_percent

        # Per-core utilization (optional, can be verbose)
        # cpu_per_core = psutil.cpu_percent(percpu=True)
        # for i, pct in enumerate(cpu_per_core):
        #     metrics[f"cpu/core_{i}_percent"] = pct

        # CPU count
        metrics["cpu/count"] = psutil.cpu_count()
        metrics["cpu/count_physical"] = psutil.cpu_count(logical=False) or psutil.cpu_count()

        # Load average (Unix only)
        if hasattr(os, 'getloadavg'):
            load1, load5, load15 = os.getloadavg()
            metrics["cpu/load_1min"] = load1
            metrics["cpu/load_5min"] = load5
            metrics["cpu/load_15min"] = load15

    except Exception:
        pass

    return metrics


def get_memory_metrics() -> dict[str, float]:
    """Get system memory (RAM) metrics."""
    metrics = {}

    if not HAS_PSUTIL:
        return metrics

    try:
        vm = psutil.virtual_memory()
        metrics["memory/total_gb"] = vm.total / (1024 ** 3)
        metrics["memory/available_gb"] = vm.available / (1024 ** 3)
        metrics["memory/used_gb"] = vm.used / (1024 ** 3)
        metrics["memory/used_percent"] = vm.percent

        # Swap memory
        swap = psutil.swap_memory()
        metrics["memory/swap_total_gb"] = swap.total / (1024 ** 3)
        metrics["memory/swap_used_gb"] = swap.used / (1024 ** 3)
        metrics["memory/swap_percent"] = swap.percent

    except Exception:
        pass

    return metrics


# Store previous I/O counters for rate calculation
_prev_disk_io = None
_prev_net_io = None
_prev_io_time = None


def get_disk_io_metrics() -> dict[str, float]:
    """Get disk I/O metrics (read/write rates in MB/s)."""
    global _prev_disk_io, _prev_io_time

    metrics = {}

    if not HAS_PSUTIL:
        return metrics

    try:
        import time
        current_time = time.time()
        disk_io = psutil.disk_io_counters()

        if disk_io is None:
            return metrics

        if _prev_disk_io is not None and _prev_io_time is not None:
            time_delta = current_time - _prev_io_time
            if time_delta > 0:
                # Calculate rates in MB/s
                read_rate = (disk_io.read_bytes - _prev_disk_io.read_bytes) / time_delta / (1024 ** 2)
                write_rate = (disk_io.write_bytes - _prev_disk_io.write_bytes) / time_delta / (1024 ** 2)

                metrics["disk/read_mbps"] = max(0, read_rate)
                metrics["disk/write_mbps"] = max(0, write_rate)
                metrics["disk/io_mbps"] = max(0, read_rate + write_rate)

        # Store current values for next call
        _prev_disk_io = disk_io
        _prev_io_time = current_time

    except Exception:
        pass

    return metrics


def get_network_io_metrics() -> dict[str, float]:
    """Get network I/O metrics (send/receive rates in MB/s)."""
    global _prev_net_io, _prev_io_time

    metrics = {}

    if not HAS_PSUTIL:
        return metrics

    try:
        import time
        current_time = time.time()
        net_io = psutil.net_io_counters()

        if net_io is None:
            return metrics

        if _prev_net_io is not None and _prev_io_time is not None:
            time_delta = current_time - _prev_io_time
            if time_delta > 0:
                # Calculate rates in MB/s
                recv_rate = (net_io.bytes_recv - _prev_net_io.bytes_recv) / time_delta / (1024 ** 2)
                sent_rate = (net_io.bytes_sent - _prev_net_io.bytes_sent) / time_delta / (1024 ** 2)

                metrics["network/recv_mbps"] = max(0, recv_rate)
                metrics["network/sent_mbps"] = max(0, sent_rate)
                metrics["network/io_mbps"] = max(0, recv_rate + sent_rate)

        # Store current values for next call
        _prev_net_io = net_io
        _prev_io_time = current_time

    except Exception:
        pass

    return metrics


def get_system_metrics() -> dict[str, float]:
    """Get all system metrics (GPU, CPU, memory, disk I/O, network I/O).

    Returns:
        Dictionary of metric name -> numeric value (only floats/ints)
    """
    metrics = {}
    metrics.update(get_gpu_metrics())
    metrics.update(get_cpu_metrics())
    metrics.update(get_memory_metrics())
    metrics.update(get_disk_io_metrics())
    metrics.update(get_network_io_metrics())

    # Filter to only numeric values (MLRunX metrics must be numeric)
    return {k: v for k, v in metrics.items() if isinstance(v, (int, float))}



def log_system_metrics(run: "mlrunx.Run", step: int) -> dict[str, float]:
    """Collect and log system metrics to MLRunX.

    Args:
        run: MLRunX run instance
        step: Current training step

    Returns:
        Dictionary of logged metrics
    """
    metrics = get_system_metrics()
    if metrics:
        run.log(metrics, step=step)
    return metrics


# Device detection helpers
def get_device_info() -> dict[str, str | bool]:
    """Get device information for logging as parameters."""
    info = {
        "has_cuda": False,
        "has_mps": False,
        "device_type": "cpu",
    }

    if HAS_TORCH:
        info["has_cuda"] = torch.cuda.is_available()
        info["has_mps"] = torch.backends.mps.is_available()

        if info["has_cuda"]:
            info["device_type"] = "cuda"
            info["cuda_device_count"] = torch.cuda.device_count()
            info["cuda_device_name"] = torch.cuda.get_device_name(0)
        elif info["has_mps"]:
            info["device_type"] = "mps"

    if HAS_PSUTIL:
        info["cpu_count"] = psutil.cpu_count()
        info["ram_total_gb"] = round(psutil.virtual_memory().total / (1024 ** 3), 2)

    return info
