#!/usr/bin/env python3
"""W1 benchmark: query latency at run scale."""

from __future__ import annotations

import argparse
import json
import random
import time
from collections.abc import Callable
from pathlib import Path
from typing import Any

from bench.workloads.common import ApiClient, Timer, load_scale_config, summarize_ms, utc_now_iso


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run W1 query-scale benchmark")
    parser.add_argument("--scale", default="nightly", choices=["nightly", "release", "stress"])
    parser.add_argument("--server-url", default="http://localhost:3001")
    parser.add_argument("--api-key", default=None)
    parser.add_argument("--runs", type=int, default=None, help="Override seeded run count")
    parser.add_argument(
        "--queries-per-type",
        type=int,
        default=None,
        help="Override benchmark samples per query type",
    )
    parser.add_argument("--output", required=True)
    return parser.parse_args()


def benchmark_call(samples: int, fn: Callable[[int], Any]) -> list[float]:
    latencies_ms: list[float] = []
    for idx in range(samples):
        with Timer() as timer:
            fn(idx)
        latencies_ms.append(timer.elapsed * 1000.0)
    return latencies_ms


def seed_runs(client: ApiClient, project: str, run_count: int) -> list[str]:
    run_ids: list[str] = []

    for i in range(run_count):
        run_id = client.init_run(
            project=project,
            name=f"bench-w1-run-{i}",
            tags={
                "model": "baseline" if i % 2 == 0 else "candidate",
                "split": "train" if i % 3 == 0 else "eval",
            },
        )
        run_ids.append(run_id)

        if i < 16:
            metrics = [
                {"name": "loss", "value": max(0.01, 1.0 - (step * 0.1)), "step": step}
                for step in range(10)
            ]
            client.ingest_batch(run_id, metrics=metrics, batch_id=f"seed-{i}", seq=i)

        if i % 3 == 0:
            client.finish_run(run_id, status="finished")
        elif i % 11 == 0:
            client.finish_run(run_id, status="failed")

    return run_ids


def main() -> None:
    args = parse_args()
    scale_config = load_scale_config(args.scale, "w1")

    run_count = max(1, int(args.runs or scale_config.get("runs", 1000)))
    query_samples = max(1, int(args.queries_per_type or scale_config.get("queries_per_type", 100)))

    client = ApiClient(args.server_url, args.api_key)
    try:
        client.health()

        project = f"bench-w1-{int(time.time())}"
        run_ids = seed_runs(client, project, run_count)
        compare_ids = run_ids[: min(8, len(run_ids))]

        list_runs_ms = benchmark_call(
            query_samples,
            lambda idx: client.list_runs(project=project, limit=100, offset=(idx * 10) % max(1, run_count)),
        )

        filter_tag_ms = benchmark_call(
            query_samples,
            lambda idx: client.list_runs(
                project=project,
                tags="model=baseline" if idx % 2 == 0 else "split=train",
                limit=50,
            ),
        )

        filter_status_ms = benchmark_call(
            query_samples,
            lambda idx: client.list_runs(
                project=project,
                status="finished" if idx % 2 == 0 else "running",
                limit=50,
            ),
        )

        search_ms = benchmark_call(
            query_samples,
            lambda idx: client.list_runs(project=project, q=f"bench-w1-run-{idx % 20}", limit=25),
        )

        compare_ms = benchmark_call(
            query_samples,
            lambda idx: client.compare_runs(
                random.sample(compare_ids, min(4, len(compare_ids))),
                metric_names=["loss"],
            ),
        )

        results = {
            "workload": "w1_run_scale",
            "timestamp": utc_now_iso(),
            "scale": args.scale,
            "config": {
                "runs": run_count,
                "queries_per_type": query_samples,
            },
            "results": {
                "list_runs": summarize_ms(list_runs_ms),
                "filter_tag": summarize_ms(filter_tag_ms),
                "filter_status": summarize_ms(filter_status_ms),
                "search": summarize_ms(search_ms),
                "compare_runs": summarize_ms(compare_ms),
            },
        }

        output_path = Path(args.output)
        output_path.parent.mkdir(parents=True, exist_ok=True)
        output_path.write_text(json.dumps(results, indent=2), encoding="utf-8")

        print(json.dumps(results, indent=2))
    finally:
        client.close()


if __name__ == "__main__":
    main()
