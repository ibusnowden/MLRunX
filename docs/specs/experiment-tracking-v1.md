# Experiment Tracking V1 Contract

## Purpose
Define the canonical MLRunX v1 contract for experiment tracking so SDK, API, and UI ship the same behavior across local, VM, and cloud deployments.

This contract is run-centric and architecture-compatible with `ARCHITECTURE.md`.

## Scope
- In scope: run lifecycle, metrics/params/tags/events logging, structured metadata, query/list/compare behavior, and payload boundaries.
- Out of scope: workflow orchestration, DAG/node execution engines, job schedulers, and cluster/job management APIs.

## Canonical Run Model
- `run_id`: immutable unique ID (string UUID).
- `project_id`: immutable owner project boundary.
- `name`: optional user label.
- `status`: `running | finished | failed | killed`.
- `tags`: mutable key/value labels used for filtering.
- `parameters`: immutable-or-last-write key/value hyperparameters.
- `metrics`: time-series numeric points keyed by metric name.
- `events`: ordered log stream for trainer/system/sdk messages.
- `created_at`, `updated_at`, `duration_seconds`: lifecycle timestamps and derived duration.

## Event Taxonomy
All ingest flows normalize into these event classes:
- `metric`: `{name, value, step, timestamp}`
- `param`: `{name, value}`
- `tag`: `{key, value}`
- `event`: `{level, source, message, step?, timestamp?}`

Structured logging kinds are encoded as JSON envelopes in `event.message`:
- `artifact`
- `image`
- `chart`
- `trainer`
- `system`

Envelope format:
```json
{"kind":"chart","payload":{"name":"loss_curve","chart_type":"line","data":{}}}
```

## SDK Surface (v1)
Required Python SDK surface:
- `mlrunx.init(...)`
- `run.log(...)`
- `run["parameters"] = {...}`
- `run["tags"] = {...}`
- `run.log_image(...)`
- `run.log_chart(...)`
- `run.finish(...)`

Also supported:
- `run.log_event(...)`
- `run.log_artifact(...)`
- Module-level convenience wrappers (`mlrunx.log_*`) for active runs.

Behavior guarantees:
- Non-blocking enqueue on training thread.
- Background batching with retry + backoff.
- Disk spool fallback when offline.
- Idempotent ingest support via SDK-provided `batch_id` and `seq`.

## Query Contract (List / Filter / Compare)
### List runs: `GET /api/v1/runs`
Supported query fields:
- `project`
- `status`
- `q` (free-text)
- `tags` (comma-separated `key` or `key=value`)
- `limit` (capped at 1000)
- `offset`

### Compare runs: `POST /api/v1/runs/compare`
Request:
- `run_ids` (1..100)
- `metric_names` (optional, empty means common metrics)
- `max_points` (default 1000)
- `alignment` (`step` or `time`)

## Payload Limits (v1)
- Recommended ingest batch payload: <= 1 MB (see `docs/spec/limits.md`).
- `event.message`: server-normalized and truncated to <= 2000 chars.
- `event.source`: normalized and capped at <= 64 chars.
- SDK structured envelope helper (`run.log_image/log_chart/log_artifact`) caps encoded message size at 1800 chars and marks truncation metadata when exceeded.
- Compare endpoint maximum run count: 100.
- Metrics retrieval `max_points` default: 1000 (downsampling path).

## Multi-Tenant and Audit Expectations
- Every run is project-scoped.
- Read/write access must pass endpoint RBAC + project ownership checks.
- SDK/API keys and sessions are project-scoped; no cross-project run visibility.
- Run mutations are auditable via API audit event stream.

## Non-Goals Boundary (Explicit)
MLRunX v1 is a pure experiment tracking platform. It does not expose:
- Orchestration node graph APIs
- Scheduler/job queue APIs
- Training execution control plane APIs

Any future orchestration integration must remain a separate product boundary and must not weaken this run-centric contract.
