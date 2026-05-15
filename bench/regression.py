#!/usr/bin/env python3
"""Regression detection for benchmark results.

Compares current benchmark results against historical baselines
to detect performance regressions.

Usage:
    from bench.regression import RegressionDetector

    detector = RegressionDetector(thresholds_path="bench/ci_thresholds.yaml")
    regressions = detector.check(current_results, history_path="bench/results/history")
"""

from __future__ import annotations

import json
import logging
from dataclasses import dataclass
from datetime import datetime, timedelta
from pathlib import Path

import yaml

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)


@dataclass
class Regression:
    """Represents a detected regression."""

    workload: str
    metric: str
    current_value: float
    baseline_value: float
    threshold: float
    deviation_percent: float
    severity: str  # "warning" or "failure"

    def to_dict(self) -> dict:
        return {
            "workload": self.workload,
            "metric": self.metric,
            "current_value": round(self.current_value, 2),
            "baseline_value": round(self.baseline_value, 2),
            "threshold": round(self.threshold, 2),
            "deviation_percent": round(self.deviation_percent, 2),
            "severity": self.severity,
        }

    def __str__(self) -> str:
        return (
            f"{self.workload}/{self.metric}: {self.current_value:.2f} "
            f"(baseline: {self.baseline_value:.2f}, +{self.deviation_percent:.1f}%)"
        )


@dataclass
class RegressionConfig:
    """Configuration for regression detection."""

    allowable_deviation_percent: float = 20.0
    consecutive_failures: int = 2
    baseline_window_days: int = 7
    min_baseline_samples: int = 3


class RegressionDetector:
    """Detects performance regressions by comparing against historical baselines."""

    def __init__(
        self,
        thresholds_path: str | Path = "bench/ci_thresholds.yaml",
        config: RegressionConfig | None = None,
    ):
        self.thresholds_path = Path(thresholds_path)
        self.config = config or self._load_config()

    def _load_config(self) -> RegressionConfig:
        """Load regression config from thresholds file."""
        if not self.thresholds_path.exists():
            logger.warning(f"Thresholds file not found: {self.thresholds_path}")
            return RegressionConfig()

        with open(self.thresholds_path) as f:
            data = yaml.safe_load(f)

        reg_config = data.get("regression", {})
        return RegressionConfig(
            allowable_deviation_percent=reg_config.get("allowable_deviation_percent", 20.0),
            consecutive_failures=reg_config.get("consecutive_failures", 2),
            baseline_window_days=reg_config.get("baseline_window_days", 7),
            min_baseline_samples=reg_config.get("min_baseline_samples", 3),
        )

    def load_history(self, history_path: str | Path) -> list[dict]:
        """Load historical benchmark results.

        Args:
            history_path: Directory containing historical JSON files

        Returns:
            List of historical result dicts, sorted by timestamp
        """
        history_path = Path(history_path)
        if not history_path.exists():
            logger.warning(f"History directory not found: {history_path}")
            return []

        results = []
        cutoff = datetime.utcnow() - timedelta(days=self.config.baseline_window_days)

        for json_file in history_path.glob("*.json"):
            try:
                with open(json_file) as f:
                    data = json.load(f)

                # Parse timestamp
                ts_str = data.get("timestamp", "")
                if ts_str:
                    ts = datetime.fromisoformat(ts_str.replace("Z", "+00:00"))
                    if ts.replace(tzinfo=None) >= cutoff:
                        results.append(data)

            except (json.JSONDecodeError, ValueError) as e:
                logger.warning(f"Failed to parse {json_file}: {e}")

        # Sort by timestamp
        results.sort(key=lambda x: x.get("timestamp", ""))
        return results

    def calculate_baseline(
        self, history: list[dict], workload: str, metric: str
    ) -> float | None:
        """Calculate baseline value from historical data.

        Args:
            history: List of historical results
            workload: Workload name (e.g., "w1_run_scale")
            metric: Metric name (e.g., "list_runs.p95_ms")

        Returns:
            Baseline value (average of p95s), or None if insufficient data
        """
        values = []

        for result in history:
            if result.get("workload") != workload:
                continue

            # Navigate to metric value
            results_data = result.get("results", {})
            metric_parts = metric.split(".")

            value = results_data
            for part in metric_parts:
                if isinstance(value, dict):
                    value = value.get(part)
                else:
                    value = None
                    break

            if value is not None and isinstance(value, (int, float)):
                values.append(value)

        if len(values) < self.config.min_baseline_samples:
            return None

        # Use average of historical values as baseline
        return sum(values) / len(values)

    def check(
        self,
        current_results: dict | list[dict],
        history_path: str | Path = "bench/results/history",
    ) -> list[Regression]:
        """Check current results for regressions against historical baseline.

        Args:
            current_results: Current benchmark result(s)
            history_path: Path to historical results directory

        Returns:
            List of detected regressions
        """
        if isinstance(current_results, dict):
            current_results = [current_results]

        history = self.load_history(history_path)
        regressions = []

        for result in current_results:
            workload = result.get("workload", "")
            results_data = result.get("results", {})

            # Check each metric in the results
            for query_name, query_stats in results_data.items():
                if not isinstance(query_stats, dict):
                    continue

                # Check p95 values
                for stat_name in ["p95_ms", "p95"]:
                    current_value = query_stats.get(stat_name)
                    if current_value is None:
                        continue

                    metric = f"{query_name}.{stat_name}"
                    baseline = self.calculate_baseline(history, workload, metric)

                    if baseline is None:
                        logger.debug(f"No baseline for {workload}/{metric}")
                        continue

                    # Calculate deviation
                    if baseline > 0:
                        deviation_percent = ((current_value - baseline) / baseline) * 100
                    else:
                        deviation_percent = 0

                    # Check for regression
                    if deviation_percent > self.config.allowable_deviation_percent:
                        threshold = baseline * (1 + self.config.allowable_deviation_percent / 100)
                        severity = "failure" if deviation_percent > self.config.allowable_deviation_percent * 2 else "warning"

                        regressions.append(
                            Regression(
                                workload=workload,
                                metric=metric,
                                current_value=current_value,
                                baseline_value=baseline,
                                threshold=threshold,
                                deviation_percent=deviation_percent,
                                severity=severity,
                            )
                        )

        return regressions

    def format_report(self, regressions: list[Regression]) -> str:
        """Format regressions as a markdown report.

        Args:
            regressions: List of detected regressions

        Returns:
            Markdown-formatted report
        """
        if not regressions:
            return "## Regression Check: PASS\n\nNo regressions detected."

        lines = [
            "## Regression Check: FAIL",
            "",
            f"Detected {len(regressions)} regression(s):",
            "",
            "| Workload | Metric | Current | Baseline | Deviation | Severity |",
            "|----------|--------|---------|----------|-----------|----------|",
        ]

        for reg in regressions:
            lines.append(
                f"| {reg.workload} | {reg.metric} | {reg.current_value:.2f} | "
                f"{reg.baseline_value:.2f} | +{reg.deviation_percent:.1f}% | {reg.severity} |"
            )

        lines.extend([
            "",
            f"Allowable deviation: {self.config.allowable_deviation_percent}%",
        ])

        return "\n".join(lines)


def main():
    """CLI for regression detection."""
    import argparse

    parser = argparse.ArgumentParser(description="Check for benchmark regressions")
    parser.add_argument(
        "--results",
        required=True,
        help="Path to current results JSON file",
    )
    parser.add_argument(
        "--history",
        default="bench/results/history",
        help="Path to history directory",
    )
    parser.add_argument(
        "--thresholds",
        default="bench/ci_thresholds.yaml",
        help="Path to thresholds YAML",
    )
    parser.add_argument(
        "--fail-on-regression",
        action="store_true",
        help="Exit with error code on regression",
    )

    args = parser.parse_args()

    # Load current results
    with open(args.results) as f:
        current = json.load(f)

    # Check for regressions
    detector = RegressionDetector(thresholds_path=args.thresholds)
    regressions = detector.check(current, history_path=args.history)

    # Print report
    print(detector.format_report(regressions))

    # Exit code
    if args.fail_on_regression and regressions:
        import sys
        sys.exit(1)


if __name__ == "__main__":
    main()
