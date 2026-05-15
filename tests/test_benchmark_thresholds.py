from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

import yaml

REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT))


def _write_thresholds(path: Path) -> None:
    payload = {
        "workloads": {
            "w1_run_scale": {
                "list_runs": {
                    "p95_ms": 200,
                    "p99_ms": 500,
                },
            },
        },
        "command_slos": {
            "runs.list": {
                "workload": "w1_run_scale",
                "metric": "list_runs",
                "targets": {
                    "nightly": {
                        "p95_ms": 200,
                        "p99_ms": 500,
                    },
                    "release": {
                        "p95_ms": 150,
                        "p99_ms": 300,
                    },
                },
            },
        },
    }
    path.write_text(yaml.safe_dump(payload), encoding="utf-8")


def test_command_slos_use_result_scale(tmp_path: Path) -> None:
    from bench.run_nightly import NightlyRunner

    thresholds = tmp_path / "thresholds.yaml"
    _write_thresholds(thresholds)

    runner = NightlyRunner(thresholds_path=thresholds, output_dir=tmp_path / "results")
    results = {
        "workload": "w1_run_scale",
        "scale": "release",
        "results": {
            "list_runs": {
                "p95_ms": 175,
                "p99_ms": 250,
            },
        },
    }

    threshold_passed, threshold_violations = runner.check_thresholds(results)
    _, command_violations = runner.check_command_slos(results)

    assert threshold_passed
    assert threshold_violations == []
    assert command_violations == ["runs.list/p95_ms: 175.00 > 150"]


def test_check_threshold_cli_fails_on_command_slo_violation(tmp_path: Path) -> None:
    thresholds = tmp_path / "thresholds.yaml"
    results_path = tmp_path / "w1_release.json"
    _write_thresholds(thresholds)
    results_path.write_text(
        json.dumps(
            {
                "workload": "w1_run_scale",
                "scale": "release",
                "results": {
                    "list_runs": {
                        "p95_ms": 175,
                        "p99_ms": 250,
                    },
                },
            }
        ),
        encoding="utf-8",
    )

    completed = subprocess.run(
        [
            sys.executable,
            "bench/run_nightly.py",
            "check-threshold",
            "--results",
            str(results_path),
            "--thresholds",
            str(thresholds),
        ],
        check=False,
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
    )

    assert completed.returncode == 1
    assert "runs.list/p95_ms: 175.00 > 150" in completed.stdout
