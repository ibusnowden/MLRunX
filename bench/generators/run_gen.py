#!/usr/bin/env python3
"""Synthetic run generator for benchmarking.

This generator creates realistic ML training runs using the MLRun Python SDK.
It tests the true hot path by going through all SDK batching and transport layers.

Usage:
    python -m bench.generators.run_gen --runs 1000
    python -m bench.generators.run_gen --config bench/generators/config.yaml --scale nightly
"""

from __future__ import annotations

import argparse
import json
import logging
import math
import random
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import yaml

# Add project root to path for imports
sys.path.insert(0, str(Path(__file__).parent.parent.parent))

try:
    import mlrun
except ImportError:
    # Fallback for running outside installed package
    sys.path.insert(0, str(Path(__file__).parent.parent.parent / "sdks" / "python" / "src"))
    import mlrun

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s - %(name)s - %(levelname)s - %(message)s",
)
logger = logging.getLogger(__name__)


@dataclass
class GeneratorConfig:
    """Configuration for the run generator."""

    runs: int = 1000
    metrics_per_run: int = 10
    points_per_metric: int = 100
    project: str = "benchmark"
    metric_patterns: dict = field(default_factory=dict)
    tags: dict = field(default_factory=dict)
    params: dict = field(default_factory=dict)


@dataclass
class TimingStats:
    """Timing statistics for a single run."""

    run_id: str
    init_time_ms: float
    log_time_ms: float
    finish_time_ms: float
    total_time_ms: float
    metrics_logged: int
    points_logged: int


@dataclass
class GeneratorStats:
    """Aggregate statistics from generation."""

    runs_generated: int
    total_metrics: int
    total_points: int
    total_time_s: float
    avg_run_time_ms: float
    avg_init_time_ms: float
    avg_log_time_ms: float
    avg_finish_time_ms: float
    runs_per_second: float
    points_per_second: float


def load_config(config_path: str | Path | None = None) -> GeneratorConfig:
    """Load configuration from YAML file.

    Args:
        config_path: Path to config file. Uses default if not provided.

    Returns:
        GeneratorConfig instance
    """
    if config_path is None:
        config_path = Path(__file__).parent / "config.yaml"

    config_path = Path(config_path)
    if not config_path.exists():
        logger.warning(f"Config file not found: {config_path}, using defaults")
        return GeneratorConfig()

    with open(config_path) as f:
        data = yaml.safe_load(f)

    defaults = data.get("defaults", {})
    return GeneratorConfig(
        runs=defaults.get("runs", 1000),
        metrics_per_run=defaults.get("metrics_per_run", 10),
        points_per_metric=defaults.get("points_per_metric", 100),
        project=defaults.get("project", "benchmark"),
        metric_patterns=data.get("metric_patterns", {}),
        tags=data.get("tags", {}),
        params=data.get("params", {}),
    )


class MetricGenerator:
    """Generates realistic metric values based on patterns."""

    def __init__(self, patterns: dict):
        self.patterns = patterns
        self._random_walk_state: dict[str, float] = {}

    def generate_value(
        self, metric_name: str, step: int, total_steps: int
    ) -> float:
        """Generate a metric value for a given step.

        Args:
            metric_name: Name of the metric
            step: Current step number
            total_steps: Total number of steps

        Returns:
            Generated metric value
        """
        # Get pattern for this metric or use default
        pattern = self.patterns.get(metric_name, {"type": "gaussian", "mean": 0.5, "std": 0.1})
        pattern_type = pattern.get("type", "gaussian")
        progress = step / max(total_steps - 1, 1)  # 0 to 1

        if pattern_type == "exponential_decay":
            start = pattern.get("start", 2.0)
            end = pattern.get("end", 0.1)
            noise = pattern.get("noise", 0.05)
            base = start * math.exp(-5 * progress) + end
            return base + random.gauss(0, noise)

        elif pattern_type == "sigmoid":
            start = pattern.get("start", 0.1)
            end = pattern.get("end", 0.95)
            inflection = pattern.get("inflection", 0.5)
            noise = pattern.get("noise", 0.02)
            x = (progress - inflection) * 10
            sigmoid = 1 / (1 + math.exp(-x))
            base = start + (end - start) * sigmoid
            return min(max(base + random.gauss(0, noise), 0), 1)

        elif pattern_type == "constant":
            value = pattern.get("value", 1.0)
            noise = pattern.get("noise", 0)
            return value + random.gauss(0, noise)

        elif pattern_type == "gaussian":
            mean = pattern.get("mean", 0.5)
            std = pattern.get("std", 0.1)
            return random.gauss(mean, std)

        elif pattern_type == "random_walk":
            key = metric_name
            if key not in self._random_walk_state:
                self._random_walk_state[key] = pattern.get("start", 0)
            max_delta = pattern.get("max_delta", 1)
            bounds = pattern.get("bounds", [float("-inf"), float("inf")])
            delta = random.uniform(-max_delta, max_delta)
            new_value = self._random_walk_state[key] + delta
            new_value = max(bounds[0], min(bounds[1], new_value))
            self._random_walk_state[key] = new_value
            return new_value

        else:
            # Default to random
            return random.random()

    def reset(self):
        """Reset state for new run."""
        self._random_walk_state.clear()


class RunGenerator:
    """Generates synthetic runs using the MLRun SDK.

    This class creates realistic ML training runs with configurable:
    - Number of runs
    - Metrics per run
    - Points per metric
    - Metric value patterns (loss decay, accuracy sigmoid, etc.)
    - Tag variety
    - Parameter distributions
    """

    def __init__(
        self,
        server_url: str = "http://localhost:3001",
        api_key: str | None = None,
        project: str = "benchmark",
        config: GeneratorConfig | None = None,
    ):
        """Initialize the generator.

        Args:
            server_url: MLRun API server URL
            api_key: Optional API key for authentication
            project: Project name for generated runs
            config: Generator configuration
        """
        self.server_url = server_url
        self.api_key = api_key
        self.project = project
        self.config = config or GeneratorConfig()
        self.metric_gen = MetricGenerator(self.config.metric_patterns)

        # Configure SDK
        mlrun.configure(server_url=server_url, api_key=api_key)

    def _select_random(self, options: list) -> Any:
        """Select a random value from options list."""
        return random.choice(options) if options else None

    def _generate_tags(self) -> dict[str, str]:
        """Generate random tags from configured options."""
        tags = {}
        for key, options in self.config.tags.items():
            if options:
                tags[key] = self._select_random(options)
        return tags

    def _generate_params(self) -> dict[str, Any]:
        """Generate random parameters from configured options."""
        params = {}
        for key, options in self.config.params.items():
            if options:
                params[key] = self._select_random(options)
        return params

    def _generate_metric_names(self, count: int) -> list[str]:
        """Generate metric names.

        Uses configured patterns first, then adds generic names.
        """
        names = list(self.config.metric_patterns.keys())
        while len(names) < count:
            names.append(f"metric_{len(names)}")
        return names[:count]

    def generate_run(
        self,
        metrics_per_run: int | None = None,
        points_per_metric: int | None = None,
        tags: dict[str, str] | None = None,
        run_name: str | None = None,
    ) -> TimingStats:
        """Generate a single synthetic run.

        Args:
            metrics_per_run: Number of metrics (uses config default if not provided)
            points_per_metric: Points per metric (uses config default if not provided)
            tags: Tags for the run (generates random if not provided)
            run_name: Optional run name

        Returns:
            TimingStats for the generated run
        """
        metrics_per_run = metrics_per_run or self.config.metrics_per_run
        points_per_metric = points_per_metric or self.config.points_per_metric

        self.metric_gen.reset()
        metric_names = self._generate_metric_names(metrics_per_run)
        run_tags = tags or self._generate_tags()
        run_params = self._generate_params()

        # Time init
        t_init_start = time.perf_counter()
        run = mlrun.init(
            project=self.project,
            name=run_name,
            tags=run_tags,
            config=run_params,
        )
        t_init_end = time.perf_counter()

        # Time logging
        t_log_start = time.perf_counter()
        for step in range(points_per_metric):
            metrics = {}
            for name in metric_names:
                metrics[name] = self.metric_gen.generate_value(
                    name, step, points_per_metric
                )
            run.log(metrics, step=step)
        t_log_end = time.perf_counter()

        # Time finish
        t_finish_start = time.perf_counter()
        run.finish()
        t_finish_end = time.perf_counter()

        init_time_ms = (t_init_end - t_init_start) * 1000
        log_time_ms = (t_log_end - t_log_start) * 1000
        finish_time_ms = (t_finish_end - t_finish_start) * 1000
        total_time_ms = init_time_ms + log_time_ms + finish_time_ms

        return TimingStats(
            run_id=run.run_id,
            init_time_ms=init_time_ms,
            log_time_ms=log_time_ms,
            finish_time_ms=finish_time_ms,
            total_time_ms=total_time_ms,
            metrics_logged=metrics_per_run,
            points_logged=metrics_per_run * points_per_metric,
        )

    def generate_batch(
        self,
        n_runs: int | None = None,
        metrics_per_run: int | None = None,
        points_per_metric: int | None = None,
        progress_callback: callable | None = None,
    ) -> GeneratorStats:
        """Generate a batch of synthetic runs.

        Args:
            n_runs: Number of runs to generate (uses config default if not provided)
            metrics_per_run: Metrics per run
            points_per_metric: Points per metric
            progress_callback: Optional callback(completed, total) for progress updates

        Returns:
            GeneratorStats with aggregate timing information
        """
        n_runs = n_runs or self.config.runs
        metrics_per_run = metrics_per_run or self.config.metrics_per_run
        points_per_metric = points_per_metric or self.config.points_per_metric

        logger.info(
            f"Generating {n_runs} runs with {metrics_per_run} metrics x {points_per_metric} points each"
        )

        timing_stats: list[TimingStats] = []
        t_start = time.perf_counter()

        for i in range(n_runs):
            stats = self.generate_run(
                metrics_per_run=metrics_per_run,
                points_per_metric=points_per_metric,
            )
            timing_stats.append(stats)

            if progress_callback:
                progress_callback(i + 1, n_runs)

            if (i + 1) % 100 == 0:
                logger.info(f"Generated {i + 1}/{n_runs} runs")

        t_end = time.perf_counter()
        total_time_s = t_end - t_start

        # Calculate aggregate stats
        total_points = sum(s.points_logged for s in timing_stats)
        avg_init = sum(s.init_time_ms for s in timing_stats) / len(timing_stats)
        avg_log = sum(s.log_time_ms for s in timing_stats) / len(timing_stats)
        avg_finish = sum(s.finish_time_ms for s in timing_stats) / len(timing_stats)
        avg_total = sum(s.total_time_ms for s in timing_stats) / len(timing_stats)

        return GeneratorStats(
            runs_generated=n_runs,
            total_metrics=n_runs * metrics_per_run,
            total_points=total_points,
            total_time_s=total_time_s,
            avg_run_time_ms=avg_total,
            avg_init_time_ms=avg_init,
            avg_log_time_ms=avg_log,
            avg_finish_time_ms=avg_finish,
            runs_per_second=n_runs / total_time_s,
            points_per_second=total_points / total_time_s,
        )


def main():
    """CLI entry point."""
    parser = argparse.ArgumentParser(
        description="Generate synthetic ML training runs for benchmarking",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument(
        "--runs", "-n",
        type=int,
        default=None,
        help="Number of runs to generate (overrides config)",
    )
    parser.add_argument(
        "--metrics",
        type=int,
        default=None,
        help="Metrics per run (overrides config)",
    )
    parser.add_argument(
        "--points",
        type=int,
        default=None,
        help="Points per metric (overrides config)",
    )
    parser.add_argument(
        "--config", "-c",
        type=str,
        default=None,
        help="Path to config YAML file",
    )
    parser.add_argument(
        "--scale",
        type=str,
        choices=["nightly", "release", "stress"],
        default=None,
        help="Use predefined scale configuration",
    )
    parser.add_argument(
        "--server-url",
        type=str,
        default="http://localhost:3001",
        help="MLRun API server URL",
    )
    parser.add_argument(
        "--api-key",
        type=str,
        default=None,
        help="API key for authentication",
    )
    parser.add_argument(
        "--project",
        type=str,
        default="benchmark",
        help="Project name for generated runs",
    )
    parser.add_argument(
        "--output", "-o",
        type=str,
        default=None,
        help="Output JSON file for stats",
    )
    parser.add_argument(
        "--quiet", "-q",
        action="store_true",
        help="Suppress progress output",
    )

    args = parser.parse_args()

    # Load config
    config = load_config(args.config)

    # Apply scale configuration if specified
    if args.scale:
        config_path = args.config or Path(__file__).parent / "config.yaml"
        with open(config_path) as f:
            full_config = yaml.safe_load(f)
        scale_config = full_config.get("scale", {}).get(args.scale, {})
        config.runs = scale_config.get("runs", config.runs)
        config.metrics_per_run = scale_config.get("metrics_per_run", config.metrics_per_run)
        config.points_per_metric = scale_config.get("points_per_metric", config.points_per_metric)

    # Override with CLI args
    if args.runs:
        config.runs = args.runs
    if args.metrics:
        config.metrics_per_run = args.metrics
    if args.points:
        config.points_per_metric = args.points
    if args.project:
        config.project = args.project

    # Set logging level
    if args.quiet:
        logging.getLogger().setLevel(logging.WARNING)

    # Create generator
    generator = RunGenerator(
        server_url=args.server_url,
        api_key=args.api_key,
        project=config.project,
        config=config,
    )

    # Progress callback
    def progress(completed, total):
        if not args.quiet and completed % 10 == 0:
            pct = completed / total * 100
            print(f"\rProgress: {completed}/{total} ({pct:.1f}%)", end="", flush=True)

    # Generate runs
    print(f"\nGenerating {config.runs} synthetic runs...")
    print(f"  Metrics per run: {config.metrics_per_run}")
    print(f"  Points per metric: {config.points_per_metric}")
    print(f"  Server: {args.server_url}")
    print()

    try:
        stats = generator.generate_batch(progress_callback=progress if not args.quiet else None)
    except Exception as e:
        logger.error(f"Generation failed: {e}")
        sys.exit(1)

    if not args.quiet:
        print("\n")

    # Print results
    print("=" * 60)
    print("Generation Complete")
    print("=" * 60)
    print(f"  Runs generated:     {stats.runs_generated:,}")
    print(f"  Total metrics:      {stats.total_metrics:,}")
    print(f"  Total points:       {stats.total_points:,}")
    print(f"  Total time:         {stats.total_time_s:.2f}s")
    print()
    print("Timing Statistics:")
    print(f"  Avg run time:       {stats.avg_run_time_ms:.2f}ms")
    print(f"  Avg init time:      {stats.avg_init_time_ms:.2f}ms")
    print(f"  Avg log time:       {stats.avg_log_time_ms:.2f}ms")
    print(f"  Avg finish time:    {stats.avg_finish_time_ms:.2f}ms")
    print()
    print("Throughput:")
    print(f"  Runs/second:        {stats.runs_per_second:.2f}")
    print(f"  Points/second:      {stats.points_per_second:,.0f}")
    print("=" * 60)

    # Write output file if specified
    if args.output:
        output_path = Path(args.output)
        output_path.parent.mkdir(parents=True, exist_ok=True)
        with open(output_path, "w") as f:
            json.dump(
                {
                    "generator": "run_gen",
                    "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                    "config": {
                        "runs": config.runs,
                        "metrics_per_run": config.metrics_per_run,
                        "points_per_metric": config.points_per_metric,
                        "project": config.project,
                        "server_url": args.server_url,
                    },
                    "stats": {
                        "runs_generated": stats.runs_generated,
                        "total_metrics": stats.total_metrics,
                        "total_points": stats.total_points,
                        "total_time_s": stats.total_time_s,
                        "avg_run_time_ms": stats.avg_run_time_ms,
                        "avg_init_time_ms": stats.avg_init_time_ms,
                        "avg_log_time_ms": stats.avg_log_time_ms,
                        "avg_finish_time_ms": stats.avg_finish_time_ms,
                        "runs_per_second": stats.runs_per_second,
                        "points_per_second": stats.points_per_second,
                    },
                },
                f,
                indent=2,
            )
        print(f"\nStats written to: {output_path}")


if __name__ == "__main__":
    main()
