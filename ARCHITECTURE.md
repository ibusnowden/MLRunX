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

## Change Guide

- API/auth/rbac behavior: `apps/api/src/`
- UI behavior: `apps/ui/src/`
- SDK client behavior: `sdks/python/src/mlrunx/`
- Contract changes: `proto/` and `crates/proto/`
