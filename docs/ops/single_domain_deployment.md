# Single-Domain Deployment (Oracle Free VM + Caddy)

This setup runs MLRunX UI and API behind one domain:

- `https://mlrunx.yourdomain.com/` -> UI
- `https://mlrunx.yourdomain.com/api/*` -> API

## Why Two Similar Domain Env Vars Exist

These are intentionally the same public domain, but they serve different purposes:

- `NEXT_PUBLIC_API_URL=https://mlrunx.yourdomain.com`
  - Used by browser-side UI code to call API endpoints.
- `MLRUNX_UI_ALLOWED_ORIGINS=https://mlrunx.yourdomain.com`
  - Used by API CORS/session checks to allow that browser origin.

There is also an internal service-to-service URL:

- `MLRUNX_API_URL=http://api:3001`
  - Used by UI server-side code inside Docker network.

## Files Added for This Layout

- `infra/docker/docker-compose.prod.yml`
- `infra/docker/.env.prod.example`
- `infra/docker/Caddyfile.prod`

## Quick Start

1. Copy and edit env file.

```bash
cp infra/docker/.env.prod.example infra/docker/.env.prod
```

2. Set required values in `.env.prod`:
- Always:
  - `MLRUNX_DOMAIN`
  - `MLRUNX_AUTH_MODE` (`hybrid` for Supabase UI login, or `api_key` for API-key-only mode)
  - `MLRUNX_API_KEY` (recommended; required for SDK/service API-key traffic)
- If `MLRUNX_AUTH_MODE=hybrid` and `MLRUNX_UI_JWT_AUTH_ENABLED=true`:
  - `NEXT_PUBLIC_SUPABASE_URL`
  - `NEXT_PUBLIC_SUPABASE_ANON_KEY`
  - `MLRUNX_JWT_ALGORITHM` (`ES256` recommended for Supabase)
  - `MLRUNX_JWT_PUBLIC_KEY_PEM` (or `MLRUNX_JWT_SECRET` only for `HS256`)
  - `MLRUNX_JWT_ISSUER` (for Supabase: `https://<project-ref>.supabase.co/auth/v1`)
  - `MLRUNX_JWT_AUDIENCE` (for Supabase: `authenticated`)
  - `MLRUNX_UI_COOKIE_DOMAIN` (optional, only when sharing cookies across subdomains)
  - Optional: `MLRUNX_UI_KEY_MAX_TTL_SECONDS` (default `7776000`, 90 days)
- If `MLRUNX_AUTH_MODE=api_key`:
  - Set `MLRUNX_UI_JWT_AUTH_ENABLED=false`

3. Ensure DNS `A` record points your domain to the VM public IP.

4. Start stack.

```bash
docker compose \
  -f infra/docker/docker-compose.prod.yml \
  --env-file infra/docker/.env.prod \
  up -d --build
```

5. Verify:
- UI: `https://mlrunx.yourdomain.com`
- API health: `https://mlrunx.yourdomain.com/health`

Cookie/session stability notes:
- In production with `MLRUNX_AUTH_MODE=hybrid`, set `MLRUNX_UI_COOKIE_SECURE=true` (required).
- `MLRUNX_UI_COOKIE_SAMESITE=None` requires `MLRUNX_UI_COOKIE_SECURE=true`.
- `MLRUNX_UI_ALLOWED_ORIGINS` values must be valid origins only (scheme + host + optional port, no paths).

## Troubleshooting

### `container mlrunx-api is unhealthy`

Most common cause: `MLRUNX_AUTH_MODE=hybrid` with missing JWT claim settings.

Run:

```bash
docker compose \
  -f infra/docker/docker-compose.prod.yml \
  --env-file infra/docker/.env.prod \
  logs api --tail=200
```

If logs include:
- `MLRUNX_JWT_ISSUER is required when UI JWT auth is enabled.`
- `MLRUNX_JWT_AUDIENCE is required when UI JWT auth is enabled.`

set those values in `.env.prod` and restart.

### 502 from Caddy/Cloudflare after API is healthy

If `api` and `ui` are healthy but edge still returns 502, check:

1. Caddy upstream connectivity:

```bash
docker compose \
  -f infra/docker/docker-compose.prod.yml \
  --env-file infra/docker/.env.prod \
  logs caddy --tail=200
```

2. Cloudflare SSL mode is `Full (strict)` and DNS `A` record points to the VM public IP.
3. VM/network allows inbound `80` and `443` to Caddy.

## Auth Login Model

### Admin login (same `/login` flow)

Use normal UI login, but designate admin identity by email:

1. Configure these API env vars in `.env.prod`:
   - `MLRUNX_AUTH_MODE=hybrid`
   - `MLRUNX_UI_JWT_AUTH_ENABLED=true`
   - `MLRUNX_ADMIN_EMAIL=<exact admin login email>`
2. Restart API after env changes (env is read on startup).
3. Log in at `/login` with the same email in `MLRUNX_ADMIN_EMAIL`.
4. After changing `MLRUNX_ADMIN_EMAIL`, sign out and sign in again to refresh the platform-admin flag.
5. If `/admin` still shows access denied, clear any browser-stored API key and retry:

```js
localStorage.removeItem('mlrunx_api_key');
```

Why this matters:
- Browser-stored API keys can override cookie-based UI session auth for admin endpoints.
- A non-admin stored key can block admin control-plane access even when the session user is platform admin.

### Standard user login

1. Use `/signup` then `/login` (email/password).
2. UI auth is session-cookie based (JWT is exchanged server-side for a UI session).
3. Users do not see Admin navigation unless their login email matches `MLRUNX_ADMIN_EMAIL`.

## API Key Policy (Recommended)

Use sessions for browser UI access, and API keys for SDK/service traffic.

1. Keep UI auth as session-cookie only (avoid long-lived API keys in browser storage).
2. Create project-scoped SDK keys (`read`/`write`) per project and environment (`dev`, `staging`, `prod`).
3. Do not use one user key for all projects; for multi-project users, create separate keys per project.
4. Do not grant admin-scoped keys to regular users.
5. Reserve global admin keys (`project_id=null`, `admin` scope) for break-glass automation only.
6. Store keys in a secret manager, rotate on schedule, and revoke immediately on exposure.

Current enforcement for UI-created keys:
- `project_id` is required.
- `name` is required and must follow lowercase slash/hyphen convention.
- `expires_in_seconds` is required and capped by `MLRUNX_UI_KEY_MAX_TTL_SECONDS`.

## Notes

- Only ports `80/443/22` need to be open publicly.
- SQLite data persists in Docker volume `mlrunx-data`.
- For RBAC rollout, follow `docs/ops/auth_rbac_rollout_runbook.md`.

## Encryption at Rest

The SQLite database stores API key hashes, session tokens, user records, and audit logs. While key material is hashed (SHA-256 / argon2id), the plaintext metadata (user IDs, project names, audit trails) is sensitive. Protect the data volume with encryption at rest.

### Linux (recommended for production VMs)

Use LUKS to encrypt the volume backing the Docker data directory:

```bash
# Create an encrypted volume (one-time setup)
sudo cryptsetup luksFormat /dev/sdX
sudo cryptsetup luksOpen /dev/sdX mlrunx-data
sudo mkfs.ext4 /dev/mapper/mlrunx-data
sudo mount /dev/mapper/mlrunx-data /var/lib/mlrunx

# Mount on boot via /etc/crypttab + /etc/fstab
```

### Docker with encrypted volume driver

If using Docker managed volumes, use an encrypted volume driver or bind-mount from an encrypted filesystem:

```bash
docker volume create --driver local \
  --opt type=none \
  --opt device=/var/lib/mlrunx \
  --opt o=bind \
  mlrunx-data
```

### Cloud providers

- **AWS:** Use encrypted EBS volumes (`--encrypted` flag).
- **GCP:** Persistent disks are encrypted by default (CMEK available).
- **Oracle Cloud:** Use encrypted block volumes (enabled by default on OCI).
- **Azure:** Managed disks use server-side encryption by default (CMEK available).

### Verification

Confirm the data directory is on an encrypted partition:

```bash
# Check LUKS status
sudo cryptsetup status mlrunx-data

# Check if mount point is encrypted
lsblk -o NAME,FSTYPE,SIZE,MOUNTPOINT,CRYPT
```
