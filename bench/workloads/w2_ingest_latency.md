# W2: High Frequency Ingestion Benchmark

## Overview

W2 benchmarks the end-to-end latency of high-frequency metric logging, measuring
the time from when the SDK logs a metric to when it's visible via the query API.

This is the critical "log-to-visible" SLO metric that determines how quickly
users see their training progress in the dashboard.

## Target Metrics

| Metric | p95 Target (Nightly) | p95 Target (Release) |
|--------|---------------------|----------------------|
| SDK enqueue time | < 50μs | < 50μs |
| Flush time | < 200ms | < 200ms |
| Ingest RPC latency | < 100ms | < 100ms |
| Log-to-visible | < 500ms | < 500ms |

## Scale Configuration

| Scale | Duration | Frequency | Metrics/Run | Total Points |
|-------|----------|-----------|-------------|--------------|
| Nightly | 30s | 10 Hz | 5 | ~1,500 |
| Release | 300s | 10 Hz | 10 | ~30,000 |
| Stress | 600s | 100 Hz | 20 | ~1,200,000 |

## Measurement Points

### 1. SDK Enqueue Time
Time from `run.log()` call to event being queued internally.

**Measurement:**
- Instrument `Run.log()` method
- Capture time before and after queue.put()

**Target:** < 50μs (must be non-blocking)

### 2. Flush Time
Time from flush trigger to HTTP batch request completion.

**Measurement:**
- Instrument FlushWorker
- Track batch start to response received

**Target:** < 200ms

### 3. Ingest RPC Latency
HTTP round-trip time for the ingest batch request.

**Measurement:**
- Time from HTTP request send to response complete
- Excludes SDK batching overhead

**Target:** < 100ms

### 4. Log-to-Visible Latency
End-to-end time from log() call to metric appearing in query API.

**Measurement:**
1. Record timestamp when run.log() called
2. Immediately start polling query API
3. Record timestamp when metric appears in response
4. Calculate delta

**Target:** < 500ms (p95)

## Execution

```bash
# Nightly scale (CI)
python -m bench.workloads.w2_ingest_latency_runner --scale nightly

# Release scale (pre-release)
python -m bench.workloads.w2_ingest_latency_runner --scale release

# Custom configuration
python -m bench.workloads.w2_ingest_latency_runner --duration 60 --frequency 10
```

## Output Format

```json
{
  "workload": "w2_ingest_latency",
  "timestamp": "2024-01-15T10:30:00Z",
  "config": {
    "duration_s": 30,
    "frequency_hz": 10,
    "metrics_per_step": 5
  },
  "results": {
    "sdk_enqueue_us": {
      "p50": 5,
      "p95": 15,
      "p99": 50,
      "count": 1500
    },
    "flush_ms": {
      "p50": 45,
      "p95": 120,
      "p99": 180
    },
    "ingest_rpc_ms": {
      "p50": 25,
      "p95": 80,
      "p99": 150
    },
    "log_to_visible_ms": {
      "p50": 150,
      "p95": 350,
      "p99": 480
    }
  },
  "throughput": {
    "events_per_sec": 850,
    "batches_per_sec": 2.5
  },
  "pass": true,
  "threshold_violations": []
}
```

## Pass/Fail Criteria

The benchmark passes if ALL of the following are true:
- log_to_visible p95 < 500ms
- ingest_rpc p95 < 100ms
- No dropped events due to queue overflow
- No timeout errors

## Dependencies

- Running MLRun stack with all services
- BENCH-001: Synthetic generator (optional, for comparison)
