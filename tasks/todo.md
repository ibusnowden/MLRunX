# MLRunX Experiment Tracking Milestone Plan

## Objective
Build MLRunX as a pure experiment tracking platform with:
- Easy SDK startup via `mlrunx.init()`
- Automatic background sync to a remote VM server
- Structured, searchable run metadata
- Fine-grained logging of parameters, metrics, arrays, images, and chart payloads
- Strong user/project isolation and auditability for public multi-user usage

## Product Scope
- In scope: experiment tracking, run metadata, visualizations, filtering, comparison, access control, deployment hardening.
- Out of scope: workflow orchestration, schedulers, distributed job control, training execution management.

## Phase 0: Product Contract and Baseline
- [x] Write `docs/specs/experiment-tracking-v1.md` with the canonical run model and event taxonomy.
- [x] Define canonical SDK surface for v1:
- [x] `mlrunx.init(...)`
- [x] `run.log(...)`
- [x] `run["parameters"] = {...}` and `run["tags"] = {...}`
- [x] `run.log_image(...)`
- [x] `run.log_chart(...)`
- [x] `run.finish(...)`
- [x] Define query contract for list/filter/compare and max payload limits per event type.
- [x] Confirm non-goal boundary in docs: no orchestration node graph, no scheduler API.

Acceptance tests
- [ ] Spec review sign-off by API, SDK, and UI owners.
- [ ] No contradiction with current architecture in `ARCHITECTURE.md`.

## Phase 1: SDK Run Lifecycle and Automatic Sync
- [ ] Ensure `mlrunx.init()` creates or resumes a run deterministically.
- [ ] Run background worker for buffered async sync with retry and backoff.
- [x] Add graceful shutdown flush path for normal process exit.
- [ ] Add durable local spool for transient network or VM downtime.
- [x] Add idempotency keys for replay-safe sync after reconnect.

Acceptance tests
- [ ] Unit: background queue, retry, backoff, flush, and resume semantics.
- [ ] Integration: simulate API outage, log events, restore API, verify eventual delivery without duplicate writes.
- [ ] Integration: process restart with unsent spool, verify pending events are recovered and sent.

## Phase 2: Structured and Searchable Metadata Model
- [x] Define typed metadata namespaces:
- [x] `parameters.*`
- [x] `tags.*`
- [x] `system.*`
- [x] `dataset.*`
- [x] `model.*`
- [x] Add server-side validation for key naming, max depth, value types, and size limits.
- [x] Add indexed query paths for common filters (project, owner, status, name, tags, created_at).
- [x] Add audit metadata for create/update/delete actions that affect run discoverability.

Acceptance tests
- [x] API contract tests reject malformed metadata and oversize payloads.
- [x] Query correctness tests for equality, range, and compound filters.
- [ ] Performance target: list/filter queries remain within agreed p95 budget on seeded dataset.

## Phase 3: Rich Logging (Metrics, Arrays, Images, Charts)
- [ ] Standardize numeric series logging for scalar and array metrics.
- [ ] Add image logging API with metadata and storage references.
- [ ] Add custom chart logging schema:
- [ ] chart type
- [ ] data payload
- [ ] layout/options
- [ ] renderer hint
- [ ] Add ingestion-time sanitization and size limits for chart JSON and image metadata.
- [ ] Add retrieval APIs that support both run detail and compare views.

Acceptance tests
- [ ] API tests for scalar metrics, array metrics, image references, and chart payload validation.
- [ ] UI tests that load and render logged charts for multiple runs without crashes.
- [ ] Security tests that reject unsafe or malformed chart payloads.

## Phase 4: Query and Compare at Scale
- [ ] Implement filter grammar for runs:
- [ ] field comparisons
- [ ] tag and parameter filtering
- [ ] status and time windows
- [ ] multi-clause AND/OR support
- [ ] Implement compare API for thousands of candidate runs with server-side paging.
- [ ] Add stable sorting and deterministic tie-breakers.
- [ ] Add cached summaries for high-traffic list/compare endpoints where needed.

Acceptance tests
- [ ] Contract tests for filter grammar parsing and validation errors.
- [ ] Load tests for 10k+ runs with query and compare p95/p99 measurements.
- [ ] Correctness tests for sorting, pagination, and filter intersections.

## Phase 5: UI Usability and Visualization
- [ ] Keep run detail above-the-fold usable without deep scrolling.
- [ ] Keep unified training console panel (configuration, progress, hyperparameters, logs) sticky or docked.
- [ ] Keep metric group filters with clear defaults and low click path to target charts.
- [ ] Render chart lines without visual clutter by default for dense runs.
- [ ] Add saved filters and compare presets for repeat workflows.

Acceptance tests
- [ ] UI e2e for run list -> run detail -> compare flow.
- [ ] Visual regression snapshots for light and dark modes.
- [ ] Manual usability pass on desktop and mobile breakpoints.

## Phase 6: Multi-Tenant Security and Access Control
- [ ] Enforce project/user isolation at every run and key access path.
- [ ] Keep admin elevation explicit and minimal; default new users must be non-admin.
- [ ] Ensure UI session auth does not depend on localStorage API keys by default.
- [ ] Keep share links disabled by default; if enabled, enforce short bounded TTL and revocation.
- [ ] Keep trusted proxy headers fail-closed with explicit proxy CIDR allowlist.
- [ ] Ensure secrets are required at startup in production-like deployments.

Acceptance tests
- [ ] Negative tests: user A cannot access user B runs, keys, sessions, or admin endpoints.
- [ ] Auth tests: no implicit admin grants from signup.
- [ ] Startup checks fail when required auth/proxy/secrets configuration is missing.
- [ ] Audit trail tests for auth failures, key actions, and admin actions.

## Phase 7: Deployment Tracks (On-Prem and Cloud)
- [ ] Maintain and verify Docker Compose production path.
- [ ] Maintain and verify Kubernetes path with explicit TLS ingress guidance.
- [ ] Add production env checklist for required security settings.
- [ ] Add VM bootstrap script for Oracle VM deploy consistency.
- [ ] Add backup/restore smoke path for SQLite and artifacts.

Acceptance tests
- [ ] `docker compose ... config` passes with production env file and required vars.
- [ ] Fresh VM deploy smoke: login/signup, create API key, run ingest, run detail render.
- [ ] K8s deploy smoke with ingress and TLS.

## Phase 8: Launch Gate (Public Multi-User)
- [ ] Set and verify launch-critical env vars:
- [ ] `MLRUNX_ENVIRONMENT=production`
- [ ] `MLRUNX_AUTH_MODE=hybrid`
- [ ] `MLRUNX_UI_JWT_AUTH_ENABLED=true`
- [ ] RBAC read/write/admin enforcement all `true`
- [ ] `MLRUNX_ALLOW_INSECURE_LOCAL_DEV=false`
- [ ] `MLRUNX_AUTH_HMAC_SECRET` set
- [ ] `MLRUNX_TRUST_PROXY_HEADERS=true` with exact `MLRUNX_TRUSTED_PROXY_CIDRS`
- [ ] `MLRUNX_SHARE_LINKS_ENABLED=false` unless policy-approved
- [ ] Execute preflight and smoke tests in deployment VM.
- [ ] Capture launch evidence (logs, response codes, screenshots).

Acceptance tests
- [ ] Unauthenticated `GET /api/v1/runs` returns `401` or `403`.
- [ ] Two-user isolation smoke passes end-to-end.
- [ ] CLI and SDK can create runs and logs successfully against deployment.
- [ ] No Sev-1/Sev-2 issues open for auth, data isolation, or run loss.

## Execution Rules
- [ ] For non-trivial work, write implementation steps in this file before coding.
- [ ] If a step fails, stop and update the plan before continuing.
- [ ] Mark items complete only after proving behavior with tests or logs.
- [ ] Keep changes minimal and architecture-consistent.

## Review Notes
- Date:
- Reviewer:
- Summary:
- Risks remaining:
- Follow-ups:
