#!/usr/bin/env python3
"""W1 Benchmark: Run Count Scale.

Benchmarks query performance at scale:
- Populates database with N runs
- Executes query workload (list, filter, search, compare)
- Measures p50/p95/p99 latencies
- Outputs JSON report

Usage:
    python -m bench.workloads.w1_run_scale_runner --scale nightly
    python -m bench.workloads.w1_run_scale_runner --runs 5000 --queries 100
"""

from __future__ import annotations

import argparse
import json
import logging
import statistics
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable

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
        "runs": 1000,
        "queries_per_type": 100,
    },
    "release": {
        "runs": 10000,
        "queries_per_type": 100,
    },
    "stress": {
        "runs": 100000,
        "queries_per_type": 1000,
    },
}

# Thresholds (ms)
THRESHOLDS = {
    "list_runs": {"p95_ms": 200},
    "filter_tag": {"p95_ms": 150},
    "filter_status": {"p95_ms": 150},
    "search": {"p95_ms": 200},
    "compare_runs": {"p95_ms": 300},
}


@dataclass
class LatencyStats:
    """Statistics for a set of latency measurements."""

    p50_ms: float
    p95_ms: float
    p99_ms: float
    min_ms: float
    max_ms: float
    mean_ms: float
    count: int

    @classmethod
    def from_samples(cls, samples_ms: list[float]) -> "LatencyStats":
        """Calculate stats from raw samples."""
        if not samples_ms:
            return cls(0, 0, 0, 0, 0, 0, 0)

        sorted_samples = sorted(samples_ms)
        n = len(sorted_samples)

        return cls(
            p50_ms=sorted_samples[int(n * 0.50)],
            p95_ms=sorted_samples[int(n * 0.95)] if n >= 20 else sorted_samples[-1],
            p99_ms=sorted_samples[int(n * 0.99)] if n >= 100 else sorted_samples[-1],
            min_ms=sorted_samples[0],
            max_ms=sorted_samples[-1],
            mean_ms=statistics.mean(sorted_samples),
            count=n,
        )

    def to_dict(self) -> dict:
        return {
            "p50_ms": round(self.p50_ms, 2),
            "p95_ms": round(self.p95_ms, 2),
            "p99_ms": round(self.p99_ms, 2),
            "min_ms": round(self.min_ms, 2),
            "max_ms": round(self.max_ms, 2),
            "mean_ms": round(self.mean_ms, 2),
            "count": self.count,
        }


@dataclass
class W1Config:
    """Configuration for W1 benchmark."""

    runs: int = 1000
    queries_per_type: int = 100
    server_url: str = "http://localhost:3001"
    api_key: str | None = None
    timeout_s: float = 30.0
    skip_setup: bool = False


@dataclass
class W1Result:
    """Results from W1 benchmark."""

    workload: str = "w1_run_scale"
    timestamp: str = ""
    config: dict = field(default_factory=dict)
    results: dict[str, LatencyStats] = field(default_factory=dict)
    pass_: bool = True
    threshold_violations: list[str] = field(default_factory=list)
    errors: list[str] = field(default_factory=list)

    def to_dict(self) -> dict:
        return {
            "workload": self.workload,
            "timestamp": self.timestamp,
            "config": self.config,
            "results": {k: v.to_dict() for k, v in self.results.items()},
            "pass": self.pass_,
            "threshold_violations": self.threshold_violations,
            "errors": self.errors,
        }


class W1Runner:
    """W1 Benchmark Runner: Query performance at scale."""

    def __init__(self, config: W1Config):
        self.config = config
        self.client = httpx.Client(
            base_url=config.server_url,
            timeout=config.timeout_s,
            headers={"X-API-Key": config.api_key} if config.api_key else {},
        )
        self._run_ids: list[str] = []
        self._tags: list[str] = ["model:resnet50", "model:bert-base", "dataset:imagenet"]

    def setup(self) -> bool:
        """Populate database with synthetic runs.

        Returns:
            True if setup successful
        """
        if self.config.skip_setup:
            logger.info("Skipping setup (--skip-setup flag)")
            return self._load_existing_runs()

        logger.info(f"Setting up W1 benchmark with {self.config.runs} runs...")

        try:
            # Try to use the generator if available
            from bench.generators.run_gen import RunGenerator, load_config

            gen_config = load_config()
            gen_config.runs = self.config.runs
            gen_config.metrics_per_run = 5
            gen_config.points_per_metric = 50

            generator = RunGenerator(
                server_url=self.config.server_url,
                api_key=self.config.api_key,
                config=gen_config,
            )

            stats = generator.generate_batch(n_runs=self.config.runs)
            logger.info(f"Generated {stats.runs_generated} runs in {stats.total_time_s:.2f}s")

        except ImportError:
            logger.warning("Generator not available, using inline setup")
            return self._inline_setup()

        return self._load_existing_runs()

    def _inline_setup(self) -> bool:
        """Fallback setup without generator module."""
        try:
            import mlrun

            mlrun.configure(
                server_url=self.config.server_url,
                api_key=self.config.api_key,
            )

            for i in range(self.config.runs):
                tags = {"model": ["resnet50", "bert-base", "gpt2"][i % 3]}
                run = mlrun.init(project="w1-benchmark", tags=tags)
                for step in range(50):
                    run.log({"loss": 1.0 / (step + 1)}, step=step)
                run.finish()

                if (i + 1) % 100 == 0:
                    logger.info(f"Setup progress: {i + 1}/{self.config.runs}")

            return self._load_existing_runs()

        except Exception as e:
            logger.error(f"Inline setup failed: {e}")
            return False

    def _load_existing_runs(self) -> bool:
        """Load existing run IDs from the server."""
        try:
            response = self.client.get("/api/v1/runs", params={"limit": 1000})
            response.raise_for_status()
            data = response.json()
            self._run_ids = [r["run_id"] for r in data.get("runs", [])]
            logger.info(f"Loaded {len(self._run_ids)} existing runs")
            return len(self._run_ids) > 0
        except Exception as e:
            logger.error(f"Failed to load runs: {e}")
            return False

    def _measure_query(
        self, name: str, query_fn: Callable[[], httpx.Response], iterations: int
    ) -> tuple[LatencyStats, list[str]]:
        """Measure query latency over multiple iterations.

        Args:
            name: Query name for logging
            query_fn: Function that executes the query
            iterations: Number of iterations

        Returns:
            Tuple of (LatencyStats, list of errors)
        """
        latencies: list[float] = []
        errors: list[str] = []

        for i in range(iterations):
            try:
                t_start = time.perf_counter()
                response = query_fn()
                t_end = time.perf_counter()

                if response.status_code == 200:
                    latencies.append((t_end - t_start) * 1000)
                else:
                    errors.append(f"{name}[{i}]: HTTP {response.status_code}")

            except httpx.TimeoutException:
                errors.append(f"{name}[{i}]: Timeout")
            except Exception as e:
                errors.append(f"{name}[{i}]: {str(e)}")

        return LatencyStats.from_samples(latencies), errors

    def run_workload(self) -> W1Result:
        """Execute the W1 query workload.

        Returns:
            W1Result with latency statistics
        """
        result = W1Result(
            timestamp=time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            config={
                "runs": self.config.runs,
                "queries_per_type": self.config.queries_per_type,
                "server_url": self.config.server_url,
            },
        )

        queries_per_type = self.config.queries_per_type

        # 1. List runs
        logger.info("Running list_runs queries...")
        stats, errors = self._measure_query(
            "list_runs",
            lambda: self.client.get("/api/v1/runs", params={"limit": 100}),
            queries_per_type,
        )
        result.results["list_runs"] = stats
        result.errors.extend(errors)

        # 2. Filter by tag
        logger.info("Running filter_tag queries...")
        stats, errors = self._measure_query(
            "filter_tag",
            lambda: self.client.get(
                "/api/v1/runs",
                params={"tags": self._tags[hash(str(time.time())) % len(self._tags)]},
            ),
            queries_per_type,
        )
        result.results["filter_tag"] = stats
        result.errors.extend(errors)

        # 3. Filter by status
        logger.info("Running filter_status queries...")
        stats, errors = self._measure_query(
            "filter_status",
            lambda: self.client.get("/api/v1/runs", params={"status": "finished"}),
            queries_per_type,
        )
        result.results["filter_status"] = stats
        result.errors.extend(errors)

        # 4. Search
        logger.info("Running search queries...")
        stats, errors = self._measure_query(
            "search",
            lambda: self.client.get("/api/v1/runs", params={"q": "benchmark"}),
            queries_per_type,
        )
        result.results["search"] = stats
        result.errors.extend(errors)

        # 5. Compare runs
        if len(self._run_ids) >= 10:
            logger.info("Running compare_runs queries...")
            import random

            def compare_query():
                run_ids = random.sample(self._run_ids, min(10, len(self._run_ids)))
                return self.client.post(
                    "/api/v1/runs/compare",
                    json={"run_ids": run_ids},
                )

            stats, errors = self._measure_query(
                "compare_runs",
                compare_query,
                queries_per_type,
            )
            result.results["compare_runs"] = stats
            result.errors.extend(errors)
        else:
            logger.warning("Not enough runs for compare_runs benchmark")

        # Check thresholds
        for query_name, stats in result.results.items():
            threshold = THRESHOLDS.get(query_name, {}).get("p95_ms")
            if threshold and stats.p95_ms > threshold:
                violation = f"{query_name}: p95={stats.p95_ms:.2f}ms > {threshold}ms"
                result.threshold_violations.append(violation)
                result.pass_ = False

        if result.errors:
            logger.warning(f"Encountered {len(result.errors)} errors during benchmark")

        return result

    def run(self) -> W1Result:
        """Run the complete W1 benchmark.

        Returns:
            W1Result with all statistics
        """
        logger.info("=" * 60)
        logger.info("W1 Benchmark: Run Count Scale")
        logger.info("=" * 60)

        # Setup
        if not self.setup():
            return W1Result(
                timestamp=time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                pass_=False,
                errors=["Setup failed - no runs available"],
            )

        # Run workload
        result = self.run_workload()

        # Log summary
        logger.info("")
        logger.info("=" * 60)
        logger.info("W1 Results Summary")
        logger.info("=" * 60)
        for name, stats in result.results.items():
            threshold = THRESHOLDS.get(name, {}).get("p95_ms", "N/A")
            status = "PASS" if stats.p95_ms <= (threshold if isinstance(threshold, (int, float)) else float("inf")) else "FAIL"
            logger.info(f"  {name:20s}: p95={stats.p95_ms:7.2f}ms (target: {threshold}ms) [{status}]")

        logger.info("")
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
        description="W1 Benchmark: Query performance at scale",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument(
        "--scale",
        choices=["nightly", "release", "stress"],
        default=None,
        help="Use predefined scale configuration",
    )
    parser.add_argument(
        "--runs",
        type=int,
        default=None,
        help="Number of runs to populate (overrides scale)",
    )
    parser.add_argument(
        "--queries",
        type=int,
        default=None,
        help="Number of queries per type (overrides scale)",
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
        "--skip-setup",
        action="store_true",
        help="Skip data population (use existing runs)",
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
    config = W1Config(server_url=args.server_url, api_key=args.api_key)

    if args.scale:
        scale_cfg = SCALE_CONFIG[args.scale]
        config.runs = scale_cfg["runs"]
        config.queries_per_type = scale_cfg["queries_per_type"]

    if args.runs:
        config.runs = args.runs
    if args.queries:
        config.queries_per_type = args.queries
    config.skip_setup = args.skip_setup

    # Run benchmark
    runner = W1Runner(config)
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
