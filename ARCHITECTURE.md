# Architecture

This file describes the stable, high-level shape of MLRunX.
It is intentionally short and avoids implementation details that change often.

## Core Model

MLRunX is run-centric.

- A `run` is one training execution.
- SDKs initialize runs, then stream metrics/params/tags/events/artifacts.
- UI and APIs query runs for monitoring, comparison, and auditability.

## Main Components

- `apps/api`:
  - Rust API gateway (Axum HTTP + Tonic gRPC).
  - AuthN/AuthZ, RBAC checks, key/session management, ingest/query endpoints.
- `apps/ui`:
  - Next.js UI.
  - Uses API endpoints for auth, run listing, run detail, charts, admin operations.
- `sdks/python`:
  - Non-blocking client logging with batching + local spool for offline resilience.
- `crates/proto`:
  - Shared protobuf contracts used by ingest/query surfaces.
- `crates/api-policy`:
  - Shared authorization policy checks reused by API handlers.
- `crates/api-http-types`:
  - Shared HTTP request/response structs for API surface boundaries.

## Data Ownership (v0.1)

- SQLite is the primary source of truth in the shipped path.
- API layer owns schema evolution and storage access.
- UI and SDK do not write storage directly; they only call API contracts.

## Request Lifecycle

1. Client initializes a run (`init`).
2. Client logs batches (`ingest`), with idempotency and validation.
3. API persists run state and time-series metadata.
4. UI queries run/project/admin endpoints for visualization and operations.

## Reliability and Safety

- SDK logging is asynchronous and designed to not block training loops.
- Offline/local spool supports later replay when connectivity returns.
- Ingest endpoints apply dedup/idempotency semantics.
- Auth is scoped (project + role + endpoint enforcement).

## Evolution Path

The repo also contains scaffolded scale-out building blocks (Postgres/ClickHouse/MinIO and service splits in `services/`), but current production behavior remains API + SQLite first.

## Experiment Tracking Contract (v1)

MLRunX is a pure experiment tracking system.

- In scope:
  - Run lifecycle and metadata.
  - Metrics, params, tags, and run events.
  - Query/list/compare workflows.
  - Multi-tenant isolation and auditability.
- Out of scope:
  - Workflow orchestration graphs.
  - Scheduler/job control APIs.
  - Training execution control planes.

Canonical run model:
- `run_id`, `project_id`, `name`, `status`
- `tags`, `parameters`, `metrics`, `events`
- `created_at`, `updated_at`, `duration_seconds`

Canonical event taxonomy:
- `metric`: `{name, value, step, timestamp}`
- `param`: `{name, value}`
- `tag`: `{key, value}`
- `event`: `{level, source, message, step?, timestamp?}`

SDK surface boundary (v1):
- `mlrunx.init(...)`
- `run.log(...)`
- `run["parameters"] = {...}` / `run["tags"] = {...}`
- `run.log_image(...)`
- `run.log_chart(...)`
- `run.finish(...)`

Current query contract boundary:
- List: `GET /api/v1/runs` with `project`, `status`, `q`, `tags`, `limit`, `offset`.
- Compare: `POST /api/v1/runs/compare` with `run_ids`, `metric_names`, `max_points`, `alignment`.

Limits and safety:
- Ingest batch payload target <= 1 MB (`docs/spec/limits.md`).
- Run event message server limit: 2000 chars.
- Run event source limit: 64 chars.
- Compare request maximum: 100 runs.
- Access enforcement is project-scoped with RBAC and ownership checks.

## Internal Boundaries

- `apps/api` should primarily assemble routes, middleware, and storage wiring.
- Authorization decision logic belongs in `crates/api-policy`.
- Shared HTTP payload schemas belong in `crates/api-http-types`.
- Handler modules should avoid re-implementing policy checks inline.

## Change Guide

- API/auth/rbac behavior: `apps/api/src/`
- UI behavior: `apps/ui/src/`
- SDK client behavior: `sdks/python/src/mlrunx/`
- Contract changes: `proto/` and `crates/proto/`
