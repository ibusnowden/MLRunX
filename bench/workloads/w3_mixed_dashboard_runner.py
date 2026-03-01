#!/usr/bin/env python3
"""W3 benchmark: mixed dashboard workload latency."""

from __future__ import annotations

import argparse
import json
import random
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from bench.workloads.common import ApiClient, Timer, load_scale_config, summarize_ms, utc_now_iso


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run W3 mixed-dashboard benchmark")
    parser.add_argument("--scale", default="nightly", choices=["nightly", "release", "stress"])
    parser.add_argument("--server-url", default="http://localhost:3001")
    parser.add_argument("--api-key", default=None)
    parser.add_argument(
        "--concurrent-users",
        type=int,
        default=None,
        help="Override worker concurrency for local validation",
    )
    parser.add_argument(
        "--duration-s",
        type=int,
        default=None,
        help="Override benchmark duration for local validation",
    )
    parser.add_argument("--output", required=True)
    return parser.parse_args()


def seed_dashboard_data(client: ApiClient, project: str, run_count: int = 20) -> list[str]:
    run_ids: list[str] = []
    for i in range(run_count):
        run_id = client.init_run(project=project, name=f"bench-w3-run-{i}")
        run_ids.append(run_id)

        metrics = [
            {
                "name": "accuracy",
                "value": min(0.999, 0.5 + (step * 0.01) + (i * 0.001)),
                "step": step,
            }
            for step in range(30)
        ]
        client.ingest_batch(run_id, metrics=metrics, batch_id=f"seed-w3-{i}", seq=i)
        client.finish_run(run_id, status="finished")

    return run_ids


def worker(
    server_url: str,
    api_key: str | None,
    project: str,
    run_ids: list[str],
    deadline: float,
    seed: int,
) -> dict[str, list[float]]:
    client = ApiClient(server_url, api_key)
    rng = random.Random(seed)
    latencies: dict[str, list[float]] = {
        "dashboard_load": [],
        "list_runs": [],
        "run_detail": [],
        "metrics_series": [],
        "compare_runs": [],
    }

    def op_list() -> str:
        client.list_runs(project=project, limit=50)
        return "list_runs"

    def op_detail() -> str:
        client.get_run(rng.choice(run_ids))
        return "run_detail"

    def op_metrics() -> str:
        client.get_metrics(rng.choice(run_ids), names=["accuracy"], max_points=200)
        return "metrics_series"

    def op_compare() -> str:
        chosen = rng.sample(run_ids, min(4, len(run_ids)))
        client.compare_runs(chosen, metric_names=["accuracy"])
        return "compare_runs"

    operations = [op_list, op_detail, op_metrics, op_compare]

    try:
        while time.perf_counter() < deadline:
            op = rng.choice(operations)
            with Timer() as timer:
                op_name = op()
            elapsed_ms = timer.elapsed * 1000.0
            latencies["dashboard_load"].append(elapsed_ms)
            latencies[op_name].append(elapsed_ms)
            time.sleep(0.02)
    finally:
        client.close()

    return latencies


def main() -> None:
    args = parse_args()
    scale_config = load_scale_config(args.scale, "w3")

    concurrent_users = max(1, int(args.concurrent_users or scale_config.get("concurrent_users", 5)))
    duration_s = max(1, int(args.duration_s or scale_config.get("duration_s", 30)))

    bootstrap = ApiClient(args.server_url, args.api_key)
    try:
        bootstrap.health()
        project = f"bench-w3-{int(time.time())}"
        run_ids = seed_dashboard_data(bootstrap, project, run_count=max(20, concurrent_users * 4))
    finally:
        bootstrap.close()

    deadline = time.perf_counter() + duration_s
    latency_values: dict[str, list[float]] = {
        "dashboard_load": [],
        "list_runs": [],
        "run_detail": [],
        "metrics_series": [],
        "compare_runs": [],
    }
    lock = threading.Lock()

    with ThreadPoolExecutor(max_workers=concurrent_users) as pool:
        futures = [
            pool.submit(
                worker,
                args.server_url,
                args.api_key,
                project,
                run_ids,
                deadline,
                idx,
            )
            for idx in range(concurrent_users)
        ]

        for future in futures:
            sample = future.result()
            with lock:
                for op_name, op_latencies in sample.items():
                    latency_values[op_name].extend(op_latencies)

    results = {
        "workload": "w3_mixed",
        "timestamp": utc_now_iso(),
        "scale": args.scale,
            "config": {
                "concurrent_users": concurrent_users,
                "duration_s": duration_s,
                "samples": len(latency_values["dashboard_load"]),
            },
            "results": {
                "dashboard_load": summarize_ms(latency_values["dashboard_load"]),
                "list_runs": summarize_ms(latency_values["list_runs"]),
                "run_detail": summarize_ms(latency_values["run_detail"]),
                "metrics_series": summarize_ms(latency_values["metrics_series"]),
                "compare_runs": summarize_ms(latency_values["compare_runs"]),
            },
        }

    output_path = Path(args.output)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(json.dumps(results, indent=2), encoding="utf-8")

    print(json.dumps(results, indent=2))


if __name__ == "__main__":
    main()
