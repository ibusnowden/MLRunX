# Option 2 Auth Rollout Plan (Safe, Incremental)

## Goal

Introduce user accounts, project memberships, and role-based authorization without breaking current API-key SDK workflows.

## Current Baseline

- API key auth is the active guard path.
- Project scoping by key is already enforced for run access paths.
- API keys are currently managed in-memory in the API process.

## PR 1 (This Branch): Foundation Schema, No Behavior Change

Branch: `feat/pr1-auth-foundation`

Scope:
- Add additive Postgres migration `003_auth_foundation.sql` for:
  - `users`
  - `project_memberships` (`owner`/`editor`/`viewer`)
  - `audit_events`
  - backward-compatible `api_keys` metadata extensions
- Add matching SQLite schema tables for local parity and tests.
- Keep existing API-key auth behavior unchanged.

Safety rules:
- No endpoint auth behavior changes.
- No SDK surface changes.
- No in-memory API-key store removal yet.

Validation:
- Existing storage/API tests must continue passing.
- New storage test validates read/write paths for new auth tables and confirms existing run lookup still works.

## PR 2: Persist API Key Store to DB

Branch: `feat/pr2-persist-apikey-store`

Scope:
- Move `ApiKeyStore` backing data from memory to durable DB storage.
- Keep existing key format and current middleware contract.
- Add migration/backfill for legacy keys where required.

Safety rules:
- Preserve current auth semantics and response codes.
- Keep fallback/feature flag for quick rollback.

## PR 3: Add JWT/Session Auth Path for UI (Feature-Flagged)

Branch: `feat/pr3-ui-jwt-auth`

Scope:
- Introduce user login identity path (JWT/session) for UI routes.
- Map authenticated users to projects via `project_memberships`.
- Keep API-key path active for SDK/service traffic.

Safety rules:
- Dual auth mode: UI can use JWT/session, SDK continues API keys.
- Gate with feature flag (default off).

Implementation notes:
- Runtime mode: `MLRUNX_AUTH_MODE=hybrid` with `MLRUNX_UI_JWT_AUTH_ENABLED=true`
- JWT secret: `MLRUNX_JWT_SECRET=<hs256-secret>`
- Required claim validation in hybrid mode: `MLRUNX_JWT_ISSUER`, `MLRUNX_JWT_AUDIENCE`
- UI session cookies (safe browser mode):
  - `POST /api/v1/ui-auth/login` exchanges JWT for `HttpOnly` session cookie + CSRF cookie.
  - Session data is stored server-side in `auth_sessions` (token hash + csrf hash + expiry + revocation).
  - UI no longer persists JWT in `localStorage`.
  - Mutating cookie-authenticated routes require `X-CSRF-Token`.
  - Optional env controls:
    - `MLRUNX_UI_ALLOWED_ORIGINS` (comma-separated)
    - `MLRUNX_UI_SESSION_TTL_SECONDS` (default `43200`)
    - `MLRUNX_UI_SESSION_COOKIE_NAME`, `MLRUNX_UI_CSRF_COOKIE_NAME`
    - `MLRUNX_UI_COOKIE_SECURE`, `MLRUNX_UI_COOKIE_SAMESITE`
  - Active sessions are renewed server-side on each authenticated request (sliding expiration).
- JWT user identity is mapped to `users` + `project_memberships`; role -> scopes:
  - `viewer` => `read`
  - `editor` => `read`,`write`
  - `owner` => `read`,`write`,`admin`

## PR 4: Endpoint RBAC Enforcement (Feature-Flagged Rollout)

Branch: `feat/pr4-rbac-endpoint-enforcement`

Scope:
- Enforce `owner`/`editor`/`viewer` checks endpoint-by-endpoint.
- Emit `audit_events` for sensitive actions and denials.

Rollout sequence:
1. Read-only endpoints (`viewer+`)
2. Mutating endpoints (`editor+`)
3. Admin/sensitive operations (`owner`)

Safety rules:
- Feature flags per endpoint group.
- Canary rollout first, then full enablement.

Implementation notes:
- Endpoint checks now flow through a shared RBAC gate in API handlers.
- Scope enforcement flags:
  - `MLRUNX_RBAC_ENDPOINT_ENFORCEMENT_ENABLED` (master gate, default `true`)
  - `MLRUNX_RBAC_READ_ENFORCEMENT_ENABLED` (default `true`)
  - `MLRUNX_RBAC_WRITE_ENFORCEMENT_ENABLED` (default `true`)
  - `MLRUNX_RBAC_ADMIN_ENFORCEMENT_ENABLED` (default `true`)
- API-key callers keep existing behavior (always scope-enforced).
- UI JWT/session callers can be rolled out per endpoint tier using the flags above.
- `audit_events` are emitted for:
  - RBAC denials (scope or project mismatch)
  - Sensitive successes (`run.init`, `run.finish`, `run.delete`, `api_key.create`, `api_key.revoke`, `share_token.create`, `share_token.revoke`)

## Deployment Notes

- Keep API-key auth as the baseline safety net until PR 4 is fully rolled out.
- Run migration-first deploys, then application deploys.
- Add dashboards/alerts for auth failures and denied requests before enforcing strict RBAC globally.
- Use the operational runbook at `docs/ops/auth_rbac_rollout_runbook.md` for canary gates, alert thresholds, and rollback steps.

## STO-002 (In Progress): PostgreSQL Parameter Shadow Writes

Scope (current start):
- `log_params` now supports an optional PostgreSQL shadow write path.
- In-memory behavior remains authoritative for safety; PostgreSQL write is best-effort.

Feature flag:
- `MLRUNX_POSTGRES_SHADOW_WRITES_ENABLED` (default `false`)

Safety behavior:
- If PostgreSQL write fails, ingest stays available and returns a warning detail.
- If `run_id` is not a UUID, PostgreSQL shadow write is skipped with a warning detail.
