#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<USAGE
Run MLRunX W1 scale checks (list/filter/compare latency).

Usage:
  $0 [--scale nightly|release|stress] [--runs N] [--queries-per-type N] [--output PATH]

Defaults:
  --scale release
  --output bench/results/w1_<scale>.json

Notes:
  - Starts a local API on http://127.0.0.1:3001 with auth disabled.
  - Builds ./target/release/mlrunx-api if missing.
USAGE
}

SCALE="release"
RUNS=""
QUERIES_PER_TYPE=""
OUTPUT=""
SERVER_URL="http://127.0.0.1:3001"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --scale)
      SCALE="${2:-}"
      shift 2
      ;;
    --runs)
      RUNS="${2:-}"
      shift 2
      ;;
    --queries-per-type)
      QUERIES_PER_TYPE="${2:-}"
      shift 2
      ;;
    --output)
      OUTPUT="${2:-}"
      shift 2
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

case "$SCALE" in
  nightly|release|stress) ;;
  *)
    echo "Invalid --scale '$SCALE' (expected nightly|release|stress)" >&2
    exit 2
    ;;
esac

if [[ -z "$OUTPUT" ]]; then
  OUTPUT="bench/results/w1_${SCALE}.json"
fi

mkdir -p "$(dirname "$OUTPUT")"

API_BIN="./target/release/mlrunx-api"
if [[ ! -x "$API_BIN" ]]; then
  echo "[phase4] Building release API binary..."
  cargo build --release --bin mlrunx-api
fi

DB_PATH="/tmp/mlrunx-w1-${SCALE}-${RANDOM}.db"
LOG_FILE="/tmp/mlrunx-api-w1-${SCALE}.log"

MLRUNX_AUTH_DISABLED=true \
MLRUNX_AUTH_HMAC_SECRET=bench-local-hmac-secret \
MLRUNX_ALLOW_INSECURE_LOCAL_DEV=true \
MLRUNX_SQLITE_PATH="$DB_PATH" \
API_HOST=127.0.0.1 \
API_HTTP_PORT=3001 \
API_GRPC_PORT=50051 \
  "$API_BIN" >"$LOG_FILE" 2>&1 &
API_PID=$!

cleanup() {
  kill "$API_PID" >/dev/null 2>&1 || true
  wait "$API_PID" >/dev/null 2>&1 || true
  rm -f "$DB_PATH" "${DB_PATH}-shm" "${DB_PATH}-wal"
}
trap cleanup EXIT INT TERM

ready="false"
for i in {1..90}; do
  if curl -fsS "$SERVER_URL/health" >/dev/null 2>&1; then
    ready="true"
    break
  fi
  if ! kill -0 "$API_PID" >/dev/null 2>&1; then
    echo "[phase4] API process exited before health check. Recent logs:" >&2
    tail -n 80 "$LOG_FILE" >&2 || true
    exit 1
  fi
  sleep 1
done

if [[ "$ready" != "true" ]]; then
  echo "[phase4] API did not become healthy in time. Recent logs:" >&2
  tail -n 80 "$LOG_FILE" >&2 || true
  exit 1
fi

CMD=(
  uv run python -m bench.workloads.w1_run_scale_runner
  --scale "$SCALE"
  --server-url "$SERVER_URL"
  --output "$OUTPUT"
)
if [[ -n "$RUNS" ]]; then
  CMD+=(--runs "$RUNS")
fi
if [[ -n "$QUERIES_PER_TYPE" ]]; then
  CMD+=(--queries-per-type "$QUERIES_PER_TYPE")
fi

echo "[phase4] Running W1 benchmark (scale=$SCALE)..."
"${CMD[@]}"

echo "[phase4] Checking thresholds..."
uv run python bench/run_nightly.py check-threshold \
  --results "$OUTPUT" \
  --thresholds bench/ci_thresholds.yaml

python3 - "$OUTPUT" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, encoding="utf-8") as f:
    payload = json.load(f)

print("[phase4] Summary:")
for key in ("list_runs", "filter_tag", "filter_status", "search", "compare_runs"):
    stats = payload.get("results", {}).get(key, {})
    p95 = stats.get("p95_ms")
    p99 = stats.get("p99_ms")
    print(f"  - {key}: p95={p95}ms p99={p99}ms")
PY

echo "[phase4] Results saved to $OUTPUT"
