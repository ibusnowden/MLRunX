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

## Deployment Notes

- Keep API-key auth as the baseline safety net until PR 4 is fully rolled out.
- Run migration-first deploys, then application deploys.
- Add dashboards/alerts for auth failures and denied requests before enforcing strict RBAC globally.
