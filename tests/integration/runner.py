#!/usr/bin/env python3
"""
MLRunX Integration Test Runner

Runs end-to-end scenarios against the ephemeral test stack.
See: TEST-002 for specification.

Usage:
    # Start the test stack first:
    docker compose -f docker-compose.test.yml up -d --wait

    # Run tests:
    python runner.py

    # Or run specific tests:
    python runner.py --test test_sdk_basic_logging

    # Cleanup:
    docker compose -f docker-compose.test.yml down -v
"""

import argparse
import json
import os
import sys
import time
import uuid
from dataclasses import dataclass
from typing import Any

import requests

# Test configuration
API_URL = os.environ.get("MLRUNX_TEST_API_URL", "http://localhost:13001")
GRPC_URL = os.environ.get("MLRUNX_TEST_GRPC_URL", "localhost:15051")
TIMEOUT = 30  # seconds


@dataclass
class TestResult:
    """Result of a single test."""

    name: str
    passed: bool
    duration: float
    error: str | None = None


class MLRunTestClient:
    """HTTP client for MLRunX API testing."""

    def __init__(self, base_url: str):
        self.base_url = base_url.rstrip("/")
        self.session = requests.Session()

    def health_check(self) -> bool:
        """Check if API is healthy."""
        try:
            r = self.session.get(f"{self.base_url}/health", timeout=5)
            return r.status_code == 200 and r.text == "ok"
        except Exception:
            return False

    def init_run(
        self,
        project: str,
        name: str | None = None,
        run_id: str | None = None,
        tags: dict[str, str] | None = None,
    ) -> dict[str, Any]:
        """Initialize a new run."""
        payload = {"project": project}
        if name:
            payload["name"] = name
        if run_id:
            payload["run_id"] = run_id
        if tags:
            payload["tags"] = tags

        r = self.session.post(
            f"{self.base_url}/api/v1/runs",
            json=payload,
            timeout=TIMEOUT,
        )
        r.raise_for_status()
        return r.json()

    def ingest_batch(
        self,
        run_id: str,
        metrics: list[dict] | None = None,
        params: list[dict] | None = None,
        tags: list[dict] | None = None,
        batch_id: str | None = None,
        seq: int | None = None,
    ) -> dict[str, Any]:
        """Ingest a batch of metrics/params/tags."""
        payload = {
            "run_id": run_id,
            "metrics": metrics or [],
            "params": params or [],
            "tags": tags or [],
        }
        if batch_id:
            payload["batch_id"] = batch_id
        if seq is not None:
            payload["seq"] = seq

        r = self.session.post(
            f"{self.base_url}/api/v1/ingest/batch",
            json=payload,
            timeout=TIMEOUT,
        )
        r.raise_for_status()
        return r.json()

    def finish_run(self, run_id: str, status: str = "finished") -> dict[str, Any]:
        """Finish a run."""
        r = self.session.post(
            f"{self.base_url}/api/v1/runs/{run_id}/finish",
            json={"status": status},
            timeout=TIMEOUT,
        )
        r.raise_for_status()
        return r.json()

    def list_runs(
        self,
        project: str | None = None,
        status: str | None = None,
        limit: int = 100,
    ) -> dict[str, Any]:
        """List runs with optional filters."""
        params = {"limit": limit}
        if project:
            params["project"] = project
        if status:
            params["status"] = status

        r = self.session.get(
            f"{self.base_url}/api/v1/runs",
            params=params,
            timeout=TIMEOUT,
        )
        r.raise_for_status()
        return r.json()

    def get_run(self, run_id: str) -> dict[str, Any]:
        """Get run details."""
        r = self.session.get(
            f"{self.base_url}/api/v1/runs/{run_id}",
            timeout=TIMEOUT,
        )
        r.raise_for_status()
        return r.json()


# =============================================================================
# Test Cases
# =============================================================================


def test_health_check(client: MLRunTestClient) -> None:
    """Test that the API health endpoint is reachable."""
    assert client.health_check(), "Health check failed"


def test_sdk_basic_logging(client: MLRunTestClient) -> None:
    """Test basic SDK logging flow: init -> log metrics -> finish."""
    project = f"test-project-{uuid.uuid4().hex[:8]}"
    run_name = f"test-run-{uuid.uuid4().hex[:8]}"

    # Initialize run
    init_resp = client.init_run(project=project, name=run_name)
    run_id = init_resp["run_id"]
    assert run_id, "Run ID should be returned"
    assert not init_resp.get("offline"), "Should not be offline"

    # Log metrics in batches (simulating training loop)
    for batch_idx in range(3):
        metrics = [
            {"name": "loss", "value": 1.0 - (batch_idx * 0.1), "step": batch_idx * 10},
            {
                "name": "accuracy",
                "value": 0.5 + (batch_idx * 0.1),
                "step": batch_idx * 10,
            },
        ]
        params = (
            [{"name": "lr", "value": "0.001"}, {"name": "batch_size", "value": "32"}]
            if batch_idx == 0
            else []
        )
        tags = (
            [{"key": "model", "value": "resnet50"}] if batch_idx == 0 else []
        )

        resp = client.ingest_batch(
            run_id=run_id,
            metrics=metrics,
            params=params,
            tags=tags,
            batch_id=f"batch-{batch_idx}",
            seq=batch_idx,
        )
        assert resp["status"] == "ok", f"Batch {batch_idx} should be accepted"
        assert not resp.get("duplicate"), f"Batch {batch_idx} should not be duplicate"

    # Finish the run
    finish_resp = client.finish_run(run_id, status="finished")
    assert finish_resp["status"] == "ok", "Finish should succeed"

    # Verify run status
    run = client.get_run(run_id)
    assert run["status"] == "finished", "Run should be finished"
    assert run["metrics_count"] >= 6, "Should have at least 6 metrics (3 batches x 2)"


def test_run_listing_and_filtering(client: MLRunTestClient) -> None:
    """Test run listing with project and status filters."""
    project = f"filter-test-{uuid.uuid4().hex[:8]}"

    # Create multiple runs
    run_ids = []
    for i in range(3):
        resp = client.init_run(project=project, name=f"run-{i}")
        run_ids.append(resp["run_id"])

    # Finish some runs
    client.finish_run(run_ids[0], status="finished")
    client.finish_run(run_ids[1], status="failed")
    # run_ids[2] stays running

    # List all runs for project
    runs_resp = client.list_runs(project=project)
    assert runs_resp["total"] == 3, "Should have 3 runs"

    # Filter by status
    finished_resp = client.list_runs(project=project, status="finished")
    assert finished_resp["total"] == 1, "Should have 1 finished run"

    running_resp = client.list_runs(project=project, status="running")
    assert running_resp["total"] == 1, "Should have 1 running run"

    failed_resp = client.list_runs(project=project, status="failed")
    assert failed_resp["total"] == 1, "Should have 1 failed run"


def test_idempotent_batch_ingestion(client: MLRunTestClient) -> None:
    """Test that duplicate batches are handled idempotently."""
    project = f"idempotent-test-{uuid.uuid4().hex[:8]}"

    # Initialize run
    init_resp = client.init_run(project=project)
    run_id = init_resp["run_id"]

    batch_id = f"batch-{uuid.uuid4().hex[:8]}"
    metrics = [{"name": "loss", "value": 0.5, "step": 0}]

    # Send batch first time
    resp1 = client.ingest_batch(
        run_id=run_id,
        metrics=metrics,
        batch_id=batch_id,
        seq=0,
    )
    assert resp1["status"] == "ok"
    assert not resp1.get("duplicate")
    assert resp1["accepted"] == 1

    # Send same batch again (duplicate)
    resp2 = client.ingest_batch(
        run_id=run_id,
        metrics=metrics,
        batch_id=batch_id,
        seq=0,
    )
    assert resp2["status"] == "ok"
    assert resp2["duplicate"], "Second batch should be marked as duplicate"
    assert resp2["accepted"] == 0, "Duplicate batch should not accept new items"

    # Verify metrics count didn't double
    run = client.get_run(run_id)
    assert run["metrics_count"] == 1, "Should only have 1 metric (duplicate ignored)"


def test_large_batch_ingestion(client: MLRunTestClient) -> None:
    """Test ingestion of larger metric batches."""
    project = f"large-batch-{uuid.uuid4().hex[:8]}"

    init_resp = client.init_run(project=project)
    run_id = init_resp["run_id"]

    # Create a batch with 1000 metrics
    metrics = [
        {"name": f"metric_{i % 10}", "value": float(i), "step": i}
        for i in range(1000)
    ]

    start = time.time()
    resp = client.ingest_batch(run_id=run_id, metrics=metrics)
    elapsed = time.time() - start

    assert resp["status"] == "ok"
    assert elapsed < 5.0, f"Large batch should complete in <5s, took {elapsed:.2f}s"

    # Verify run
    run = client.get_run(run_id)
    assert run["metrics_count"] == 1000, "Should have 1000 metrics"


def test_run_tags_update(client: MLRunTestClient) -> None:
    """Test that tags can be added and updated during a run."""
    project = f"tags-test-{uuid.uuid4().hex[:8]}"

    init_resp = client.init_run(
        project=project,
        tags={"initial": "tag"},
    )
    run_id = init_resp["run_id"]

    # Add more tags via ingest
    resp = client.ingest_batch(
        run_id=run_id,
        tags=[
            {"key": "model", "value": "gpt-4"},
            {"key": "dataset", "value": "openwebtext"},
        ],
    )
    assert resp["status"] == "ok"

    # Verify tags
    run = client.get_run(run_id)
    assert "initial" in run["tags"] or len(run["tags"]) >= 2


def test_concurrent_runs(client: MLRunTestClient) -> None:
    """Test multiple concurrent runs from the same project."""
    project = f"concurrent-{uuid.uuid4().hex[:8]}"

    # Start 5 concurrent runs
    run_ids = []
    for i in range(5):
        resp = client.init_run(project=project, name=f"worker-{i}")
        run_ids.append(resp["run_id"])

    # Each run logs some metrics
    for i, run_id in enumerate(run_ids):
        metrics = [{"name": "step", "value": float(i * 100 + j), "step": j} for j in range(10)]
        client.ingest_batch(run_id=run_id, metrics=metrics)

    # Finish all runs
    for run_id in run_ids:
        client.finish_run(run_id)

    # Verify all runs finished
    runs_resp = client.list_runs(project=project, status="finished")
    assert runs_resp["total"] == 5, "All 5 runs should be finished"


# =============================================================================
# Test Runner
# =============================================================================


ALL_TESTS = [
    test_health_check,
    test_sdk_basic_logging,
    test_run_listing_and_filtering,
    test_idempotent_batch_ingestion,
    test_large_batch_ingestion,
    test_run_tags_update,
    test_concurrent_runs,
]


def wait_for_api(client: MLRunTestClient, timeout: int = 60) -> bool:
    """Wait for the API to become healthy."""
    print(f"Waiting for API at {client.base_url}...")
    start = time.time()
    while time.time() - start < timeout:
        if client.health_check():
            print(f"API ready after {time.time() - start:.1f}s")
            return True
        time.sleep(1)
    return False


def run_tests(
    client: MLRunTestClient,
    test_names: list[str] | None = None,
) -> list[TestResult]:
    """Run all or specified tests."""
    results = []

    tests_to_run = ALL_TESTS
    if test_names:
        tests_to_run = [t for t in ALL_TESTS if t.__name__ in test_names]
        if not tests_to_run:
            print(f"No tests found matching: {test_names}")
            return results

    for test_fn in tests_to_run:
        name = test_fn.__name__
        print(f"\n{'='*60}")
        print(f"Running: {name}")
        print(f"{'='*60}")

        start = time.time()
        try:
            test_fn(client)
            duration = time.time() - start
            print(f"PASSED ({duration:.2f}s)")
            results.append(TestResult(name=name, passed=True, duration=duration))
        except AssertionError as e:
            duration = time.time() - start
            print(f"FAILED: {e}")
            results.append(
                TestResult(name=name, passed=False, duration=duration, error=str(e))
            )
        except Exception as e:
            duration = time.time() - start
            print(f"ERROR: {e}")
            results.append(
                TestResult(name=name, passed=False, duration=duration, error=str(e))
            )

    return results


def print_summary(results: list[TestResult]) -> int:
    """Print test summary and return exit code."""
    print(f"\n{'='*60}")
    print("TEST SUMMARY")
    print(f"{'='*60}")

    passed = sum(1 for r in results if r.passed)
    failed = len(results) - passed
    total_duration = sum(r.duration for r in results)

    for r in results:
        status = "PASS" if r.passed else "FAIL"
        print(f"  [{status}] {r.name} ({r.duration:.2f}s)")
        if r.error:
            print(f"         Error: {r.error}")

    print(f"\nTotal: {passed}/{len(results)} passed in {total_duration:.2f}s")

    if failed > 0:
        print(f"\n{failed} test(s) failed!")
        return 1
    else:
        print("\nAll tests passed!")
        return 0


def main() -> int:
    parser = argparse.ArgumentParser(description="MLRunX Integration Test Runner")
    parser.add_argument(
        "--api-url",
        default=API_URL,
        help=f"API URL (default: {API_URL})",
    )
    parser.add_argument(
        "--test",
        action="append",
        dest="tests",
        help="Run specific test(s) by name",
    )
    parser.add_argument(
        "--list",
        action="store_true",
        help="List available tests",
    )
    parser.add_argument(
        "--no-wait",
        action="store_true",
        help="Don't wait for API to become healthy",
    )
    parser.add_argument(
        "--timeout",
        type=int,
        default=60,
        help="Timeout waiting for API (seconds)",
    )
    args = parser.parse_args()

    if args.list:
        print("Available tests:")
        for t in ALL_TESTS:
            print(f"  - {t.__name__}")
            if t.__doc__:
                print(f"      {t.__doc__.strip()}")
        return 0

    client = MLRunTestClient(args.api_url)

    if not args.no_wait:
        if not wait_for_api(client, timeout=args.timeout):
            print(f"ERROR: API did not become healthy within {args.timeout}s")
            return 1

    results = run_tests(client, test_names=args.tests)
    return print_summary(results)


if __name__ == "__main__":
    sys.exit(main())
