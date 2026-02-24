#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<USAGE
Run production preflight checks for MLRunX VM deployment.

Usage:
  $0 [--env-file PATH] [--base-url URL] [--skip-isolation]

Defaults:
  --env-file infra/docker/.env.prod
  --base-url derived from MLRUNX_DOMAIN in env file (https://<domain>)

Checks:
  1) docker compose prod config validates
  2) health endpoint is reachable
  3) unauthenticated /api/v1/runs is denied
  4) two-key isolation smoke (requires MLRUNX_API_KEY in env file)
USAGE
}

ENV_FILE="infra/docker/.env.prod"
BASE_URL=""
SKIP_ISOLATION="false"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --env-file)
      ENV_FILE="${2:-}"
      shift 2
      ;;
    --base-url)
      BASE_URL="${2:-}"
      shift 2
      ;;
    --skip-isolation)
      SKIP_ISOLATION="true"
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage
      exit 2
      ;;
  esac
done

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

if [[ ! -f "$ENV_FILE" ]]; then
  echo "[preflight] Env file not found: $ENV_FILE" >&2
  exit 1
fi

set -a
# shellcheck disable=SC1090
source "$ENV_FILE"
set +a

if [[ -z "$BASE_URL" ]]; then
  if [[ -n "${MLRUNX_BASE_URL:-}" ]]; then
    BASE_URL="$MLRUNX_BASE_URL"
  elif [[ -n "${MLRUNX_DOMAIN:-}" ]]; then
    BASE_URL="https://${MLRUNX_DOMAIN}"
  else
    echo "[preflight] Missing base URL. Set --base-url or MLRUNX_DOMAIN in $ENV_FILE." >&2
    exit 1
  fi
fi
BASE_URL="${BASE_URL%/}"

COMPOSE_FILE="infra/docker/docker-compose.prod.yml"

echo "[preflight] Validating compose config..."
docker compose -f "$COMPOSE_FILE" --env-file "$ENV_FILE" config >/dev/null
echo "[preflight] Compose config OK"

echo "[preflight] Checking health endpoint: $BASE_URL/health"
curl -fsS --max-time 10 "$BASE_URL/health" >/dev/null
echo "[preflight] Health check OK"

echo "[preflight] Checking unauthenticated API access is denied"
UNAUTH_CODE="$(curl -sS -o /dev/null -w "%{http_code}" --max-time 10 "$BASE_URL/api/v1/runs")"
case "$UNAUTH_CODE" in
  401|403)
    echo "[preflight] Unauthenticated runs endpoint denied as expected ($UNAUTH_CODE)"
    ;;
  *)
    echo "[preflight] Expected 401/403 from unauthenticated /api/v1/runs, got $UNAUTH_CODE" >&2
    exit 1
    ;;
esac

if [[ "$SKIP_ISOLATION" == "true" ]]; then
  echo "[preflight] Skipping two-key isolation smoke (--skip-isolation)."
  echo "[preflight] PASS"
  exit 0
fi

ADMIN_KEY="${MLRUNX_API_KEY:-}"
if [[ -z "$ADMIN_KEY" ]]; then
  echo "[preflight] MLRUNX_API_KEY is required for two-key isolation smoke." >&2
  exit 1
fi

json_field() {
  local field="$1"
  python3 - "$field" <<'PY'
import json
import sys

field = sys.argv[1]
payload = json.load(sys.stdin)
value = payload.get(field)
if isinstance(value, str):
    print(value)
PY
}

api_post_admin() {
  local path="$1"
  local data="$2"
  curl -sS --fail-with-body \
    -X POST "$BASE_URL$path" \
    -H "x-api-key: $ADMIN_KEY" \
    -H "content-type: application/json" \
    -d "$data"
}

api_delete_admin() {
  local path="$1"
  curl -sS --fail-with-body \
    -X DELETE "$BASE_URL$path" \
    -H "x-api-key: $ADMIN_KEY" >/dev/null
}

created_project_a=""
created_project_b=""

cleanup() {
  if [[ -n "$created_project_a" ]]; then
    api_delete_admin "/api/v1/projects/$created_project_a" || true
  fi
  if [[ -n "$created_project_b" ]]; then
    api_delete_admin "/api/v1/projects/$created_project_b" || true
  fi
}
trap cleanup EXIT

suffix="$(date +%s)"

create_project_response_a="$(api_post_admin "/api/v1/projects" "{\"name\":\"preflight-a-$suffix\"}")"
create_project_response_b="$(api_post_admin "/api/v1/projects" "{\"name\":\"preflight-b-$suffix\"}")"
created_project_a="$(printf '%s' "$create_project_response_a" | json_field "project_id")"
created_project_b="$(printf '%s' "$create_project_response_b" | json_field "project_id")"

if [[ -z "$created_project_a" || -z "$created_project_b" ]]; then
  echo "[preflight] Failed to create smoke-test projects." >&2
  exit 1
fi

echo "[preflight] Created projects for isolation smoke"

create_key_response_a="$(api_post_admin "/api/v1/keys" "{\"project_id\":\"$created_project_a\",\"name\":\"preflight/a-$suffix\",\"scopes\":[\"read\",\"write\"]}")"
create_key_response_b="$(api_post_admin "/api/v1/keys" "{\"project_id\":\"$created_project_b\",\"name\":\"preflight/b-$suffix\",\"scopes\":[\"read\",\"write\"]}")"

key_a="$(printf '%s' "$create_key_response_a" | json_field "api_key")"
key_b="$(printf '%s' "$create_key_response_b" | json_field "api_key")"

if [[ -z "$key_a" || -z "$key_b" ]]; then
  echo "[preflight] Failed to create smoke-test API keys." >&2
  exit 1
fi

run_a_response="$(curl -sS --fail-with-body \
  -X POST "$BASE_URL/api/v1/runs" \
  -H "x-api-key: $key_a" \
  -H "content-type: application/json" \
  -d "{\"project_id\":\"$created_project_a\",\"name\":\"preflight-run-a\"}")"
run_b_response="$(curl -sS --fail-with-body \
  -X POST "$BASE_URL/api/v1/runs" \
  -H "x-api-key: $key_b" \
  -H "content-type: application/json" \
  -d "{\"project_id\":\"$created_project_b\",\"name\":\"preflight-run-b\"}")"

run_a="$(printf '%s' "$run_a_response" | json_field "run_id")"
run_b="$(printf '%s' "$run_b_response" | json_field "run_id")"

if [[ -z "$run_a" || -z "$run_b" ]]; then
  echo "[preflight] Failed to create smoke-test runs." >&2
  exit 1
fi

own_read_code="$(curl -sS -o /dev/null -w "%{http_code}" -H "x-api-key: $key_a" "$BASE_URL/api/v1/runs/$run_a")"
if [[ "$own_read_code" != "200" ]]; then
  echo "[preflight] Expected key A to read its own run (200), got $own_read_code" >&2
  exit 1
fi

cross_read_code="$(curl -sS -o /dev/null -w "%{http_code}" -H "x-api-key: $key_b" "$BASE_URL/api/v1/runs/$run_a")"
case "$cross_read_code" in
  401|403|404)
    echo "[preflight] Cross-run read blocked as expected ($cross_read_code)"
    ;;
  *)
    echo "[preflight] Expected cross-run read to be denied, got $cross_read_code" >&2
    exit 1
    ;;
esac

cross_delete_code="$(curl -sS -o /dev/null -w "%{http_code}" -X DELETE -H "x-api-key: $key_b" "$BASE_URL/api/v1/runs/$run_a")"
case "$cross_delete_code" in
  401|403|404)
    echo "[preflight] Cross-run delete blocked as expected ($cross_delete_code)"
    ;;
  *)
    echo "[preflight] Expected cross-run delete to be denied, got $cross_delete_code" >&2
    exit 1
    ;;
esac

echo "[preflight] Two-key isolation smoke passed"
echo "[preflight] PASS"
