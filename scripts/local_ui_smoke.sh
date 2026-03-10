#!/usr/bin/env bash
set -euo pipefail

# Local UI/API smoke test:
# - Validates API and optional UI health
# - Seeds a JWT user + project membership in SQLite
# - Logs in via /api/v1/ui-auth/login and validates session cookie flow
# - Creates/lists/revokes API keys through UI auth endpoints
# - Verifies SDK-style run -> ingest -> finish flow via API key
#
# Usage:
#   bash scripts/local_ui_smoke.sh
#
# Optional env:
#   API_BASE_URL=http://127.0.0.1:3001
#   UI_BASE_URL=http://127.0.0.1:3000
#   MLRUNX_SQLITE_PATH=/tmp/mlrunx-local.db
#   MLRUNX_JWT_SECRET=local-dev-jwt-secret
#   JWT_SUBJECT=example-ui-test
#   JWT_EMAIL=you@example.com
#   JWT_NAME=Example User
#   PROJECT_NAME=demo
#   VERBOSE=1

API_BASE_URL="${API_BASE_URL:-http://127.0.0.1:3001}"
UI_BASE_URL="${UI_BASE_URL:-http://127.0.0.1:3000}"
SQLITE_PATH="${MLRUNX_SQLITE_PATH:-/tmp/mlrunx-local.db}"
JWT_SECRET="${MLRUNX_JWT_SECRET:-local-dev-jwt-secret}"
JWT_SUBJECT="${JWT_SUBJECT:-example-ui-test}"
JWT_EMAIL="${JWT_EMAIL:-you@example.com}"
JWT_NAME="${JWT_NAME:-Example User}"
PROJECT_NAME="${PROJECT_NAME:-demo}"
VERBOSE="${VERBOSE:-0}"

PASS_COUNT=0
FAIL_COUNT=0

WORKDIR="$(mktemp -d /tmp/mlrunx-smoke.XXXXXX)"
COOKIE_JAR="${WORKDIR}/cookies.txt"
trap 'rm -rf "${WORKDIR}"' EXIT

pass() {
  PASS_COUNT=$((PASS_COUNT + 1))
  printf "PASS %s\n" "$1"
}

fail() {
  FAIL_COUNT=$((FAIL_COUNT + 1))
  printf "FAIL %s\n" "$1"
}

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing required command: $1"
    exit 2
  fi
}

debug_dump() {
  if [[ "${VERBOSE}" == "1" ]]; then
    echo "---- ${1} ----"
    cat "$2"
    echo "--------------"
  fi
}

http_call() {
  # Args: method url body_file_or_dash output_body_file extra curl args...
  local method="$1"
  local url="$2"
  local body_file="$3"
  local out_file="$4"
  shift 4

  if [[ "${body_file}" == "-" ]]; then
    curl -sS -X "${method}" "$url" -o "${out_file}" -w "%{http_code}" "$@"
  else
    curl -sS -X "${method}" "$url" --data-binary "@${body_file}" -o "${out_file}" -w "%{http_code}" "$@"
  fi
}

require_cmd curl
require_cmd sqlite3
require_cmd python3

echo "Running MLRunX local smoke checks against:"
echo "  API: ${API_BASE_URL}"
echo "  UI : ${UI_BASE_URL}"
echo "  DB : ${SQLITE_PATH}"

# -----------------------------------------------------------------------------
# 1) API health
# -----------------------------------------------------------------------------
health_body="${WORKDIR}/health.body"
health_code="$(http_call GET "${API_BASE_URL}/health" - "${health_body}")"
if [[ "${health_code}" == "200" ]] && grep -q "ok" "${health_body}"; then
  pass "API /health is reachable"
else
  debug_dump "health response" "${health_body}"
  fail "API /health check failed (status=${health_code})"
fi

# -----------------------------------------------------------------------------
# 2) UI health (optional but recommended)
# -----------------------------------------------------------------------------
ui_body="${WORKDIR}/ui.body"
ui_code="$(http_call GET "${UI_BASE_URL}/" - "${ui_body}")"
if [[ "${ui_code}" == "200" ]]; then
  pass "UI root is reachable"
else
  debug_dump "ui response" "${ui_body}"
  fail "UI root check failed (status=${ui_code})"
fi

# -----------------------------------------------------------------------------
# 3) Seed local auth records in SQLite
# -----------------------------------------------------------------------------
seed_json="${WORKDIR}/seed.json"
export SQLITE_PATH JWT_SUBJECT JWT_EMAIL JWT_NAME PROJECT_NAME seed_json
python3 - <<'PY'
import json
import os
import sqlite3
import uuid

db = os.environ["SQLITE_PATH"]
subject = os.environ["JWT_SUBJECT"]
email = os.environ["JWT_EMAIL"]
name = os.environ["JWT_NAME"]
project_name = os.environ["PROJECT_NAME"]
out = os.environ["seed_json"]

con = sqlite3.connect(db)
cur = con.cursor()

cur.execute(
    "INSERT OR IGNORE INTO projects (id, name) VALUES (?, ?)",
    (uuid.uuid4().hex, project_name),
)
cur.execute("SELECT id FROM projects WHERE name = ?", (project_name,))
row = cur.fetchone()
if row is None:
    raise RuntimeError("project not found after insert")
project_id = row[0]

cur.execute(
    "SELECT id FROM users WHERE auth_provider = ? AND external_subject = ?",
    ("jwt", subject),
)
row = cur.fetchone()
if row is None:
    user_id = uuid.uuid4().hex
    cur.execute(
        """
        INSERT INTO users (id, email, display_name, auth_provider, external_subject)
        VALUES (?, ?, ?, ?, ?)
        """,
        (user_id, email, name, "jwt", subject),
    )
else:
    user_id = row[0]

cur.execute(
    """
    UPDATE project_memberships
    SET revoked_at = datetime('now'), updated_at = datetime('now')
    WHERE project_id = ? AND user_id = ? AND revoked_at IS NULL
    """,
    (project_id, user_id),
)
cur.execute(
    """
    INSERT INTO project_memberships (id, project_id, user_id, role, granted_by_user_id)
    VALUES (?, ?, ?, 'owner', NULL)
    """,
    (uuid.uuid4().hex, project_id, user_id),
)

con.commit()
con.close()

with open(out, "w", encoding="utf-8") as f:
    json.dump({"project_id": project_id, "user_id": user_id}, f)
PY

if [[ -s "${seed_json}" ]]; then
  pass "Seeded user and project membership in SQLite"
else
  fail "Failed to seed user/project membership"
fi

PROJECT_ID="$(python3 - <<'PY'
import json, os
with open(os.environ["seed_json"], encoding="utf-8") as f:
    data = json.load(f)
print(data["project_id"])
PY
)"

# -----------------------------------------------------------------------------
# 4) Generate JWT
# -----------------------------------------------------------------------------
jwt_token="$(
  JWT_SECRET="${JWT_SECRET}" JWT_SUBJECT="${JWT_SUBJECT}" JWT_EMAIL="${JWT_EMAIL}" JWT_NAME="${JWT_NAME}" \
  python3 - <<'PY'
import base64
import hashlib
import hmac
import json
import os
import time

secret = os.environ["JWT_SECRET"].encode()
claims = {
    "sub": os.environ["JWT_SUBJECT"],
    "email": os.environ["JWT_EMAIL"],
    "name": os.environ["JWT_NAME"],
    "exp": int(time.time()) + 3600,
}

header = {"alg": "HS256", "typ": "JWT"}

def b64(data: bytes) -> bytes:
    return base64.urlsafe_b64encode(data).rstrip(b"=")

h = b64(json.dumps(header, separators=(",", ":")).encode())
p = b64(json.dumps(claims, separators=(",", ":")).encode())
s = b64(hmac.new(secret, h + b"." + p, hashlib.sha256).digest())
print((h + b"." + p + b"." + s).decode())
PY
)"

if [[ -n "${jwt_token}" ]]; then
  pass "Generated JWT for local session login"
else
  fail "JWT generation failed"
fi

# -----------------------------------------------------------------------------
# 5) Login UI session
# -----------------------------------------------------------------------------
login_payload="${WORKDIR}/login.json"
python3 - <<'PY' > "${login_payload}"
import json
import os
print(json.dumps({"jwt": os.environ["jwt_token"]}))
PY

login_body="${WORKDIR}/login.body"
login_code="$(http_call POST "${API_BASE_URL}/api/v1/ui-auth/login" "${login_payload}" "${login_body}" \
  -c "${COOKIE_JAR}" -H "content-type: application/json")"

if [[ "${login_code}" == "200" ]]; then
  pass "UI auth login endpoint accepted JWT"
else
  debug_dump "login response" "${login_body}"
  fail "UI auth login failed (status=${login_code})"
fi

csrf_token="$(awk '$6=="mlrunx_ui_csrf"{print $7}' "${COOKIE_JAR}" | tail -1)"
session_cookie="$(awk '$6=="mlrunx_ui_session"{print $7}' "${COOKIE_JAR}" | tail -1)"

if [[ -n "${csrf_token}" && -n "${session_cookie}" ]]; then
  pass "Session + CSRF cookies were issued"
else
  fail "Missing session/CSRF cookies after login"
fi

# -----------------------------------------------------------------------------
# 6) Session status check
# -----------------------------------------------------------------------------
session_body="${WORKDIR}/session.body"
session_code="$(http_call GET "${API_BASE_URL}/api/v1/ui-auth/session" - "${session_body}" -b "${COOKIE_JAR}")"
if [[ "${session_code}" == "200" ]]; then
  if SESSION_BODY_FILE="${session_body}" EXPECT_PROJECT_ID="${PROJECT_ID}" python3 - <<'PY'
import json
import os
with open(os.environ["SESSION_BODY_FILE"], encoding="utf-8") as f:
    body = json.load(f)
assert body.get("authenticated") is True
projects = body.get("project_ids", [])
assert os.environ["EXPECT_PROJECT_ID"] in projects
PY
  then
    pass "Session endpoint reports authenticated user with project access"
  else
    debug_dump "session response" "${session_body}"
    fail "Session endpoint payload invalid"
  fi
else
  debug_dump "session response" "${session_body}"
  fail "Session endpoint failed (status=${session_code})"
fi

# -----------------------------------------------------------------------------
# 7) Create project-scoped API key
# -----------------------------------------------------------------------------
create_key_payload="${WORKDIR}/create_key.json"
PROJECT_ID="${PROJECT_ID}" python3 - <<'PY' > "${create_key_payload}"
import json
import os
print(json.dumps({
    "project_id": os.environ["PROJECT_ID"],
    "name": "local-smoke-key",
    "scopes": ["read", "write"]
}))
PY

create_key_body="${WORKDIR}/create_key.body"
create_key_code="$(http_call POST "${API_BASE_URL}/api/v1/keys" "${create_key_payload}" "${create_key_body}" \
  -b "${COOKIE_JAR}" -H "content-type: application/json" -H "x-csrf-token: ${csrf_token}")"

key_id=""
api_key=""
if [[ "${create_key_code}" == "200" ]]; then
  if CREATE_KEY_BODY_FILE="${create_key_body}" python3 - <<'PY' > "${WORKDIR}/created_key.txt"
import json
import os
with open(os.environ["CREATE_KEY_BODY_FILE"], encoding="utf-8") as f:
    body = json.load(f)
key_id = body.get("key_id", "")
api_key = body.get("api_key", "")
assert key_id
assert isinstance(api_key, str) and api_key.startswith("mlrunx_")
print(key_id)
print(api_key)
PY
  then
    key_id="$(sed -n '1p' "${WORKDIR}/created_key.txt")"
    api_key="$(sed -n '2p' "${WORKDIR}/created_key.txt")"
    pass "Create API key succeeded via UI session"
  else
    debug_dump "create key response" "${create_key_body}"
    fail "Create API key payload invalid"
  fi
else
  debug_dump "create key response" "${create_key_body}"
  fail "Create API key failed (status=${create_key_code})"
fi

# -----------------------------------------------------------------------------
# 8) List API keys includes newly created key
# -----------------------------------------------------------------------------
list_keys_body="${WORKDIR}/list_keys.body"
list_keys_code="$(http_call GET "${API_BASE_URL}/api/v1/keys" - "${list_keys_body}" -b "${COOKIE_JAR}")"
if [[ "${list_keys_code}" == "200" ]]; then
  if LIST_KEYS_BODY_FILE="${list_keys_body}" EXPECT_KEY_ID="${key_id}" python3 - <<'PY'
import json
import os
with open(os.environ["LIST_KEYS_BODY_FILE"], encoding="utf-8") as f:
    body = json.load(f)
keys = body.get("keys", [])
assert any(k.get("key_id") == os.environ["EXPECT_KEY_ID"] for k in keys)
PY
  then
    pass "List API keys includes created key"
  else
    debug_dump "list keys response" "${list_keys_body}"
    fail "Created key not found in list"
  fi
else
  debug_dump "list keys response" "${list_keys_body}"
  fail "List API keys failed (status=${list_keys_code})"
fi

# -----------------------------------------------------------------------------
# 9) Run ingest flow with created API key
# -----------------------------------------------------------------------------
run_create_payload="${WORKDIR}/run_create.json"
PROJECT_NAME="${PROJECT_NAME}" python3 - <<'PY' > "${run_create_payload}"
import json
import os
print(json.dumps({"project": os.environ["PROJECT_NAME"], "name": "local-smoke-run"}))
PY

run_create_body="${WORKDIR}/run_create.body"
run_create_code="$(http_call POST "${API_BASE_URL}/api/v1/runs" "${run_create_payload}" "${run_create_body}" \
  -H "content-type: application/json" -H "x-api-key: ${api_key}")"

run_id=""
if [[ "${run_create_code}" == "200" ]]; then
  if RUN_CREATE_BODY_FILE="${run_create_body}" python3 - <<'PY' > "${WORKDIR}/run_id.txt"
import json
import os
with open(os.environ["RUN_CREATE_BODY_FILE"], encoding="utf-8") as f:
    body = json.load(f)
run_id = body.get("run_id", "")
assert run_id
print(run_id)
PY
  then
    run_id="$(cat "${WORKDIR}/run_id.txt")"
    pass "Run initialization via API key succeeded"
  else
    debug_dump "run create response" "${run_create_body}"
    fail "Run create payload invalid"
  fi
else
  debug_dump "run create response" "${run_create_body}"
  fail "Run create failed (status=${run_create_code})"
fi

ingest_payload="${WORKDIR}/ingest.json"
RUN_ID="${run_id}" python3 - <<'PY' > "${ingest_payload}"
import json
import os
print(json.dumps({
    "run_id": os.environ["RUN_ID"],
    "batch_id": "smoke-batch-1",
    "seq": 1,
    "metrics": [{"name": "loss", "value": 0.42, "step": 1}],
    "params": [],
    "tags": []
}))
PY

ingest_body="${WORKDIR}/ingest.body"
ingest_code="$(http_call POST "${API_BASE_URL}/api/v1/ingest/batch" "${ingest_payload}" "${ingest_body}" \
  -H "content-type: application/json" -H "x-api-key: ${api_key}")"
if [[ "${ingest_code}" == "200" ]]; then
  pass "Batch ingestion via API key succeeded"
else
  debug_dump "ingest response" "${ingest_body}"
  fail "Batch ingestion failed (status=${ingest_code})"
fi

finish_payload="${WORKDIR}/finish.json"
printf '{"status":"finished"}' > "${finish_payload}"
finish_body="${WORKDIR}/finish.body"
finish_code="$(http_call POST "${API_BASE_URL}/api/v1/runs/${run_id}/finish" "${finish_payload}" "${finish_body}" \
  -H "content-type: application/json" -H "x-api-key: ${api_key}")"
if [[ "${finish_code}" == "200" ]]; then
  pass "Run finish via API key succeeded"
else
  debug_dump "finish response" "${finish_body}"
  fail "Run finish failed (status=${finish_code})"
fi

# -----------------------------------------------------------------------------
# 10) Revoke key and verify key is marked revoked
# -----------------------------------------------------------------------------
revoke_body="${WORKDIR}/revoke.body"
revoke_code="$(http_call DELETE "${API_BASE_URL}/api/v1/keys/${key_id}" - "${revoke_body}" \
  -b "${COOKIE_JAR}" -H "x-csrf-token: ${csrf_token}")"
if [[ "${revoke_code}" == "200" ]]; then
  pass "Revoke API key succeeded"
else
  debug_dump "revoke response" "${revoke_body}"
  fail "Revoke API key failed (status=${revoke_code})"
fi

list_keys_after_body="${WORKDIR}/list_keys_after.body"
list_keys_after_code="$(http_call GET "${API_BASE_URL}/api/v1/keys" - "${list_keys_after_body}" -b "${COOKIE_JAR}")"
if [[ "${list_keys_after_code}" == "200" ]]; then
  if LIST_KEYS_BODY_FILE="${list_keys_after_body}" EXPECT_KEY_ID="${key_id}" python3 - <<'PY'
import json
import os
with open(os.environ["LIST_KEYS_BODY_FILE"], encoding="utf-8") as f:
    body = json.load(f)
keys = body.get("keys", [])
matches = [k for k in keys if k.get("key_id") == os.environ["EXPECT_KEY_ID"]]
assert matches
assert matches[0].get("is_revoked") is True
PY
  then
    pass "Revoked key is flagged as revoked"
  else
    debug_dump "list keys after revoke response" "${list_keys_after_body}"
    fail "Revoked key not flagged correctly"
  fi
else
  debug_dump "list keys after revoke response" "${list_keys_after_body}"
  fail "List API keys after revoke failed (status=${list_keys_after_code})"
fi

echo
echo "Smoke summary: PASS=${PASS_COUNT} FAIL=${FAIL_COUNT}"
if [[ "${FAIL_COUNT}" -gt 0 ]]; then
  exit 1
fi

echo "All smoke checks passed."
