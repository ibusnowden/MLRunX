# MLRunX Security Audit Report

**Date:** 2026-02-13
**Scope:** Full codebase review — API (Rust/Axum), Dashboard (Next.js), Python SDK, Infrastructure
**Severity Scale:** CRITICAL / HIGH / MEDIUM / LOW / INFO
**Status:** All 16 codebase issues have been **RESOLVED**. New host-level findings from the Ubuntu runtime output (2026-02-14) are **OPEN** (see Section 9).

---

## 1. Architecture Overview

MLRunX is a performance-first ML experiment tracking platform with:

| Component | Stack | Location |
|---|---|---|
| API Gateway | Rust (Axum + gRPC) | `apps/api/src/` |
| Dashboard | Next.js + TypeScript | `apps/ui/src/` |
| Python SDK | httpx, async batching | `sdks/python/src/` |
| Storage | SQLite (dev), Postgres + ClickHouse (prod) | `apps/api/src/storage/` |
| Artifacts | MinIO / S3 | configured via env |
| Infra | Docker Compose, K8s manifests | `infra/` |

**Auth model:** API keys (SHA-256 hashed), UI JWT sessions (ES256/RS256/HS256), CSRF tokens for browser sessions, RBAC with read/write/admin scopes, optional project-scoping.

---

## 2. Critical Issues

### 2.1 CORS Fully Permissive When Auth Is Disabled

**Severity:** CRITICAL
**File:** `apps/api/src/main.rs` (CORS setup block)

When UI JWT auth is disabled, the CORS layer falls through to `CorsLayer::permissive()`, allowing **any origin** to make credentialed requests to the API.

```rust
let cors = if ui_jwt_enabled {
    CorsLayer::new()
        .allow_credentials(true)
        .allow_origin(allowed_origins)
        // ...
} else {
    CorsLayer::permissive()  // ANY origin, ANY method, ANY header
};
```

**Why this matters:** A developer running `MLRUNX_AUTH_DISABLED=true` (as documented for dev mode) exposes the entire API to cross-origin attacks from any website. Combined with disabled auth, any webpage the developer visits can silently read/write/delete their experiment data.

**Recommendation:** Apply restrictive CORS regardless of auth state. At minimum, default to `localhost` origins only.

> **RESOLVED:** Replaced `CorsLayer::permissive()` fallback with restrictive CORS that always applies `allow_origin` with explicit origins (defaults to `localhost:3000`). `allow_credentials` is now tied to `ui_jwt_enabled` state.

---

### 2.2 Hardcoded Development Credentials in `.env.example`

**Severity:** CRITICAL
**File:** `infra/docker/.env.example:54, 63, 65, 72`

```env
CLICKHOUSE_PASSWORD=mlrunx_dev
POSTGRES_PASSWORD=mlrunx_dev
DATABASE_URL=postgres://mlrunx:mlrunx_dev@postgres:5432/mlrunx
MINIO_SECRET_KEY=mlrunx_dev_secret
```

**Why this matters:** `.env.example` files are meant to be copied to `.env`. Developers routinely copy-paste without changing values. These predictable passwords will inevitably appear in production deployments, especially quick Docker Compose setups. Any attacker who knows the project uses MLRunX can guess every credential.

**Recommendation:** Leave password fields blank with a comment requiring `openssl rand -hex 32`, or generate random values in the Docker entrypoint. The `MLRUNX_API_KEY` field already follows this pattern correctly — apply it consistently.

> **RESOLVED:** Removed all hardcoded passwords from `.env.example`. Fields are now blank with `# REQUIRED: Set a strong password. Generate with: openssl rand -hex 32` comments.

---

### 2.3 No Rate Limiting on Authentication Endpoints

**Severity:** CRITICAL
**File:** `apps/api/src/main.rs` (route definitions)

There is no rate limiting middleware on any endpoint, including:
- `/api/v1/ui-auth/login` — session creation
- `/api/v1/ingest/batch` — data ingestion
- API key-authenticated routes

**Why this matters:** Without rate limiting, an attacker can brute-force API keys, flood the ingest pipeline, or DoS the SQLite backend (which serializes writes). The SHA-256 key hash comparison is fast, making key brute-force practical if the key space is predictable.

**Recommendation:** Add per-IP rate limiting via `tower::limit` or a dedicated middleware. At minimum: 10 req/s on auth endpoints, 100 req/s on ingest, 1000 req/s globally.

> **RESOLVED:** Implemented a zero-dependency in-memory token-bucket rate limiter (`HttpRateLimiter`) in `main.rs`. Per-IP tracking with configurable capacity (`MLRUNX_RATE_LIMIT_CAPACITY`, default 60) and refill rate (`MLRUNX_RATE_LIMIT_REFILL_RATE`, default 10/s). Background task prunes stale entries every 60s. Auth-level rate limiting (`AuthRateLimiter`) was already present; this adds HTTP-level rate limiting as an outer layer. Returns `429 Too Many Requests` with `Retry-After` header.

---

## 3. High Severity Issues

### 3.1 RBAC Enforcement Is a Kill-Switch Environment Variable

**Severity:** HIGH
**File:** `apps/api/src/main.rs:202-205`

```rust
if !env_flag_default("MLRUNX_RBAC_ENDPOINT_ENFORCEMENT_ENABLED", true) {
    return false;  // All RBAC checks bypassed
}
```

**Why this matters:** A single environment variable silently disables all role-based access control for UI JWT users. A `read`-scoped user gains `admin` powers. This is a configuration footgun — especially dangerous in orchestrated environments where env vars can be accidentally inherited or template-expanded.

**Recommendation:** Remove this toggle entirely, or restrict it to a compile-time feature flag that cannot be set at runtime.

> **RESOLVED:** Removed the `MLRUNX_RBAC_ENDPOINT_ENFORCEMENT_ENABLED` check from `should_enforce_scope()`. RBAC is now always enforced for UI JWT users (unless in dev mode).

---

### 3.2 API Key Sent as Bearer Token Instead of Dedicated Header

**Severity:** HIGH
**File:** `sdks/python/src/mlrunx/transport/http.py:54`

```python
headers["Authorization"] = f"Bearer {self._api_key}"
```

**Why this matters:**
1. **Semantic confusion:** `Bearer` tokens are OAuth2 access tokens. Using `Authorization: Bearer` for API keys confuses security auditors and tooling.
2. **Proxy logging:** Many reverse proxies, WAFs, and CDNs log or strip `Authorization` headers by default. API keys are long-lived secrets — they should not appear in proxy access logs.
3. **Framework interference:** The API server must distinguish between JWT tokens and API keys in the same header, increasing parser complexity and attack surface.

**Recommendation:** Use a dedicated `X-API-Key` header for API keys and reserve `Authorization: Bearer` exclusively for JWT tokens.

> **RESOLVED:** Python SDK now uses `X-API-Key` header instead of `Authorization: Bearer`. The API server already prefers `X-API-Key` over `Authorization: Bearer` in credential extraction.

---

### 3.3 Disabled User Sessions Remain Valid

**Severity:** HIGH
**File:** `apps/api/src/storage/sqlite.rs` (`set_user_disabled()`)

Disabling a user sets `disabled_at` on the user record but does not invalidate existing sessions.

**Why this matters:** A compromised or terminated user account retains access until all sessions naturally expire. In an incident response scenario, disabling the user gives a false sense of security.

**Recommendation:** When disabling a user, also delete or invalidate all their active sessions in the same transaction.

> **RESOLVED:** Added `revoke_all_sessions_for_user()` to `SqliteStore` and wired it into `http_admin_disable_user()`. All active sessions are revoked immediately when a user is disabled.

---

### 3.4 No Security Headers on API Responses

**Severity:** HIGH
**File:** `apps/api/src/main.rs` (response pipeline)

The API does not set any of the standard security headers:

| Missing Header | Risk |
|---|---|
| `Strict-Transport-Security` | Downgrade attacks to HTTP |
| `X-Content-Type-Options: nosniff` | MIME-type confusion attacks |
| `X-Frame-Options: DENY` | Clickjacking on dashboard |
| `Content-Security-Policy` | XSS on dashboard |
| `Referrer-Policy` | Token leakage via Referer |

**Recommendation:** Add a middleware layer that sets these headers on every response.

> **RESOLVED:** Added security headers middleware to `build_http_router()` that sets `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`, `Referrer-Policy: strict-origin-when-cross-origin`, and `X-Permitted-Cross-Domain-Policies: none` on every response. Note: `Strict-Transport-Security` should be set at the reverse proxy/load balancer level, not in the app.

---

### 3.5 Broad `#![allow(...)]` Suppresses Security Warnings

**Severity:** HIGH
**File:** `apps/api/src/main.rs:17-23`

```rust
#![allow(
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    dead_code,
    unused_imports
)]
```

**Why this matters:** `clippy::all` suppresses security-relevant lints including:
- `clippy::unwrap_used` — panics in async handlers crash the server
- `clippy::indexing_slicing` — out-of-bounds panics
- `clippy::expect_used` — unhandled error paths
- `clippy::cast_possible_truncation` — integer overflow bugs

The comment says "tighten lints as API wiring stabilizes" but this is already deployed.

**Recommendation:** Replace blanket allows with targeted `#[allow(...)]` on specific items. At minimum, re-enable `clippy::all` and address warnings.

> **RESOLVED:** Replaced blanket `clippy::all, clippy::pedantic, clippy::nursery` with targeted exceptions for specific non-security lints (`module_name_repetitions`, `must_use_candidate`, etc.).

---

## 4. Medium Severity Issues

### 4.1 Database Error Messages Returned to Clients

**Severity:** MEDIUM
**File:** `apps/api/src/main.rs` (error handling paths)

Internal database errors are converted directly to HTTP 500 responses:

```rust
Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
```

**Why this matters:** SQLite and Postgres error messages can leak schema names, table structures, constraint names, and file paths. This aids SQL injection reconnaissance.

**Recommendation:** Log the full error server-side; return a generic "Internal error" message with a request ID for correlation.

> **RESOLVED:** Added `internal_error()` helper that logs the full error via `warn!()` and returns a generic `"Internal server error"` string. All `.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))` calls replaced.

---

### 4.2 No Input Validation on Path Parameters

**Severity:** MEDIUM
**File:** `apps/api/src/main.rs` (route handlers)

Run IDs, project IDs, and key IDs are extracted from URL paths with no format or length validation:

```rust
.route("/api/v1/runs/{run_id}", get(http_get_run).delete(http_delete_run))
```

**Why this matters:** While parameterized SQL prevents injection, unbounded string inputs can cause:
- Log injection (newlines in IDs)
- Storage issues (very long IDs)
- Unexpected behavior in downstream systems

**Recommendation:** Validate path parameters against a UUID or `[a-zA-Z0-9_-]{1,64}` pattern.

> **RESOLVED:** Added `validate_path_id()` function that enforces `[a-zA-Z0-9_-]{1,128}` on all path parameters (run_id, project_id, user_id, session_id, key_id).

---

### 4.3 Docker Container Runs as Root

**Severity:** MEDIUM
**File:** `Dockerfile:65-91`

The runtime stage never creates a non-root user. The API process runs as `root` inside the container.

```dockerfile
FROM debian:bookworm-slim
# ... no USER directive ...
CMD ["mlrunx-api"]
```

**Why this matters:** If the API process is compromised (e.g., via a dependency vulnerability), the attacker has root access within the container. This violates the principle of least privilege and is flagged by every container security scanner.

**Recommendation:** Add:
```dockerfile
RUN useradd -r -s /bin/false mlrunx
USER mlrunx
```

> **RESOLVED:** Added `mlrunx` system user and `USER mlrunx` directive to the Dockerfile.

---

### 4.4 SQLite File Permissions Not Enforced

**Severity:** MEDIUM
**File:** `Dockerfile:73`

```dockerfile
RUN mkdir -p /data
```

The `/data` directory (containing the SQLite database with API key hashes, sessions, and audit logs) is created with default permissions (755), readable by any process in the container.

**Recommendation:** `RUN mkdir -p /data && chmod 700 /data`

> **RESOLVED:** Updated Dockerfile to `chmod 700 /data` with `chown mlrunx:mlrunx /data`.

---

### 4.5 No API Key Expiration Policy

**Severity:** MEDIUM
**File:** `apps/api/src/auth/mod.rs` (ApiKey struct)

API keys have `created_at` and `last_used_at` but no `expires_at` field. Keys are valid forever unless manually revoked.

**Why this matters:** Long-lived credentials increase the blast radius of leaks. A key committed to a public repo 6 months ago is still valid today.

**Recommendation:** Add optional TTL support with a default of 90 days. Warn on keys older than 30 days.

> **RESOLVED:** Added `expires_at: Option<SystemTime>` to `ApiKey` struct. `is_valid()` now checks both revocation and expiration. `create_key()` accepts optional `expires_in_seconds` parameter. `CreateKeyRequest` and `CreateKeyResponse` include expiration fields. The `expires_at` column in SQLite is populated on key creation and validated on every `validate_key()` call.

---

### 4.6 Bootstrap API Key Has No Rotation Mechanism

**Severity:** MEDIUM
**File:** `apps/api/src/auth/mod.rs` (bootstrap key handling), `infra/docker/.env.example:29`

The initial `MLRUNX_API_KEY` is set via environment variable and cannot be rotated without restarting the server.

**Recommendation:** Allow key rotation via an admin API endpoint, or support reading the key from a file that can be updated without restart.

> **RESOLVED:** Added `POST /api/v1/admin/bootstrap-key/rotate` endpoint (admin-scoped). The `rotate_bootstrap_key()` method revokes the old bootstrap key (both in-memory and in SQLite via `revoke_api_key_by_id`), generates a new key, persists it via `upsert_bootstrap_api_key`, and returns the new raw key. No server restart required.

---

### 4.7 JWT Token Expiration May Not Be Enforced

**Severity:** MEDIUM
**File:** `apps/api/src/auth/mod.rs` (JWT claims struct)

The `UiJwtClaims` struct does not require an `exp` field:

```rust
#[derive(Debug, Deserialize)]
struct UiJwtClaims {
    #[serde(default)]
    sub: Option<String>,
    // no exp field
}
```

While `jsonwebtoken` validates `exp` when present, if the issuing IdP omits it, tokens become eternal.

**Recommendation:** Configure `Validation` to require `exp` and set a maximum lifetime.

> **RESOLVED:** Set `validation.validate_exp = true` in `decode_ui_jwt_claims()` to enforce JWT token expiration.

---

### 4.8 Spool Directory Permissions Unspecified (Python SDK)

**Severity:** MEDIUM
**File:** `sdks/python/src/mlrunx/config.py`

The offline spool directory defaults to `~/.mlrunx/spool/` with no permission enforcement. Other processes on the same machine can read queued experiment events.

**Recommendation:** Create the directory with mode `0700` and validate permissions on startup.

> **RESOLVED:** All `mkdir()` calls for the spool directory now use `mode=0o700` (owner-only access).

---

## 5. Low Severity / Informational Issues

### 5.1 Partial Token Logging on Auth Failure

**Severity:** LOW
**File:** `apps/api/src/auth/mod.rs`

```rust
warn!(key_prefix = %raw_key.chars().take(8).collect::<String>(), "Invalid API key");
```

Logging the first 8 characters is standard practice for correlation, but these prefixes could be useful for offline brute-force if logs are compromised.

---

### 5.2 No Request ID Propagation for Audit Trail

**Severity:** LOW
**File:** `apps/api/src/storage/sqlite.rs` (audit_events table)

The `request_id` column exists in the audit events table but is never populated, breaking log correlation.

> **RESOLVED:** Added `request_id`, `client_ip`, and `user_agent` fields to `AuthContext`. The auth middleware generates a UUIDv7 `request_id` per request and populates client IP (from `X-Forwarded-For` / `X-Real-IP`) and User-Agent. `emit_audit_event` now extracts these from `AuthContext` and passes them to `insert_audit_event`, which writes all three to the `audit_events` table. An `X-Request-Id` response header is also set on every response (including unauthenticated endpoints) via the security middleware.

---

### 5.3 No Encryption at Rest for SQLite

**Severity:** LOW
**File:** `apps/api/src/storage/sqlite.rs`

The SQLite database stores API key hashes, session tokens, and audit logs in plaintext on disk. Any filesystem access reveals all data.

**Recommendation:** For production deployments, document volume encryption requirements or integrate SQLite encryption (e.g., SQLCipher).

> **RESOLVED:** Added "Encryption at Rest" section to `docs/ops/single_domain_deployment.md` with guidance for LUKS volume encryption, Docker encrypted volume drivers, and cloud provider encrypted disk options (AWS EBS, GCP PD, Oracle Block Volumes, Azure Managed Disks).

---

### 5.4 Compression Before TLS (CRIME-like Vector)

**Severity:** INFO
**File:** `sdks/python/src/mlrunx/transport/http.py:122-132`

The SDK sends gzip-compressed payloads with `Content-Encoding: gzip`. While CRIME/BREACH attacks require attacker-controlled content mixed with secrets in the same compressed stream (unlikely here since payloads are experiment data, not secrets), it's worth noting.

---

### 5.5 Health Endpoint Unauthenticated

**Severity:** INFO
**File:** `Dockerfile:88-89`

```dockerfile
HEALTHCHECK ... CMD curl -f http://localhost:3001/health || exit 1
```

The `/health` endpoint is unauthenticated (expected), but confirms the service exists and is reachable — useful for reconnaissance.

---

## 6. Design Critique

### 6.1 Monolith With No Internal Compartmentalization

The single `mlrunx-api` binary handles auth, ingest, queries, admin, and audit in one process. A vulnerability in any handler grants access to everything. The auth module, storage layer, and ingest pipeline share the same memory space and the same database connection pool.

**Impact:** No defense-in-depth. Compromise of the ingest path (which is high-volume and externally accessible) is equivalent to compromise of the admin path.

### 6.2 Auth-Disabled Mode Is Too Easy to Reach

`MLRUNX_AUTH_DISABLED=true` disables all authentication and simultaneously opens CORS to all origins. This is a two-for-one security bypass triggered by a single env var. The flag should disable auth checks but still enforce restrictive CORS and log a persistent warning banner.

### 6.3 Configuration-Driven Security Is Fragile

Five separate environment variables control security posture:
- `MLRUNX_AUTH_DISABLED` — disables all auth
- `MLRUNX_RBAC_ENDPOINT_ENFORCEMENT_ENABLED` — disables RBAC
- `MLRUNX_UI_ALLOWED_ORIGINS` — controls CORS
- `MLRUNX_UI_JWT_*` — JWT validation settings
- `MLRUNX_API_KEY` — bootstrap key

Misconfiguring any one of these silently degrades security with no startup warning or health check failure. The system should fail-closed: refuse to start if the configuration is insecure (e.g., no API key set, RBAC disabled in non-dev mode).

### 6.4 No Audit Log Integrity Protection

Audit events are written to the same SQLite database that the API has full write access to. A compromised API can silently delete or modify its own audit trail. Audit logs should be append-only or written to a separate system.

### 6.5 SDK Trusts Server Implicitly

The Python SDK falls back to "offline mode" silently when the server is unreachable (`init_run` in `http.py:169-177`). While convenient, this means a network-level attacker who blocks connectivity can force the SDK into offline mode, causing data to accumulate locally where it may be more accessible.

---

## 7. Summary Matrix

| # | Issue | Severity | Exploitability | Fix Effort |
|---|---|---|---|---|
| 2.1 | Permissive CORS when auth disabled | CRITICAL | Trivial | Low |
| 2.2 | Hardcoded dev credentials | CRITICAL | Trivial | Low |
| 2.3 | No rate limiting on auth | CRITICAL | Easy | Medium |
| 3.1 | RBAC kill-switch env var | HIGH | Easy | Low |
| 3.2 | API key as Bearer token | HIGH | Medium | Low |
| 3.3 | Disabled user sessions survive | HIGH | Medium | Low |
| 3.4 | No security response headers | HIGH | Easy | Low |
| 3.5 | Blanket clippy suppression | HIGH | Indirect | Medium |
| 4.1 | DB errors exposed to clients | MEDIUM | Easy | Low |
| 4.2 | No input validation on paths | MEDIUM | Medium | Low |
| 4.3 | Container runs as root | MEDIUM | Requires RCE | Low |
| 4.4 | SQLite file permissions | MEDIUM | Requires access | Low |
| 4.5 | No API key expiration | MEDIUM | Requires leak | Medium |
| 4.6 | No bootstrap key rotation | MEDIUM | Requires leak | Medium |
| 4.7 | JWT exp not enforced | MEDIUM | Requires IdP issue | Low |
| 4.8 | Spool dir permissions | MEDIUM | Local access | Low |

---

## 8. Recommendations Priority

**Immediate (before any production deployment):**
1. Fix CORS to be restrictive regardless of auth state
2. Remove hardcoded credentials from `.env.example`
3. Add rate limiting to all endpoints
4. Add a non-root user to the Dockerfile
5. Set security response headers

**Short-term (next release):**
6. Remove RBAC kill-switch or make it compile-time only
7. Invalidate sessions when disabling users
8. Sanitize error messages returned to clients
9. Add path parameter validation
10. Tighten clippy lints

**Medium-term:**
11. Add API key expiration policy
12. Move to `X-API-Key` header
13. Add request ID propagation
14. Enforce spool directory permissions
15. Fail-closed on insecure configuration

---

## 9. Host Runtime Findings (From SSH MOTD, 2026-02-14)

### 9.1 Possible Unauthorized SSH Access

**Severity:** HIGH (if source IP is unrecognized)
**Evidence:** `Last login: Sat Feb 14 06:06:46 2026 from 131.186.2.55`
**Status:** OPEN

If this IP address is not expected, treat this as a potential account compromise and begin incident response immediately.

**Immediate actions:**
1. Verify login history: `last -ai | head -n 30`
2. Inspect SSH auth events around the login time:
   - `sudo journalctl -u ssh -S "2026-02-14 05:30:00" -U "2026-02-14 06:30:00"`
   - `sudo grep -E "Accepted|Failed" /var/log/auth.log | tail -n 200`
3. Rotate exposed credentials (SSH keys/passwords/API keys) if any anomaly is confirmed.
4. Temporarily restrict SSH ingress to known source IPs/security group rules.

### 9.2 Pending Security Updates

**Severity:** MEDIUM
**Evidence:** `5 updates can be applied immediately`
**Status:** OPEN

Unapplied updates increase exposure window for known CVEs.

**Remediation:**
1. `sudo apt update`
2. `sudo apt list --upgradable`
3. `sudo apt full-upgrade -y`

### 9.3 Reboot Required (Likely Kernel/Runtime Patch Not Active)

**Severity:** HIGH
**Evidence:** `*** System restart required ***`
**Status:** OPEN

Security patches requiring reboot are not active until restart.

**Remediation:**
1. Schedule maintenance window.
2. Reboot host: `sudo reboot`
3. Post-reboot verify patched kernel:
   - `uname -r`
   - `uptime -s`

### 9.4 ESM Apps Not Enabled

**Severity:** LOW to MEDIUM (depends on package set)
**Evidence:** `Expanded Security Maintenance for Applications is not enabled`
**Status:** OPEN

Without ESM Apps, some universe package security fixes may not be received after standard support windows.

**Remediation options:**
1. Enable Ubuntu Pro/ESM if this host is long-lived and internet-facing.
2. Keep package footprint minimal and monitor CVEs for installed packages.

### 9.5 Operational Hardening Follow-up

**Severity:** MEDIUM
**Status:** OPEN

Recommended baseline controls after patching:
1. Enable unattended updates: `sudo dpkg-reconfigure -plow unattended-upgrades`
2. Enforce SSH hardening (`PasswordAuthentication no`, key-only auth, optionally `PermitRootLogin no`)
3. Enable host firewall policy (`ufw` or cloud security list) for least-privilege ingress
4. Add periodic audit checks for failed SSH logins and unusual source IPs
