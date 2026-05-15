"""Common utilities for benchmark workload runners."""

from __future__ import annotations

import math
import time
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import httpx
import yaml

DEFAULT_TIMEOUT = 30.0


def utc_now_iso() -> str:
    """Return current UTC timestamp in RFC3339-like format."""
    return datetime.now(UTC).isoformat().replace("+00:00", "Z")


def percentile(values: list[float], p: float) -> float:
    """Compute percentile with linear interpolation."""
    if not values:
        raise ValueError("percentile requires at least one value")

    ordered = sorted(values)
    idx = (len(ordered) - 1) * p
    lower = math.floor(idx)
    upper = math.ceil(idx)

    if lower == upper:
        return ordered[int(idx)]

    lower_value = ordered[lower]
    upper_value = ordered[upper]
    return lower_value + (upper_value - lower_value) * (idx - lower)


def summarize_ms(values: list[float]) -> dict[str, float]:
    """Summarize latency values in milliseconds."""
    if not values:
        return {"p50_ms": 0.0, "p95_ms": 0.0, "p99_ms": 0.0}

    return {
        "p50_ms": round(percentile(values, 0.50), 3),
        "p95_ms": round(percentile(values, 0.95), 3),
        "p99_ms": round(percentile(values, 0.99), 3),
    }


def summarize_plain(values: list[float]) -> dict[str, float]:
    """Summarize values using plain p50/p95/p99 keys."""
    if not values:
        return {"p50": 0.0, "p95": 0.0, "p99": 0.0}

    return {
        "p50": round(percentile(values, 0.50), 3),
        "p95": round(percentile(values, 0.95), 3),
        "p99": round(percentile(values, 0.99), 3),
    }


def load_scale_config(
    scale: str,
    workload_key: str,
    thresholds_path: str | Path = "bench/ci_thresholds.yaml",
) -> dict[str, Any]:
    """Load scale config for a benchmark workload."""
    with open(thresholds_path, encoding="utf-8") as handle:
        config = yaml.safe_load(handle)
    return config.get("scale", {}).get(scale, {}).get(workload_key, {})


class ApiClient:
    """Simple synchronous API client for benchmark scripts."""

    def __init__(self, server_url: str, api_key: str | None = None, timeout: float = DEFAULT_TIMEOUT):
        headers: dict[str, str] = {}
        if api_key:
            headers["x-api-key"] = api_key

        self.base_url = server_url.rstrip("/")
        self.client = httpx.Client(
            base_url=self.base_url,
            headers=headers,
            timeout=timeout,
        )

    def close(self) -> None:
        self.client.close()

    def health(self) -> None:
        response = self.client.get("/health")
        response.raise_for_status()

    def init_run(self, project: str, name: str, tags: dict[str, str] | None = None) -> str:
        payload: dict[str, Any] = {"project": project, "name": name}
        if tags:
            payload["tags"] = tags
        response = self.client.post("/api/v1/runs", json=payload)
        response.raise_for_status()
        body = response.json()
        run_id = body.get("run_id")
        if not isinstance(run_id, str) or not run_id:
            raise RuntimeError(f"run init returned invalid run_id: {body}")
        return run_id

    def finish_run(self, run_id: str, status: str = "finished") -> None:
        response = self.client.post(f"/api/v1/runs/{run_id}/finish", json={"status": status})
        response.raise_for_status()

    def ingest_batch(
        self,
        run_id: str,
        metrics: list[dict[str, Any]],
        tags: list[dict[str, str]] | None = None,
        batch_id: str | None = None,
        seq: int | None = None,
    ) -> None:
        payload: dict[str, Any] = {
            "run_id": run_id,
            "metrics": metrics,
            "params": [],
            "tags": tags or [],
        }
        if batch_id:
            payload["batch_id"] = batch_id
        if seq is not None:
            payload["seq"] = seq

        response = self.client.post("/api/v1/ingest/batch", json=payload)
        response.raise_for_status()

    def list_runs(self, **params: Any) -> dict[str, Any]:
        response = self.client.get("/api/v1/runs", params=params)
        response.raise_for_status()
        return response.json()

    def get_run(self, run_id: str) -> dict[str, Any]:
        response = self.client.get(f"/api/v1/runs/{run_id}")
        response.raise_for_status()
        return response.json()

    def get_metrics(
        self,
        run_id: str,
        names: list[str] | None = None,
        max_points: int = 500,
    ) -> dict[str, Any]:
        params: dict[str, Any] = {"max_points": max_points}
        if names:
            params["names"] = ",".join(names)

        response = self.client.get(f"/api/v1/runs/{run_id}/metrics", params=params)
        response.raise_for_status()
        return response.json()

    def compare_runs(self, run_ids: list[str], metric_names: list[str] | None = None) -> dict[str, Any]:
        payload = {
            "run_ids": run_ids,
            "metric_names": metric_names or [],
            "max_points": 200,
            "alignment": "step",
        }
        response = self.client.post("/api/v1/runs/compare", json=payload)
        response.raise_for_status()
        return response.json()


class Timer:
    """Small helper for measuring elapsed wall-clock time."""

    def __enter__(self) -> Timer:
        self._start = time.perf_counter()
        return self

    def __exit__(self, *_: object) -> None:
        self.elapsed = time.perf_counter() - self._start
