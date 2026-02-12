# MLRun Benchmark Suite

Performance benchmarking infrastructure for MLRun.

## Overview

This directory contains tools for:
- **Synthetic data generation** - Create realistic ML training runs
- **Workload benchmarks** - Measure query and ingest performance
- **CI threshold checking** - Automated regression detection

## Quick Start

```bash
# Start the MLRun stack
make infra-up

# Generate 1000 synthetic runs
python -m bench.generators.run_gen --runs 1000

# Run the full benchmark suite
make bench-all
```

## Directory Structure

```
bench/
├── generators/           # Synthetic data generation
│   ├── run_gen.py       # Main generator script
│   ├── config.yaml      # Generator configuration
│   └── __init__.py
├── workloads/           # Benchmark workloads
│   ├── w1_run_scale_runner.py      # W1: Query at scale
│   ├── w2_ingest_latency_runner.py # W2: High-freq ingest
│   └── *.md             # Workload specifications
├── results/             # Benchmark output (gitignored)
├── ci_thresholds.yaml   # CI pass/fail thresholds
├── run_nightly.py       # Nightly benchmark orchestrator
├── regression.py        # Regression detection
└── README.md            # This file
```

## Generators

### run_gen.py - Synthetic Run Generator

Generates realistic ML training runs using the MLRun Python SDK.

**Usage:**
```bash
# Basic usage
python -m bench.generators.run_gen --runs 1000

# With custom metrics
python -m bench.generators.run_gen --runs 500 --metrics 20 --points 200

# Use predefined scale
python -m bench.generators.run_gen --scale nightly
python -m bench.generators.run_gen --scale release

# Output stats to file
python -m bench.generators.run_gen --runs 1000 --output bench/results/gen_stats.json
```

**Configuration (config.yaml):**
- `defaults.runs` - Default number of runs
- `defaults.metrics_per_run` - Metrics logged per run
- `defaults.points_per_metric` - Data points per metric
- `metric_patterns` - Realistic value generators (loss decay, accuracy sigmoid, etc.)
- `tags` - Tag options for variety
- `params` - Parameter distributions
- `scale` - Predefined scale configurations (nightly, release, stress)

**Metric Patterns:**
- `exponential_decay` - For loss metrics (starts high, decays)
- `sigmoid` - For accuracy metrics (S-curve to plateau)
- `constant` - Fixed value with optional noise
- `gaussian` - Random normal distribution
- `random_walk` - Bounded random walk (for memory usage, etc.)

## Workloads

### W1: Run Count Scale

Benchmarks query performance at scale:
- 10k runs in database
- Query types: list, filter by tag, filter by status, compare
- Target: p95 < 200ms

```bash
python -m bench.workloads.w1_run_scale_runner --scale nightly
```

### W2: High Frequency Ingestion

Benchmarks ingest latency:
- 10 Hz metric logging
- Measures: SDK enqueue, flush, RPC, visibility
- Target: p95 log-to-visible < 500ms

```bash
python -m bench.workloads.w2_ingest_latency_runner --duration 30
```

### W3: Mixed Dashboard

Benchmarks concurrent read/write under dashboard load:
- Target: p95 < 300ms

```bash
python -m bench.workloads.w3_mixed_runner --scale nightly
```

## CI Integration

### Thresholds (ci_thresholds.yaml)

```yaml
workloads:
  w1_run_scale:
    list_runs: { p95_ms: 200 }
  w2_ingest_latency:
    log_to_visible: { p95_ms: 500 }

regression:
  allowable_deviation_percent: 20
```

### Running in CI

```bash
# Run with threshold checking
python bench/run_nightly.py --scale nightly --fail-on-threshold

# Check for regressions against historical data
python bench/run_nightly.py --scale nightly --fail-on-regression
```

## Output Format

All benchmarks output JSON reports:

```json
{
  "workload": "w1_run_scale",
  "timestamp": "2024-01-15T10:30:00Z",
  "config": {...},
  "results": {
    "list_runs": {"p50_ms": 45, "p95_ms": 120, "p99_ms": 180}
  },
  "pass": true
}
```

## Makefile Targets

```bash
make bench-w1        # Run W1 (nightly scale)
make bench-w2        # Run W2 (nightly scale)
make bench-w3        # Run W3 (nightly scale)
make bench-all       # Run full suite
make bench-w1-full   # Run W1 (release scale)
make bench-w2-full   # Run W2 (release scale)
```

## Requirements

- Python 3.10+
- Running MLRun stack (`make infra-up`)
- MLRun Python SDK installed (`uv sync`)
