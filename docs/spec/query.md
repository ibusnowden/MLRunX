# Query Service Specification

> **Status**: Alpha
> **Proto**: `/proto/mlrunx/v1/query.proto`
> **Last Updated**: 2026-01

This document defines the authoritative semantics for the MLRunX Query Service. The protobuf definitions must match this specification. Any changes to the proto require updating this document first.

## Table of Contents

- [Overview](#overview)
- [Runs List Filtering](#runs-list-filtering)
- [Pagination](#pagination)
- [Metric Fetch Contract](#metric-fetch-contract)
- [Server-Side Downsampling](#server-side-downsampling)
- [Downsampling Methods](#downsampling-methods)
- [Compare Runs Alignment](#compare-runs-alignment)
- [Alignment Modes](#alignment-modes)
- [Performance Targets](#performance-targets)
- [Alpha Limitations](#alpha-limitations)

---

## Overview

The Query Service provides read access to experiment data for the UI and external integrations. It is designed for:

- **Fast dashboards**: Sub-200ms p95 latency for common queries
- **Efficient charting**: Server-side downsampling reduces data transfer
- **Flexible filtering**: Rich query capabilities for large run counts

### Transport

| Protocol | Port | Use Case |
|----------|------|----------|
| HTTP/JSON | 3001 | UI, REST clients |
| gRPC | 3001 | High-performance clients |

---

## Runs List Filtering

The `ListRuns` endpoint supports rich filtering for finding runs.

### Filter Types

| Filter | Type | Description |
|--------|------|-------------|
| `statuses` | enum[] | Match any of the specified statuses |
| `tags` | Tag[] | Match all specified tags (AND) |
| `name_pattern` | string | Glob pattern matching (`*` wildcard) |
| `created_after` | Timestamp | Runs created after this time |
| `created_before` | Timestamp | Runs created before this time |
| `user_id` | string | Filter by creator |
| `parent_run_id` | string | Filter nested runs by parent |
| `param_filters` | ParameterFilter[] | Filter by parameter values |

### Filter Semantics

All filters are combined with **AND** logic:

```
result = runs WHERE
  status IN (statuses) AND
  ALL(tags) match AND
  name LIKE name_pattern AND
  created_at > created_after AND
  created_at < created_before AND
  ...
```

### Parameter Filters

Parameter filters support comparison operators:

```protobuf
message ParameterFilter {
  string name = 1;      // e.g., "learning_rate"
  ComparisonOp op = 2;  // e.g., GT
  string value = 3;     // e.g., "0.001"
}
```

| Operator | Symbol | Numeric | String |
|----------|--------|---------|--------|
| `EQ` | = | Exact match | Exact match |
| `NE` | != | Not equal | Not equal |
| `GT` | > | Greater than | Lexicographic |
| `GE` | >= | Greater or equal | Lexicographic |
| `LT` | < | Less than | Lexicographic |
| `LE` | <= | Less or equal | Lexicographic |
| `CONTAINS` | ∋ | N/A | Substring match |

Type coercion: Values are compared as numbers if both parse as numbers, otherwise as strings.

### Sorting

| Field | Default Direction | Notes |
|-------|------------------|-------|
| `CREATED_AT` | DESC | Default sort |
| `NAME` | ASC | Alphabetical |
| `STATUS` | N/A | By status enum value |
| `DURATION` | DESC | Null (running) sorts last |

### Field Selection

Use `include_fields` to reduce response size:

```protobuf
ListRunsRequest {
  include_fields: ["summary", "tags"]  // Only include these
}
```

If empty, all fields are returned.

| Field | Description |
|-------|-------------|
| `summary` | Final summary metrics |
| `params` | Hyperparameters |
| `tags` | Run tags |
| `system_info` | System metadata |

---

## Pagination

All list endpoints use **cursor-based pagination** for consistency and performance.

### Why Cursor-Based?

| Offset-Based | Cursor-Based |
|--------------|--------------|
| `OFFSET 10000` is slow | Seek to cursor is O(1) |
| Inconsistent with inserts | Stable iteration |
| Can skip/duplicate rows | Consistent results |

### Cursor Format

Cursors are opaque, base64-encoded tokens:

```
eyJsYXN0X2lkIjoiMDFIWkFCQzEyMyIsImxhc3RfdHMiOjE3MDU1MjAwMDB9
```

Contents (internal, not a contract):
```json
{
  "last_id": "01HZABC123",
  "last_ts": 1705520000,
  "sort_field": "created_at",
  "direction": "desc"
}
```

### Pagination Contract

```
┌─────────────────────────────────────────────────────┐
│  Request 1:                                         │
│    page_size: 50                                    │
│    page_token: ""  (empty = first page)            │
│                                                     │
│  Response 1:                                        │
│    runs: [50 runs]                                 │
│    next_page_token: "eyJsYXN0X2lkIjo..."           │
│    total_count: 1234                               │
│                                                     │
│  Request 2:                                         │
│    page_size: 50                                    │
│    page_token: "eyJsYXN0X2lkIjo..."                │
│                                                     │
│  Response 2:                                        │
│    runs: [50 runs]                                 │
│    next_page_token: "eyJuZXh0X2lkIjo..."           │
│                                                     │
│  ... continue until next_page_token is empty       │
└─────────────────────────────────────────────────────┘
```

### Page Size Limits

| Parameter | Default | Max |
|-----------|---------|-----|
| `page_size` | 50 | 1000 |

Requesting more than max returns max.

### Total Count

`total_count` is the **estimated** total matching the filter. For large result sets (>10,000), this may be approximate to avoid expensive COUNT queries.

---

## Metric Fetch Contract

The `GetMetrics` endpoint retrieves time-series data for charting.

### Request Parameters

| Parameter | Type | Default | Max | Description |
|-----------|------|---------|-----|-------------|
| `run_ids` | RunId[] | - | 10 | Runs to fetch |
| `metric_names` | string[] | all | 50 | Metrics to fetch |
| `min_step` | int64 | 0 | - | Start step |
| `max_step` | int64 | MAX | - | End step |
| `min_time` | Timestamp | - | - | Start time |
| `max_time` | Timestamp | - | - | End time |
| `max_points` | int32 | 1000 | 10000 | Points per metric |
| `downsample_method` | enum | LTTB | - | Downsampling algorithm |

### Response Structure

```protobuf
message GetMetricsResponse {
  repeated RunMetrics run_metrics = 1;  // Per-run data
  bool downsampled = 2;                  // Whether downsampling applied
  int64 original_point_count = 3;        // Total points before downsample
}

message RunMetrics {
  RunId run_id = 1;
  repeated MetricSeries series = 2;
}

message MetricSeries {
  string name = 1;
  repeated MetricPoint points = 2;  // Sorted by step
  MetricStats stats = 3;            // Aggregated statistics
}
```

### Statistics

Even when downsampled, full statistics are computed from the original data:

| Stat | Description |
|------|-------------|
| `min` | Minimum value |
| `max` | Maximum value |
| `mean` | Arithmetic mean |
| `last` | Most recent value |
| `count` | Total point count |

---

## Server-Side Downsampling

When a metric has more points than `max_points`, the server applies downsampling to reduce data transfer while preserving visual fidelity.

### When Downsampling Applies

```
original_points > max_points → downsample
original_points ≤ max_points → return all points
```

### Downsampling Response

The response indicates whether downsampling was applied:

```protobuf
GetMetricsResponse {
  run_metrics: [...],
  downsampled: true,
  original_point_count: 50000  // Original had 50k points
}
```

Clients can use this to show a "showing X of Y points" indicator.

---

## Downsampling Methods

### LTTB (Largest-Triangle-Three-Buckets)

**Default method.** Best for general visualization—preserves visual shape.

```
Algorithm:
1. Divide data into N buckets (N = max_points)
2. First and last points always included
3. For each middle bucket:
   - Select point that forms largest triangle with
     previous selected point and next bucket average
```

| Pros | Cons |
|------|------|
| Preserves visual shape | Slightly more CPU |
| Good for trends | May miss exact extremes |
| Perceptually accurate | |

### MIN_MAX

Keeps minimum and maximum from each bucket. Good for detecting anomalies.

```
Algorithm:
1. Divide data into N/2 buckets
2. For each bucket, emit both min and max points
```

| Pros | Cons |
|------|------|
| Preserves extremes | May show artificial spikes |
| Good for alerts | Doubles point density |

### AVERAGE

Simple bucket averaging. Fast but loses detail.

```
Algorithm:
1. Divide data into N buckets
2. For each bucket, emit average value at bucket midpoint
```

| Pros | Cons |
|------|------|
| Fastest | Loses peaks/valleys |
| Smooths noise | Poor for sparse data |

### FIRST / LAST

Takes first or last point from each bucket. Simple but can miss important data.

---

## Compare Runs Alignment

When comparing multiple runs, metrics must be aligned to a common X-axis.

### Request

```protobuf
CompareRunsRequest {
  run_ids: ["run-1", "run-2", "run-3"],
  metric_names: ["loss", "accuracy"],
  alignment: ALIGNMENT_MODE_STEP,
  max_points: 500
}
```

### Alignment Process

```
┌─────────────────────────────────────────────────────┐
│  Input (different step ranges):                     │
│                                                     │
│  Run 1: steps 0, 100, 200, 300, 400                │
│  Run 2: steps 0, 50, 150, 250, 350, 450            │
│  Run 3: steps 100, 200, 300, 400, 500              │
│                                                     │
│  Alignment (STEP mode):                             │
│                                                     │
│  Common X-axis: [0, 100, 200, 300, 400, 500]       │
│                                                     │
│  Run 1: [v0, v1, v2, v3, v4, null]                 │
│  Run 2: [v0, interp, interp, interp, interp, null] │
│  Run 3: [null, v0, v1, v2, v3, v4]                 │
│                                                     │
│  Note: Missing values shown as gaps in chart       │
└─────────────────────────────────────────────────────┘
```

---

## Alignment Modes

### STEP (Default)

Align by step number. Best for comparing training progress.

| Run 1 | Run 2 | X-Axis |
|-------|-------|--------|
| step 0 | step 0 | 0 |
| step 100 | step 100 | 100 |
| step 200 | - | 200 |

**Interpolation**: Linear interpolation for missing steps (within run's range).

### RELATIVE_TIME

Align by time since run start. Good for comparing wall-clock efficiency.

```
X = timestamp - run.started_at
```

| Run 1 (fast GPU) | Run 2 (slow GPU) | X-Axis (seconds) |
|------------------|------------------|------------------|
| +0s, loss=2.0 | +0s, loss=2.0 | 0 |
| +60s, loss=1.0 | +120s, loss=1.0 | 60, 120 |

### ABSOLUTE_TIME

Align by wall-clock timestamp. Good for runs started at same time.

```
X = timestamp (Unix epoch)
```

Useful for: A/B tests, parallel sweeps started simultaneously.

### PROGRESS

Align by percentage of total steps (0-100%). Good for runs with different total steps.

```
X = (current_step / final_step) * 100
```

| Run 1 (1000 steps) | Run 2 (5000 steps) | X-Axis (%) |
|--------------------|--------------------|-----------:|
| step 0 | step 0 | 0% |
| step 500 | step 2500 | 50% |
| step 1000 | step 5000 | 100% |

---

## Performance Targets

### Alpha Targets

| Query Type | p50 | p95 | Notes |
|------------|-----|-----|-------|
| ListRuns (≤1000 runs) | 50ms | 150ms | With basic filters |
| ListRuns (10k+ runs) | 100ms | 300ms | With pagination |
| GetMetrics (single run) | 30ms | 100ms | 1000 points |
| GetMetrics (10 runs) | 100ms | 300ms | 1000 points each |
| CompareRuns (5 runs) | 80ms | 200ms | Aligned metrics |

### Measurement

Latency measured at server-side, excluding network. Targets assume:

- ClickHouse and PostgreSQL on same network
- Warm caches
- No concurrent heavy queries

---

## Alpha Limitations

The following features are **not implemented in Alpha**:

| Feature | Alpha Behavior | Future |
|---------|----------------|--------|
| Full-text search | Prefix match only | Elasticsearch integration |
| Complex aggregations | Basic stats only | Custom SQL pass-through |
| Real-time updates | Polling required | WebSocket subscriptions |
| Export | JSON only | CSV, Parquet, Arrow |
| Saved queries | None | Named query templates |
| Query caching | None | Redis result cache |

---

## Related Documents

- [Ingest Service Specification](ingest.md)
- [System Limits](limits.md)
- [Proto Definitions](/proto/mlrunx/v1/query.proto)
