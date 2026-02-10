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

2. Set at least these values in `.env.prod`:
- `MLRUNX_DOMAIN`
- `MLRUNX_JWT_SECRET`
- `MLRUNX_API_KEY` (optional but recommended for API key path)

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

## Notes

- Only ports `80/443/22` need to be open publicly.
- SQLite data persists in Docker volume `mlrunx-data`.
- For RBAC rollout, follow `docs/ops/auth_rbac_rollout_runbook.md`.
