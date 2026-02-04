# System Limits Specification

> **Status**: Alpha
> **Last Updated**: 2026-01

This document defines the authoritative limits and constraints for MLRunX. These limits apply to the Alpha release and may change in future versions.

## Table of Contents

- [Overview](#overview)
- [Naming Constraints](#naming-constraints)
- [Batch Limits](#batch-limits)
- [Parameter Limits](#parameter-limits)
- [Tag Limits](#tag-limits)
- [Artifact Limits](#artifact-limits)
- [Query Limits](#query-limits)
- [Retention and Rollup](#retention-and-rollup)
- [Cardinality Limits](#cardinality-limits)
- [Rate Limits](#rate-limits)
- [Alpha vs Future](#alpha-vs-future)

---

## Overview

Limits exist to:

1. **Ensure predictable performance** at scale
2. **Prevent runaway resource consumption**
3. **Maintain fair multi-tenant operation**

### Enforcement Policy

| Severity | Response |
|----------|----------|
| Soft limit | Warning in response, request accepted |
| Hard limit | Error, request rejected |

Most limits in Alpha are soft—we prefer degraded operation over hard failures.

---

## Naming Constraints

### Project Names

| Property | Constraint |
|----------|------------|
| Min length | 1 |
| Max length | 128 |
| Allowed chars | `a-z`, `0-9`, `-`, `_` |
| Case | Lowercase only |
| First char | Must be letter |
| Reserved | Cannot start with `mlrunx-` or `system-` |

**Regex**: `^[a-z][a-z0-9_-]{0,127}$`

### Run Names

| Property | Constraint |
|----------|------------|
| Min length | 0 (optional) |
| Max length | 256 |
| Allowed chars | Any UTF-8 |
| Reserved | None |

### Run IDs

| Property | Constraint |
|----------|------------|
| Format | UUID (any version) or custom |
| Max length | 64 |
| Allowed chars | `a-z`, `A-Z`, `0-9`, `-`, `_` |
| Recommended | UUID v7 (time-ordered) |

### Metric Names

| Property | Constraint |
|----------|------------|
| Min length | 1 |
| Max length | 256 |
| Allowed chars | `a-z`, `A-Z`, `0-9`, `.`, `-`, `_`, `/` |
| First char | Must be letter |
| Reserved prefixes | `_mlrun.` (system metrics) |

**Valid examples**:
- `loss`
- `train/accuracy`
- `eval.f1_score`
- `model-v2.precision`

**Invalid examples**:
- `123metric` (starts with number)
- `metric name` (contains space)
- `_mlrun.internal` (reserved prefix)

### Tag Keys

| Property | Constraint |
|----------|------------|
| Min length | 1 |
| Max length | 128 |
| Allowed chars | `a-z`, `A-Z`, `0-9`, `.`, `-`, `_` |
| Case | Case-sensitive |
| Reserved prefixes | `mlrunx.` (system tags) |

### Parameter Names

Same as metric names.

---

## Batch Limits

Limits for `LogMetricsRequest`.

### Points per Batch

| Limit | Value | Enforcement |
|-------|-------|-------------|
| Max points | 10,000 | Hard limit |
| Recommended | 1,000-5,000 | For optimal latency |

Exceeding 10,000 points returns error `INVALID_ARGUMENT`.

### Bytes per Batch

| Limit | Value | Enforcement |
|-------|-------|-------------|
| Max batch size | 1 MB | Hard limit |
| Max single value | 8 bytes (double) | N/A |

Calculated as serialized protobuf size.

### Batch ID

| Property | Constraint |
|----------|------------|
| Max length | 64 |
| Required | Yes |
| Format | Any string (UUID v7 recommended) |

---

## Parameter Limits

Limits for `LogParamsRequest`.

### Per-Request Limits

| Limit | Value | Enforcement |
|-------|-------|-------------|
| Max params per request | 1,000 | Hard limit |

### Per-Run Limits

| Limit | Value | Enforcement |
|-------|-------|-------------|
| Max unique params | 10,000 | Soft limit (warning) |

### Value Limits

| Property | Constraint |
|----------|------------|
| Max value length | 4 KB | Hard limit |
| Format | String (client handles types) |

### Parameter Immutability

Parameters are **immutable** once set:

- First `LogParams` with name "lr" sets value
- Subsequent `LogParams` with same name are idempotent (same value = OK)
- Different value for same name = warning, original value kept

---

## Tag Limits

Limits for `LogTagsRequest`.

### Per-Run Limits

| Limit | Value | Enforcement |
|-------|-------|-------------|
| Max unique tags | 1,000 | Soft limit (warning) |

### Value Limits

| Property | Constraint |
|----------|------------|
| Max key length | 128 | Hard limit |
| Max value length | 1 KB | Hard limit |

### Tag Mutability

Unlike parameters, tags **can be updated**:

- `LogTags` with existing key overwrites value
- Use `remove_keys` to delete tags

---

## Artifact Limits

Limits for artifact uploads.

### Size Limits

| Limit | Value | Enforcement |
|-------|-------|-------------|
| Max single artifact | 5 GB | Hard limit |
| Max total per run | 100 GB | Soft limit (warning) |

### Count Limits

| Limit | Value | Enforcement |
|-------|-------|-------------|
| Max artifacts per run | 10,000 | Soft limit (warning) |

### Name Limits

| Property | Constraint |
|----------|------------|
| Max path length | 512 |
| Allowed chars | Any UTF-8 except `\0` |
| Path separator | `/` |

### Presigned URL

| Property | Value |
|----------|-------|
| Expiration | 1 hour |
| Max concurrent uploads | 100 per run |

---

## Query Limits

Limits for Query Service operations.

### ListRuns

| Parameter | Default | Max |
|-----------|---------|-----|
| `page_size` | 50 | 1,000 |
| Filter tags | - | 20 |
| Filter params | - | 20 |

### GetMetrics

| Parameter | Default | Max |
|-----------|---------|-----|
| `run_ids` | - | 10 |
| `metric_names` | all | 50 |
| `max_points` | 1,000 | 10,000 |

### CompareRuns

| Parameter | Default | Max |
|-----------|---------|-----|
| `run_ids` | - | 20 |
| `metric_names` | - | 20 |
| `max_points` | 500 | 5,000 |

### SearchRuns

| Parameter | Default | Max |
|-----------|---------|-----|
| Query length | - | 500 chars |
| `page_size` | 50 | 500 |

---

## Retention and Rollup

Data retention policies for Alpha release.

### Metric Retention

| Data Type | Retention | Notes |
|-----------|-----------|-------|
| Raw metrics | 90 days | Full resolution |
| Rolled-up metrics | 1 year | Downsampled |
| Summary metrics | Forever | Final values only |

### Rollup Strategy

After 90 days, raw metrics are rolled up:

```
┌─────────────────────────────────────────────────────┐
│  Original: 100,000 points over 90 days             │
│                                                     │
│  Rollup buckets:                                    │
│  - 1 minute buckets (first 7 days): ~10k points    │
│  - 5 minute buckets (days 8-30): ~8k points        │
│  - 1 hour buckets (days 31-90): ~2k points         │
│                                                     │
│  Rolled up: ~20,000 points (80% reduction)         │
│                                                     │
│  Preserved per bucket:                              │
│  - min, max, avg, count, first, last               │
└─────────────────────────────────────────────────────┘
```

### Artifact Retention

| State | Retention |
|-------|-----------|
| Completed artifacts | Until run deleted |
| Incomplete uploads | 24 hours (auto-cleanup) |

### Run Retention

| State | Retention |
|-------|-----------|
| Active runs | Forever |
| Deleted runs | 30 days (soft delete) |
| Permanently deleted | Immediately |

---

## Cardinality Limits

Limits to prevent unbounded metric explosion.

### Per-Run Cardinality

| Dimension | Limit | Enforcement |
|-----------|-------|-------------|
| Unique metric names | 10,000 | Soft (warning at 5k) |
| Total points | 100 million | Soft (warning at 50M) |

### Per-Project Cardinality

| Dimension | Limit | Enforcement |
|-----------|-------|-------------|
| Unique metric names | 100,000 | Soft limit |
| Active runs | 10,000 | Soft limit |

### Cardinality Guard

When approaching limits:

1. **Warning** in `LogMetrics` response at 80% of limit
2. **Error** at 100% of limit (new metrics rejected, existing accepted)

```protobuf
ErrorDetail {
  code: "CARDINALITY_LIMIT_APPROACHING"
  message: "Run has 8,000 unique metrics (limit: 10,000)"
  severity: WARNING
}
```

---

## Rate Limits

> **Alpha**: No rate limits enforced. All limits below are planned for GA.

### Planned Rate Limits (Post-Alpha)

| Operation | Limit | Window |
|-----------|-------|--------|
| `InitRun` | 100 | per minute per project |
| `LogMetrics` | 10,000 points | per second per run |
| `LogMetrics` batches | 100 | per second per run |
| Query operations | 1,000 | per minute per project |

### Rate Limit Response

When rate limited (future):

```protobuf
Status {
  code: RESOURCE_EXHAUSTED
  message: "Rate limit exceeded"
  details: [
    RetryInfo {
      retry_delay: 5s
    }
  ]
}
```

---

## Alpha vs Future

### Current Alpha Limits

| Category | Status | Notes |
|----------|--------|-------|
| Naming constraints | Enforced | Hard limits |
| Batch limits | Enforced | Hard limits |
| Parameter/tag limits | Soft | Warnings only |
| Query limits | Enforced | Hard limits |
| Retention | 90 days raw | No rollup yet |
| Cardinality | Monitored | Warnings only |
| Rate limits | None | No enforcement |

### Planned Changes

| Version | Changes |
|---------|---------|
| Beta | Rate limits, stricter cardinality |
| GA | Configurable retention, project quotas |

---

## Client SDK Behavior

SDKs should handle limits gracefully:

### Batch Size

```python
# SDK auto-batches to stay under limits
run.log({"loss": 0.5})  # Buffered
run.log({"loss": 0.4})  # Buffered
# Auto-flush at 5000 points or 5 seconds
```

### Warning Handling

```python
# SDK logs warnings but doesn't fail
response = client.log_metrics(batch)
for warning in response.warnings:
    logger.warning(f"MLRunX: {warning.message}")
```

### Error Handling

```python
try:
    response = client.log_metrics(batch)
except InvalidArgumentError as e:
    # Hard limit exceeded, must fix
    logger.error(f"Batch rejected: {e}")
    raise
```

---

## Related Documents

- [Ingest Service Specification](ingest.md)
- [Query Service Specification](query.md)
- [Proto Definitions](/proto/mlrunx/v1/common.proto)
