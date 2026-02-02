# W1: Run Count Scale Benchmark

## Overview

W1 benchmarks query performance at scale by populating the database with many runs
and measuring API response times for common query patterns.

## Target Metrics

| Query Type | p95 Target (Nightly) | p95 Target (Release) |
|------------|---------------------|----------------------|
| List runs  | < 200ms             | < 200ms              |
| Filter by tag | < 150ms          | < 150ms              |
| Filter by status | < 150ms       | < 150ms              |
| Search     | < 200ms             | < 200ms              |
| Compare runs | < 300ms           | < 300ms              |

## Scale Configuration

| Scale | Runs | Queries per Type | Total Queries |
|-------|------|------------------|---------------|
| Nightly | 1,000 | 100 | 500 |
| Release | 10,000 | 100 | 500 |
| Stress | 100,000 | 1000 | 5000 |

## Query Workload

### 1. List Runs
```
GET /api/v1/runs?limit=100
GET /api/v1/runs?limit=100&offset=500
```
Measures: Basic pagination performance

### 2. Filter by Tag
```
GET /api/v1/runs?tags=model:resnet50
GET /api/v1/runs?tags=model:bert-base,dataset:squad
```
Measures: Tag index lookup performance

### 3. Filter by Status
```
GET /api/v1/runs?status=finished
GET /api/v1/runs?status=running
```
Measures: Status enum filtering

### 4. Search
```
GET /api/v1/runs?q=experiment
GET /api/v1/runs?q=training+v2
```
Measures: Full-text search performance

### 5. Compare Runs
```
POST /api/v1/runs/compare
Body: {"run_ids": ["id1", "id2", ..., "id10"]}
```
Measures: Multi-run metric aggregation

## Execution

```bash
# Nightly scale (CI)
python -m bench.workloads.w1_run_scale_runner --scale nightly

# Release scale (pre-release)
python -m bench.workloads.w1_run_scale_runner --scale release

# Custom configuration
python -m bench.workloads.w1_run_scale_runner --runs 5000 --queries 50
```

## Output Format

```json
{
  "workload": "w1_run_scale",
  "timestamp": "2024-01-15T10:30:00Z",
  "config": {
    "runs": 10000,
    "queries_per_type": 100,
    "scale": "release"
  },
  "results": {
    "list_runs": {
      "p50_ms": 45,
      "p95_ms": 120,
      "p99_ms": 180,
      "min_ms": 20,
      "max_ms": 250,
      "count": 100
    },
    "filter_tag": {...},
    "filter_status": {...},
    "search": {...},
    "compare_runs": {...}
  },
  "pass": true,
  "threshold_violations": []
}
```

## Pass/Fail Criteria

The benchmark passes if ALL of the following are true:
- All p95 values are below their respective thresholds
- No query times out (default: 30s)
- No errors during query execution

## Dependencies

- BENCH-001: Synthetic generator (for data population)
- Running MLRun stack with ClickHouse and PostgreSQL
