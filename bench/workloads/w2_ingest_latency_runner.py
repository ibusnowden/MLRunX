#!/usr/bin/env python3
"""W2 benchmark: ingest latency and log visibility latency."""

from __future__ import annotations

import argparse
import json
import time
from pathlib import Path

from bench.workloads.common import (
    ApiClient,
    Timer,
    load_scale_config,
    summarize_plain,
    utc_now_iso,
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run W2 ingest-latency benchmark")
    parser.add_argument("--scale", default="nightly", choices=["nightly", "release", "stress"])
    parser.add_argument("--server-url", default="http://localhost:3001")
    parser.add_argument("--api-key", default=None)
    parser.add_argument("--duration-s", type=int, default=None, help="Override test duration")
    parser.add_argument("--frequency-hz", type=int, default=None, help="Override ingest frequency")
    parser.add_argument("--output", required=True)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    scale_config = load_scale_config(args.scale, "w2")

    duration_s = max(1, int(args.duration_s or scale_config.get("duration_s", 30)))
    frequency_hz = int(args.frequency_hz or scale_config.get("frequency_hz", 10))

    if frequency_hz <= 0:
        raise ValueError("frequency_hz must be > 0")

    interval_s = 1.0 / frequency_hz

    enqueue_us_values: list[float] = []
    ingest_rpc_ms_values: list[float] = []
    log_to_visible_ms_values: list[float] = []

    client = ApiClient(args.server_url, args.api_key)

    try:
        client.health()
        project = f"bench-w2-{int(time.time())}"
        run_id = client.init_run(project=project, name="bench-w2-ingest")

        deadline = time.perf_counter() + duration_s
        next_tick = time.perf_counter()
        sequence = 0
        expected_metrics = 0

        while time.perf_counter() < deadline:
            metric_count = 4

            with Timer() as enqueue_timer:
                metrics = [
                    {
                        "name": "loss",
                        "value": max(0.001, 1.0 - (sequence * 0.001) - (idx * 0.0001)),
                        "step": sequence,
                    }
                    for idx in range(metric_count)
                ]

            enqueue_us_values.append(enqueue_timer.elapsed * 1_000_000.0)

            with Timer() as rpc_timer:
                client.ingest_batch(
                    run_id,
                    metrics=metrics,
                    batch_id=f"w2-{sequence}",
                    seq=sequence,
                )
            ingest_rpc_ms_values.append(rpc_timer.elapsed * 1000.0)

            expected_metrics += metric_count
            visible_start = time.perf_counter()
            while True:
                run = client.get_run(run_id)
                if int(run.get("metrics_count", 0)) >= expected_metrics:
                    break
                if time.perf_counter() - visible_start > 3.0:
                    break
                time.sleep(0.01)

            log_to_visible_ms_values.append((time.perf_counter() - visible_start) * 1000.0)

            sequence += 1
            next_tick += interval_s
            sleep_for = next_tick - time.perf_counter()
            if sleep_for > 0:
                time.sleep(sleep_for)

        client.finish_run(run_id, status="finished")

        results = {
            "workload": "w2_ingest_latency",
            "timestamp": utc_now_iso(),
            "scale": args.scale,
            "config": {
                "duration_s": duration_s,
                "frequency_hz": frequency_hz,
                "samples": len(ingest_rpc_ms_values),
            },
            "results": {
                "sdk_enqueue_us": summarize_plain(enqueue_us_values),
                "log_to_visible_ms": summarize_plain(log_to_visible_ms_values),
                "ingest_rpc_ms": summarize_plain(ingest_rpc_ms_values),
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
