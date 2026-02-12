#!/usr/bin/env python3
"""Nightly benchmark runner and CI orchestrator.

Orchestrates benchmark execution for CI pipelines:
- Spins up infrastructure (if needed)
- Runs W1, W2, W3 benchmarks
- Checks against thresholds
- Detects regressions
- Generates reports

Usage:
    # Run nightly benchmarks
    python bench/run_nightly.py --scale nightly

    # Run release benchmarks with regression check
    python bench/run_nightly.py --scale release --fail-on-regression

    # Check thresholds only (no execution)
    python bench/run_nightly.py check-threshold --workload w1 --results bench/results/w1.json
"""

from __future__ import annotations

import argparse
import json
import logging
import os
import shutil
import subprocess
import sys
import time
from datetime import datetime
from pathlib import Path
from typing import Any

import yaml

# Add project root to path
sys.path.insert(0, str(Path(__file__).parent.parent))

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s - %(name)s - %(levelname)s - %(message)s",
)
logger = logging.getLogger(__name__)


class NightlyRunner:
    """Orchestrates nightly benchmark runs."""

    def __init__(
        self,
        thresholds_path: str | Path = "bench/ci_thresholds.yaml",
        output_dir: str | Path = "bench/results",
        server_url: str = "http://localhost:3001",
        api_key: str | None = None,
    ):
        self.thresholds_path = Path(thresholds_path)
        self.output_dir = Path(output_dir)
        self.server_url = server_url
        self.api_key = api_key
        self.thresholds = self._load_thresholds()

        # Ensure output directories exist
        self.output_dir.mkdir(parents=True, exist_ok=True)
        (self.output_dir / "history").mkdir(parents=True, exist_ok=True)

    def _load_thresholds(self) -> dict:
        """Load thresholds configuration."""
        if not self.thresholds_path.exists():
            logger.warning(f"Thresholds file not found: {self.thresholds_path}")
            return {}

        with open(self.thresholds_path) as f:
            return yaml.safe_load(f)

    def check_infrastructure(self) -> bool:
        """Check if required infrastructure is running.

        Returns:
            True if all services are healthy
        """
        import httpx

        try:
            response = httpx.get(f"{self.server_url}/health", timeout=5)
            if response.status_code == 200:
                logger.info("API server is healthy")
                return True
        except Exception as e:
            logger.warning(f"API server not reachable: {e}")

        return False

    def spin_up_stack(self, wait_timeout: int = 120) -> bool:
        """Start the MLRunX infrastructure stack.

        Args:
            wait_timeout: Seconds to wait for services to be healthy

        Returns:
            True if stack started successfully
        """
        logger.info("Starting MLRunX infrastructure...")

        compose_file = Path("infra/docker/docker-compose.yml")
        if not compose_file.exists():
            logger.error(f"Docker compose file not found: {compose_file}")
            return False

        try:
            subprocess.run(
                ["docker", "compose", "-f", str(compose_file), "up", "-d"],
                check=True,
                capture_output=True,
            )
        except subprocess.CalledProcessError as e:
            logger.error(f"Failed to start stack: {e.stderr.decode()}")
            return False

        # Wait for services to be healthy
        logger.info(f"Waiting for services (timeout: {wait_timeout}s)...")
        start_time = time.time()
        while time.time() - start_time < wait_timeout:
            if self.check_infrastructure():
                return True
            time.sleep(5)

        logger.error("Timeout waiting for services")
        return False

    def run_benchmark(
        self,
        workload: str,
        scale: str = "nightly",
    ) -> dict | None:
        """Run a single benchmark workload.

        Args:
            workload: Workload name (w1, w2, w3)
            scale: Scale configuration name

        Returns:
            Benchmark results dict, or None on failure
        """
        logger.info(f"Running {workload} benchmark (scale: {scale})...")

        # Build command
        if workload == "w1":
            cmd = [
                sys.executable, "-m", "bench.workloads.w1_run_scale_runner",
                "--scale", scale,
                "--server-url", self.server_url,
                "--output", str(self.output_dir / f"{workload}_{scale}.json"),
            ]
        elif workload == "w2":
            cmd = [
                sys.executable, "-m", "bench.workloads.w2_ingest_latency_runner",
                "--scale", scale,
                "--server-url", self.server_url,
                "--output", str(self.output_dir / f"{workload}_{scale}.json"),
            ]
        elif workload == "w3":
            cmd = [
                sys.executable, "-m", "bench.workloads.w3_mixed_dashboard_runner",
                "--scale", scale,
                "--server-url", self.server_url,
                "--output", str(self.output_dir / f"{workload}_{scale}.json"),
            ]
        else:
            logger.warning(f"Unknown workload: {workload}")
            return None

        if self.api_key:
            cmd.extend(["--api-key", self.api_key])

        try:
            result = subprocess.run(cmd, capture_output=True, text=True, timeout=1800)
            if result.returncode != 0:
                logger.error(f"Benchmark failed: {result.stderr}")
                return None

            # Load results
            results_path = self.output_dir / f"{workload}_{scale}.json"
            if results_path.exists():
                with open(results_path) as f:
                    return json.load(f)

        except subprocess.TimeoutExpired:
            logger.error(f"Benchmark {workload} timed out")
        except Exception as e:
            logger.error(f"Benchmark {workload} failed: {e}")

        return None

    def check_thresholds(self, results: dict) -> tuple[bool, list[str]]:
        """Check benchmark results against thresholds.

        Args:
            results: Benchmark results dict

        Returns:
            Tuple of (passed, list of violations)
        """
        violations = []
        workload = results.get("workload", "")
        workload_thresholds = self.thresholds.get("workloads", {}).get(workload, {})

        for query_name, query_results in results.get("results", {}).items():
            if not isinstance(query_results, dict):
                continue

            query_thresholds = workload_thresholds.get(query_name, {})

            for metric_name, threshold in query_thresholds.items():
                current_value = query_results.get(metric_name)
                if current_value is not None and current_value > threshold:
                    violations.append(
                        f"{workload}/{query_name}/{metric_name}: "
                        f"{current_value:.2f} > {threshold}"
                    )

        return len(violations) == 0, violations

    def save_to_history(self, results: dict) -> None:
        """Save results to history for regression tracking.

        Args:
            results: Benchmark results dict
        """
        timestamp = datetime.utcnow().strftime("%Y%m%d_%H%M%S")
        workload = results.get("workload", "unknown")
        filename = f"{workload}_{timestamp}.json"

        history_path = self.output_dir / "history" / filename
        with open(history_path, "w") as f:
            json.dump(results, f, indent=2)

        logger.info(f"Saved to history: {history_path}")

    def generate_summary(
        self,
        all_results: list[dict],
        all_violations: list[str],
        regressions: list = None,
    ) -> str:
        """Generate markdown summary report.

        Args:
            all_results: List of all benchmark results
            all_violations: List of threshold violations
            regressions: List of detected regressions

        Returns:
            Markdown report string
        """
        lines = [
            "# MLRunX Benchmark Report",
            "",
            f"**Date:** {datetime.utcnow().strftime('%Y-%m-%d %H:%M:%S UTC')}",
            "",
        ]

        # Overall status
        passed = len(all_violations) == 0 and (not regressions or len(regressions) == 0)
        status = "PASS" if passed else "FAIL"
        lines.extend([
            f"## Overall Status: {status}",
            "",
        ])

        # Results summary
        lines.extend([
            "## Benchmark Results",
            "",
            "| Workload | Metric | p50 | p95 | p99 | Status |",
            "|----------|--------|-----|-----|-----|--------|",
        ])

        for results in all_results:
            workload = results.get("workload", "")
            for query_name, query_results in results.get("results", {}).items():
                if not isinstance(query_results, dict):
                    continue

                p50 = query_results.get("p50_ms", query_results.get("p50", "N/A"))
                p95 = query_results.get("p95_ms", query_results.get("p95", "N/A"))
                p99 = query_results.get("p99_ms", query_results.get("p99", "N/A"))

                # Check if this metric has a violation
                metric_status = "PASS"
                for v in all_violations:
                    if f"{workload}/{query_name}" in v:
                        metric_status = "FAIL"
                        break

                lines.append(
                    f"| {workload} | {query_name} | {p50} | {p95} | {p99} | {metric_status} |"
                )

        # Violations
        if all_violations:
            lines.extend([
                "",
                "## Threshold Violations",
                "",
            ])
            for v in all_violations:
                lines.append(f"- {v}")

        # Regressions
        if regressions:
            lines.extend([
                "",
                "## Regressions Detected",
                "",
            ])
            for reg in regressions:
                lines.append(f"- {reg}")

        return "\n".join(lines)

    def run(
        self,
        scale: str = "nightly",
        workloads: list[str] | None = None,
        fail_on_threshold: bool = True,
        fail_on_regression: bool = False,
        skip_infrastructure_check: bool = False,
    ) -> bool:
        """Run the complete nightly benchmark suite.

        Args:
            scale: Scale configuration name
            workloads: List of workloads to run (default: all)
            fail_on_threshold: Return False if thresholds exceeded
            fail_on_regression: Return False if regressions detected
            skip_infrastructure_check: Skip infrastructure startup

        Returns:
            True if all checks passed
        """
        logger.info("=" * 60)
        logger.info("MLRunX Nightly Benchmark Suite")
        logger.info("=" * 60)
        logger.info(f"Scale: {scale}")

        # Check/start infrastructure
        if not skip_infrastructure_check:
            if not self.check_infrastructure():
                if not self.spin_up_stack():
                    logger.error("Failed to start infrastructure")
                    return False

        # Determine workloads
        if workloads is None:
            workloads = ["w1", "w2", "w3"]

        # Run benchmarks
        all_results = []
        all_violations = []

        for workload in workloads:
            results = self.run_benchmark(workload, scale)
            if results:
                all_results.append(results)

                # Check thresholds
                passed, violations = self.check_thresholds(results)
                all_violations.extend(violations)

                # Save to history
                self.save_to_history(results)

        # Check for regressions
        regressions = []
        if fail_on_regression:
            from bench.regression import RegressionDetector

            detector = RegressionDetector(thresholds_path=self.thresholds_path)
            for results in all_results:
                regressions.extend(detector.check(results))

        # Generate summary
        summary = self.generate_summary(all_results, all_violations, regressions)
        print("\n" + summary)

        # Save summary
        summary_path = self.output_dir / f"summary_{scale}.md"
        with open(summary_path, "w") as f:
            f.write(summary)
        logger.info(f"Summary saved to: {summary_path}")

        # Determine overall pass/fail
        passed = True
        if fail_on_threshold and all_violations:
            logger.error(f"Threshold violations: {len(all_violations)}")
            passed = False
        if fail_on_regression and regressions:
            logger.error(f"Regressions detected: {len(regressions)}")
            passed = False

        return passed


def main():
    """CLI entry point."""
    parser = argparse.ArgumentParser(
        description="MLRunX Nightly Benchmark Runner",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )

    subparsers = parser.add_subparsers(dest="command", help="Command")

    # Run command (default)
    run_parser = subparsers.add_parser("run", help="Run benchmarks")
    run_parser.add_argument(
        "--scale",
        choices=["nightly", "release", "stress"],
        default="nightly",
        help="Scale configuration",
    )
    run_parser.add_argument(
        "--workloads",
        nargs="+",
        choices=["w1", "w2", "w3"],
        default=None,
        help="Workloads to run",
    )
    run_parser.add_argument(
        "--server-url",
        default="http://localhost:3001",
        help="MLRunX API server URL",
    )
    run_parser.add_argument(
        "--api-key",
        default=None,
        help="API key",
    )
    run_parser.add_argument(
        "--output-dir",
        default="bench/results",
        help="Output directory",
    )
    run_parser.add_argument(
        "--fail-on-threshold",
        action="store_true",
        default=True,
        help="Exit with error if thresholds exceeded",
    )
    run_parser.add_argument(
        "--fail-on-regression",
        action="store_true",
        help="Exit with error if regressions detected",
    )
    run_parser.add_argument(
        "--skip-infra-check",
        action="store_true",
        help="Skip infrastructure check/startup",
    )

    # Check threshold command
    check_parser = subparsers.add_parser("check-threshold", help="Check thresholds")
    check_parser.add_argument("--results", required=True, help="Results JSON file")
    check_parser.add_argument("--thresholds", default="bench/ci_thresholds.yaml")

    args = parser.parse_args()

    # Default to run command
    if args.command is None:
        args.command = "run"
        args.scale = "nightly"
        args.workloads = None
        args.server_url = "http://localhost:3001"
        args.api_key = None
        args.output_dir = "bench/results"
        args.fail_on_threshold = True
        args.fail_on_regression = False
        args.skip_infra_check = False

    if args.command == "run":
        runner = NightlyRunner(
            output_dir=args.output_dir,
            server_url=args.server_url,
            api_key=args.api_key,
        )
        passed = runner.run(
            scale=args.scale,
            workloads=args.workloads,
            fail_on_threshold=args.fail_on_threshold,
            fail_on_regression=args.fail_on_regression,
            skip_infrastructure_check=args.skip_infra_check,
        )
        sys.exit(0 if passed else 1)

    elif args.command == "check-threshold":
        with open(args.results) as f:
            results = json.load(f)

        runner = NightlyRunner(thresholds_path=args.thresholds)
        passed, violations = runner.check_thresholds(results)

        if passed:
            print("Threshold check: PASS")
        else:
            print("Threshold check: FAIL")
            for v in violations:
                print(f"  - {v}")

        sys.exit(0 if passed else 1)


if __name__ == "__main__":
    main()
