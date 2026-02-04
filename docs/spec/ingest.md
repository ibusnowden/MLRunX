# Ingest Service Specification

> **Status**: Alpha
> **Proto**: `/proto/mlrunx/v1/ingest.proto`
> **Last Updated**: 2026-01

This document defines the authoritative semantics for the MLRunX Ingest Service. The protobuf definitions must match this specification. Any changes to the proto require updating this document first.

## Table of Contents

- [Overview](#overview)
- [Run Lifecycle](#run-lifecycle)
- [Metric Points](#metric-points)
- [Batching](#batching)
- [Idempotency Keys](#idempotency-keys)
- [Ordering Assumptions](#ordering-assumptions)
- [Warnings vs Errors](#warnings-vs-errors)
- [Artifact Presign Flow](#artifact-presign-flow)
- [Run Resumption](#run-resumption)
- [Alpha Limitations](#alpha-limitations)

---

## Overview

The Ingest Service is the primary entry point for ML training jobs to send data to MLRunX. It is designed for:

- **High throughput**: Handle 100k+ metrics/second per instance
- **Non-blocking**: Never slow down training loops
- **At-least-once delivery**: With client-side deduplication
- **Crash resilience**: Automatic recovery of interrupted runs

### Transport

| Protocol | Port | Use Case |
|----------|------|----------|
| gRPC | 50051 | Primary SDK transport, streaming support |
| HTTP/2 | 3002 | REST fallback, browser-based clients |

---

## Run Lifecycle

A run progresses through the following states:

```
                    ┌─────────────┐
                    │   RUNNING   │◄──────────────────┐
                    └──────┬──────┘                   │
                           │                          │
           ┌───────────────┼───────────────┐          │
           │               │               │          │
           ▼               ▼               ▼          │
    ┌──────────┐    ┌──────────┐    ┌──────────┐     │
    │ FINISHED │    │  FAILED  │    │  KILLED  │     │
    └──────────┘    └──────────┘    └──────────┘     │
                           ▲               ▲          │
                           │               │          │
                    ┌──────┴───────────────┘          │
                    │                                 │
             ┌──────┴──────┐                          │
             │   CRASHED   │──── (resume) ────────────┘
             └─────────────┘
```

### State Transitions

| From | To | Trigger |
|------|-----|---------|
| (none) | RUNNING | `InitRun` called |
| RUNNING | FINISHED | `FinishRun(status=FINISHED)` |
| RUNNING | FAILED | `FinishRun(status=FAILED)` |
| RUNNING | KILLED | `FinishRun(status=KILLED)` or manual termination |
| RUNNING | CRASHED | Heartbeat timeout (5 minutes) |
| CRASHED | RUNNING | `InitRun` with `resume_token` |

### InitRun Idempotency

`InitRun` is idempotent based on `run_id`:

- If `run_id` is not provided, server generates a UUID v7
- If `run_id` exists and run is RUNNING, returns existing run
- If `run_id` exists and run is terminal (FINISHED/FAILED/KILLED), returns error
- If `run_id` exists and run is CRASHED, requires `resume_token`

---

## Metric Points

A metric point consists of:

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | Yes | Metric name (see [limits.md](limits.md#naming-constraints)) |
| `step` | int64 | Yes | Non-negative step number |
| `value` | double | Yes | Metric value (NaN/Inf preserved) |
| `timestamp` | Timestamp | No | Wall-clock time (server uses receipt time if omitted) |

### Step Semantics

- Steps represent logical progress (iteration, epoch, batch)
- Steps **do not need to be sequential** or start at 0
- Duplicate `(name, step)` pairs are deduplicated (last write wins)
- Negative steps are rejected with error

### Value Handling

| Value | Behavior |
|-------|----------|
| Normal float | Stored as-is |
| `NaN` | Stored, displayed as "NaN" in UI |
| `+Inf` / `-Inf` | Stored, excluded from statistics |
| Subnormal | Flushed to zero |

---

## Batching

Metrics are sent in batches for efficiency. See [limits.md](limits.md#batch-limits) for size constraints.

### Batch Structure

```protobuf
message LogMetricsRequest {
  RunId run_id = 1;
  MetricBatch metrics = 2;   // Up to 10,000 points
  string batch_id = 3;       // Required for idempotency
  optional int64 sequence = 4;  // Optional ordering hint
}
```

### SDK Batching Behavior

The Python SDK implements automatic batching:

1. **Buffer metrics** in memory (configurable buffer size, default 1000)
2. **Flush on threshold**: When buffer reaches limit
3. **Flush on timer**: Every 5 seconds (configurable)
4. **Flush on finish**: Drain buffer before `FinishRun`

This is transparent to the user—`run.log()` returns immediately.

---

## Idempotency Keys

Every `LogMetricsRequest` must include a `batch_id` for deduplication.

### Batch ID Requirements

- **Format**: UUID v7 recommended (time-ordered)
- **Uniqueness**: Must be unique within a run
- **Persistence**: SDK should persist batch_id with local spool

### Deduplication Behavior

```
┌─────────────────────────────────────────────────────┐
│                     Server                          │
│                                                     │
│  1. Receive batch with batch_id="abc-123"          │
│  2. Check batch_id in dedup cache (Redis/memory)   │
│  3. If exists:                                      │
│       - Return success (idempotent)                │
│       - Set deduplicated_count in response         │
│  4. If not exists:                                  │
│       - Write to ClickHouse                        │
│       - Store batch_id in dedup cache (TTL: 24h)   │
│       - Return success                             │
└─────────────────────────────────────────────────────┘
```

### Dedup Cache TTL

- **Alpha**: 24 hours
- **Future**: Configurable per project

After TTL expiration, a replayed batch_id would be re-ingested. This is acceptable for Alpha scope.

---

## Ordering Assumptions

Batches may arrive out of order due to network conditions. The `sequence` field enables server-side reordering.

### Without Sequence Field

If `sequence` is not provided:

- Batches are processed in arrival order
- Points are ordered by `step` within each batch
- **No cross-batch ordering guarantees**
- UI displays data in step order (client-side sort)

### With Sequence Field

If `sequence` is provided:

```
┌─────────────────────────────────────────────────────┐
│  Client sends:                                      │
│    Batch A: sequence=1, batch_id="aaa"             │
│    Batch B: sequence=2, batch_id="bbb"             │
│    Batch C: sequence=3, batch_id="ccc"             │
│                                                     │
│  Network delivers: B, C, A                          │
│                                                     │
│  Server behavior:                                   │
│    1. Receive B (seq=2), buffer (waiting for seq=1)│
│    2. Receive C (seq=3), buffer                    │
│    3. Receive A (seq=1), process A, B, C in order  │
│                                                     │
│  Timeout: After 30 seconds, process buffered       │
│           batches regardless of gaps               │
└─────────────────────────────────────────────────────┘
```

### Reordering Limits

| Parameter | Value | Notes |
|-----------|-------|-------|
| Buffer size | 100 batches | Per run |
| Buffer timeout | 30 seconds | Then process out-of-order |
| Max gap | 1000 | Larger gaps processed as-is |

---

## Warnings vs Errors

MLRunX follows a **degrade gracefully** philosophy. We prefer accepting partial data over rejecting entire requests.

### Error Classification

| Severity | Behavior | Examples |
|----------|----------|----------|
| **ERROR** | Request rejected, client must retry or abort | Invalid auth, malformed proto, run not found |
| **WARNING** | Request accepted with degradation | Invalid metric name (dropped), oversized batch (truncated) |

### Hard Errors (Request Rejected)

| Code | Description |
|------|-------------|
| `UNAUTHENTICATED` | Invalid or expired API key |
| `PERMISSION_DENIED` | No access to project/run |
| `NOT_FOUND` | Run does not exist |
| `INVALID_ARGUMENT` | Malformed request (missing required fields) |
| `FAILED_PRECONDITION` | Run in terminal state |
| `RESOURCE_EXHAUSTED` | Rate limit exceeded (Alpha: no rate limits) |

### Warnings (Request Accepted)

| Code | Description | Degradation |
|------|-------------|-------------|
| `INVALID_METRIC_NAME` | Name exceeds limits or invalid chars | Point dropped |
| `BATCH_TRUNCATED` | Batch exceeded max size | Excess points dropped |
| `DUPLICATE_BATCH` | batch_id already processed | Idempotent success |
| `CLOCK_SKEW` | Client timestamp far from server | Timestamp adjusted |
| `STEP_NEGATIVE` | Negative step value | Point dropped |

### Response Structure

```protobuf
message LogMetricsResponse {
  int64 accepted_count = 1;      // Points successfully stored
  int64 deduplicated_count = 2;  // Points already seen
  repeated ErrorDetail warnings = 3;  // Non-fatal issues
}
```

### Rate Limiting (Future)

> **Alpha Scope**: No rate limiting in Alpha release.

Future releases will implement token bucket rate limiting:
- Per-project limits
- Graceful degradation (queue, not reject)
- Backpressure signaling to SDK

---

## Artifact Presign Flow

Large artifacts (models, datasets) are uploaded directly to object storage using presigned URLs.

### Flow Diagram

```
┌──────────┐         ┌──────────┐         ┌──────────┐
│   SDK    │         │  Ingest  │         │  MinIO   │
└────┬─────┘         └────┬─────┘         └────┬─────┘
     │                    │                    │
     │ 1. CreateArtifactUpload(metadata)      │
     │───────────────────►│                    │
     │                    │                    │
     │ 2. Return presigned_url, upload_id     │
     │◄───────────────────│                    │
     │                    │                    │
     │ 3. HTTP PUT to presigned_url           │
     │────────────────────────────────────────►│
     │                    │                    │
     │ 4. 200 OK                              │
     │◄────────────────────────────────────────│
     │                    │                    │
     │ 5. FinalizeArtifactUpload(upload_id, checksum)
     │───────────────────►│                    │
     │                    │                    │
     │                    │ 6. Verify object   │
     │                    │───────────────────►│
     │                    │                    │
     │ 7. Return artifact metadata            │
     │◄───────────────────│                    │
     │                    │                    │
```

### Presigned URL Properties

| Property | Value |
|----------|-------|
| Expiration | 1 hour |
| Method | PUT only |
| Max size | 5 GB (Alpha) |
| Content-Type | As specified in metadata |

### Finalization Validation

On `FinalizeArtifactUpload`, server validates:

1. Object exists in storage
2. Size matches `actual_size_bytes`
3. MD5 matches `md5_checksum`

If validation fails, upload is rejected and object is deleted.

### Partial Upload Cleanup

Uploads not finalized within 24 hours are cleaned up by background job.

---

## Run Resumption

Crashed runs can be resumed using the `resume_token` returned by `InitRun`.

### Resume Flow

```
┌──────────────────────────────────────────────────────┐
│  Original Run                                        │
│  1. SDK calls InitRun, receives resume_token="xyz"  │
│  2. SDK persists resume_token to disk               │
│  3. Run crashes (process killed, OOM, etc.)         │
│  4. Server detects crash after 5min heartbeat gap   │
│  5. Run status -> CRASHED                           │
│                                                      │
│  Resumed Run                                         │
│  1. SDK restarts, finds persisted resume_token      │
│  2. SDK calls InitRun(run_id, resume_token="xyz")   │
│  3. Server validates resume_token                    │
│  4. Run status -> RUNNING, resumed=true             │
│  5. SDK replays any locally spooled data            │
└──────────────────────────────────────────────────────┘
```

### Resume Token Properties

- **Format**: Signed JWT (server-side secret)
- **Contents**: run_id, created_at, sequence_checkpoint
- **Expiration**: 7 days
- **Single use**: Token invalidated after successful resume

### What Resumes

| Data | Behavior |
|------|----------|
| Metrics already sent | Deduplicated via batch_id |
| Metrics in local spool | Replayed by SDK |
| Parameters | Preserved |
| Tags | Preserved |
| Artifacts | Preserved (completed only) |

---

## Alpha Limitations

The following features are **not implemented in Alpha**:

| Feature | Alpha Behavior | Future |
|---------|----------------|--------|
| Rate limiting | None | Token bucket per project |
| Compression | None | gzip, zstd on wire |
| Multi-region | Single region | Cross-region replication |
| Encryption | TLS only | At-rest encryption |
| Audit logging | None | Full audit trail |
| Streaming metrics | Polling only | WebSocket push |

---

## Related Documents

- [Query Service Specification](query.md)
- [System Limits](limits.md)
- [Proto Definitions](/proto/mlrunx/v1/ingest.proto)
