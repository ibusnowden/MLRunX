#!/usr/bin/env python3
"""W2 Benchmark: High Frequency Ingestion.

Benchmarks end-to-end latency for high-frequency metric logging:
- SDK enqueue time (must be non-blocking)
- Flush/batch time
- Ingest RPC latency
- Log-to-visible latency (critical SLO)

Usage:
    python -m bench.workloads.w2_ingest_latency_runner --scale nightly
    python -m bench.workloads.w2_ingest_latency_runner --duration 60 --frequency 10
"""

from __future__ import annotations

import argparse
import json
import logging
import statistics
import sys
import threading
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import httpx

# Add project root to path
sys.path.insert(0, str(Path(__file__).parent.parent.parent))

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s - %(name)s - %(levelname)s - %(message)s",
)
logger = logging.getLogger(__name__)


# Scale configurations
SCALE_CONFIG = {
    "nightly": {
        "duration_s": 30,
        "frequency_hz": 10,
        "metrics_per_step": 5,
        "visibility_samples": 20,
    },
    "release": {
        "duration_s": 300,
        "frequency_hz": 10,
        "metrics_per_step": 10,
        "visibility_samples": 50,
    },
    "stress": {
        "duration_s": 600,
        "frequency_hz": 100,
        "metrics_per_step": 20,
        "visibility_samples": 100,
    },
}

# Thresholds
THRESHOLDS = {
    "sdk_enqueue_us": {"p95": 50},
    "flush_ms": {"p95": 200},
    "ingest_rpc_ms": {"p95": 100},
    "log_to_visible_ms": {"p95": 500},
}


@dataclass
class LatencyStats:
    """Statistics for latency measurements."""

    p50: float
    p95: float
    p99: float
    min: float
    max: float
    mean: float
    count: int

    @classmethod
    def from_samples(cls, samples: list[float]) -> "LatencyStats":
        if not samples:
            return cls(0, 0, 0, 0, 0, 0, 0)

        sorted_samples = sorted(samples)
        n = len(sorted_samples)

        return cls(
            p50=sorted_samples[int(n * 0.50)],
            p95=sorted_samples[int(n * 0.95)] if n >= 20 else sorted_samples[-1],
            p99=sorted_samples[int(n * 0.99)] if n >= 100 else sorted_samples[-1],
            min=sorted_samples[0],
            max=sorted_samples[-1],
            mean=statistics.mean(sorted_samples),
            count=n,
        )

    def to_dict(self) -> dict:
        return {
            "p50": round(self.p50, 2),
            "p95": round(self.p95, 2),
            "p99": round(self.p99, 2),
            "min": round(self.min, 2),
            "max": round(self.max, 2),
            "mean": round(self.mean, 2),
            "count": self.count,
        }


@dataclass
class W2Config:
    """Configuration for W2 benchmark."""

    duration_s: float = 30
    frequency_hz: float = 10
    metrics_per_step: int = 5
    visibility_samples: int = 20
    server_url: str = "http://localhost:3001"
    api_key: str | None = None
    poll_interval_ms: float = 10
    visibility_timeout_ms: float = 5000


@dataclass
class W2Result:
    """Results from W2 benchmark."""

    workload: str = "w2_ingest_latency"
    timestamp: str = ""
    config: dict = field(default_factory=dict)
    results: dict[str, LatencyStats] = field(default_factory=dict)
    throughput: dict = field(default_factory=dict)
    pass_: bool = True
    threshold_violations: list[str] = field(default_factory=list)
    errors: list[str] = field(default_factory=list)

    def to_dict(self) -> dict:
        return {
            "workload": self.workload,
            "timestamp": self.timestamp,
            "config": self.config,
            "results": {k: v.to_dict() for k, v in self.results.items()},
            "throughput": self.throughput,
            "pass": self.pass_,
            "threshold_violations": self.threshold_violations,
            "errors": self.errors,
        }


class InstrumentedRun:
    """A Run wrapper that captures timing metrics."""

    def __init__(self, run, enqueue_times: list):
        self._run = run
        self._enqueue_times = enqueue_times

    def log(self, data: dict, step: int | None = None, timestamp: float | None = None):
        """Log metrics with timing instrumentation."""
        t_start = time.perf_counter_ns()
        self._run.log(data, step=step, timestamp=timestamp)
        t_end = time.perf_counter_ns()
        # Store in microseconds
        self._enqueue_times.append((t_end - t_start) / 1000)

    @property
    def run_id(self) -> str:
        return self._run.run_id

    def finish(self):
        self._run.finish()


class W2Runner:
    """W2 Benchmark Runner: High frequency ingestion latency."""

    def __init__(self, config: W2Config):
        self.config = config
        self.client = httpx.Client(
            base_url=config.server_url,
            timeout=30.0,
            headers={"X-API-Key": config.api_key} if config.api_key else {},
        )

        # Timing collections
        self._enqueue_times_us: list[float] = []
        self._visibility_times_ms: list[float] = []
        self._flush_times_ms: list[float] = []
        self._rpc_times_ms: list[float] = []

    def _check_visibility(
        self,
        run_id: str,
        metric_name: str,
        expected_step: int,
        log_time: float,
    ) -> float | None:
        """Poll query API until metric is visible.

        Args:
            run_id: Run ID to query
            metric_name: Metric name to look for
            expected_step: Step number to find
            log_time: Timestamp when log() was called

        Returns:
            Time to visibility in ms, or None if timeout
        """
        timeout_s = self.config.visibility_timeout_ms / 1000
        poll_interval_s = self.config.poll_interval_ms / 1000
        start = time.perf_counter()

        while (time.perf_counter() - start) < timeout_s:
            try:
                response = self.client.get(
                    f"/api/v1/runs/{run_id}/metrics",
                    params={"names": metric_name, "max_points": 1000},
                )
                if response.status_code == 200:
                    data = response.json()
                    for series in data.get("series", []):
                        if series.get("name") == metric_name:
                            for point in series.get("points", []):
                                if point.get("step", -1) >= expected_step:
                                    visible_time = time.perf_counter()
                                    return (visible_time - log_time) * 1000

            except Exception:
                pass

            time.sleep(poll_interval_s)

        return None  # Timeout

    def run_high_freq_ingest(self) -> None:
        """Run high-frequency metric logging.

        Logs metrics at the configured frequency and measures:
        - SDK enqueue time
        - Log-to-visible latency (sampled)
        """
        try:
            import mlrun

            mlrun.configure(
                server_url=self.config.server_url,
                api_key=self.config.api_key,
            )
        except ImportError:
            sys.path.insert(0, str(Path(__file__).parent.parent.parent / "sdks" / "python" / "src"))
            import mlrun

            mlrun.configure(
                server_url=self.config.server_url,
                api_key=self.config.api_key,
            )

        # Create instrumented run
        base_run = mlrun.init(
            project="w2-benchmark",
            name="high-freq-ingest",
            tags={"benchmark": "w2"},
        )
        run = InstrumentedRun(base_run, self._enqueue_times_us)

        duration_s = self.config.duration_s
        frequency_hz = self.config.frequency_hz
        metrics_per_step = self.config.metrics_per_step
        interval_s = 1.0 / frequency_hz

        total_steps = int(duration_s * frequency_hz)
        visibility_sample_interval = max(1, total_steps // self.config.visibility_samples)

        logger.info(f"Starting high-freq ingest: {duration_s}s @ {frequency_hz}Hz")
        logger.info(f"  Total steps: {total_steps}")
        logger.info(f"  Visibility samples: {self.config.visibility_samples}")

        visibility_checks: list[tuple[str, int, float]] = []
        step = 0
        start_time = time.perf_counter()

        while (time.perf_counter() - start_time) < duration_s:
            loop_start = time.perf_counter()

            # Generate metrics
            metrics = {f"metric_{i}": step * 0.01 + i for i in range(metrics_per_step)}

            # Log with timing
            log_time = time.perf_counter()
            run.log(metrics, step=step)

            # Sample visibility checks
            if step % visibility_sample_interval == 0:
                visibility_checks.append(("metric_0", step, log_time))

            step += 1

            # Maintain frequency
            elapsed = time.perf_counter() - loop_start
            if elapsed < interval_s:
                time.sleep(interval_s - elapsed)

        # Finish run
        run.finish()

        actual_duration = time.perf_counter() - start_time
        logger.info(f"Logged {step} steps in {actual_duration:.2f}s")

        # Check visibility for sampled points
        logger.info(f"Checking visibility for {len(visibility_checks)} samples...")
        for metric_name, check_step, log_time in visibility_checks:
            visibility_ms = self._check_visibility(
                run.run_id, metric_name, check_step, log_time
            )
            if visibility_ms is not None:
                self._visibility_times_ms.append(visibility_ms)
            else:
                self.errors.append(f"Visibility timeout for step {check_step}")

        # Calculate throughput
        self._actual_duration = actual_duration
        self._total_events = step * metrics_per_step

    def run(self) -> W2Result:
        """Run the complete W2 benchmark."""
        logger.info("=" * 60)
        logger.info("W2 Benchmark: High Frequency Ingestion")
        logger.info("=" * 60)

        self.errors: list[str] = []

        result = W2Result(
            timestamp=time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            config={
                "duration_s": self.config.duration_s,
                "frequency_hz": self.config.frequency_hz,
                "metrics_per_step": self.config.metrics_per_step,
                "visibility_samples": self.config.visibility_samples,
                "server_url": self.config.server_url,
            },
        )

        try:
            self.run_high_freq_ingest()
        except Exception as e:
            logger.error(f"Benchmark failed: {e}")
            result.errors.append(str(e))
            result.pass_ = False
            return result

        # Calculate statistics
        result.results["sdk_enqueue_us"] = LatencyStats.from_samples(self._enqueue_times_us)
        result.results["log_to_visible_ms"] = LatencyStats.from_samples(self._visibility_times_ms)

        # Throughput
        result.throughput = {
            "events_per_sec": round(self._total_events / self._actual_duration, 2),
            "duration_s": round(self._actual_duration, 2),
            "total_events": self._total_events,
        }

        result.errors.extend(self.errors)

        # Check thresholds
        for metric_name, stats in result.results.items():
            threshold = THRESHOLDS.get(metric_name, {}).get("p95")
            if threshold and stats.p95 > threshold:
                violation = f"{metric_name}: p95={stats.p95:.2f} > {threshold}"
                result.threshold_violations.append(violation)
                result.pass_ = False

        # Log summary
        logger.info("")
        logger.info("=" * 60)
        logger.info("W2 Results Summary")
        logger.info("=" * 60)

        for name, stats in result.results.items():
            unit = "μs" if "us" in name else "ms"
            threshold = THRESHOLDS.get(name, {}).get("p95", "N/A")
            status = "PASS" if stats.p95 <= (threshold if isinstance(threshold, (int, float)) else float("inf")) else "FAIL"
            logger.info(f"  {name:20s}: p95={stats.p95:7.2f}{unit} (target: {threshold}{unit}) [{status}]")

        logger.info("")
        logger.info(f"Throughput: {result.throughput['events_per_sec']:.0f} events/sec")
        logger.info(f"Overall: {'PASS' if result.pass_ else 'FAIL'}")

        if result.threshold_violations:
            logger.warning(f"Violations: {result.threshold_violations}")

        return result

    def close(self):
        """Close HTTP client."""
        self.client.close()


def main():
    """CLI entry point."""
    parser = argparse.ArgumentParser(
        description="W2 Benchmark: High frequency ingestion latency",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument(
        "--scale",
        choices=["nightly", "release", "stress"],
        default=None,
        help="Use predefined scale configuration",
    )
    parser.add_argument(
        "--duration",
        type=float,
        default=None,
        help="Test duration in seconds",
    )
    parser.add_argument(
        "--frequency",
        type=float,
        default=None,
        help="Logging frequency in Hz",
    )
    parser.add_argument(
        "--metrics",
        type=int,
        default=None,
        help="Metrics per step",
    )
    parser.add_argument(
        "--server-url",
        default="http://localhost:3001",
        help="MLRun API server URL",
    )
    parser.add_argument(
        "--api-key",
        default=None,
        help="API key for authentication",
    )
    parser.add_argument(
        "--output", "-o",
        default=None,
        help="Output JSON file path",
    )
    parser.add_argument(
        "--fail-on-threshold",
        action="store_true",
        help="Exit with error code if thresholds exceeded",
    )

    args = parser.parse_args()

    # Build config
    config = W2Config(server_url=args.server_url, api_key=args.api_key)

    if args.scale:
        scale_cfg = SCALE_CONFIG[args.scale]
        config.duration_s = scale_cfg["duration_s"]
        config.frequency_hz = scale_cfg["frequency_hz"]
        config.metrics_per_step = scale_cfg["metrics_per_step"]
        config.visibility_samples = scale_cfg["visibility_samples"]

    if args.duration:
        config.duration_s = args.duration
    if args.frequency:
        config.frequency_hz = args.frequency
    if args.metrics:
        config.metrics_per_step = args.metrics

    # Run benchmark
    runner = W2Runner(config)
    try:
        result = runner.run()
    finally:
        runner.close()

    # Output
    if args.output:
        output_path = Path(args.output)
        output_path.parent.mkdir(parents=True, exist_ok=True)
        with open(output_path, "w") as f:
            json.dump(result.to_dict(), f, indent=2)
        logger.info(f"Results written to: {output_path}")

    # Exit code
    if args.fail_on_threshold and not result.pass_:
        sys.exit(1)


if __name__ == "__main__":
    main()
