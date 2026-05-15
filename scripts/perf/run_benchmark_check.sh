#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Run an MLRunX benchmark workload against a local API and check thresholds.

Usage:
  run_benchmark_check.sh --workload w1|w2|w3 [--scale nightly|release|stress] [--output PATH] [--api-bin PATH] [--server-url URL] [--health-timeout SECONDS] [-- ...extra runner args]

Defaults:
  --scale nightly
  --output bench/results/<workload>_<scale>.json
  --api-bin ./target/release/mlrunx-api
  --server-url http://127.0.0.1:3001
  --health-timeout 120

Notes:
  - Builds ./target/release/mlrunx-api if the default binary is missing.
  - Starts a local API with auth disabled and SQLite storage.
USAGE
}

WORKLOAD=""
SCALE="nightly"
OUTPUT=""
API_BIN="./target/release/mlrunx-api"
SERVER_URL="http://127.0.0.1:3001"
HEALTH_TIMEOUT="120"
EXTRA_ARGS=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --workload)
      WORKLOAD="${2:-}"
      shift 2
      ;;
    --scale)
      SCALE="${2:-}"
      shift 2
      ;;
    --output)
      OUTPUT="${2:-}"
      shift 2
      ;;
    --api-bin)
      API_BIN="${2:-}"
      shift 2
      ;;
    --server-url)
      SERVER_URL="${2:-}"
      shift 2
      ;;
    --health-timeout)
      HEALTH_TIMEOUT="${2:-}"
      shift 2
      ;;
    --)
      shift
      EXTRA_ARGS+=("$@")
      break
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

case "$WORKLOAD" in
  w1)
    RUNNER_MODULE="bench.workloads.w1_run_scale_runner"
    ;;
  w2)
    RUNNER_MODULE="bench.workloads.w2_ingest_latency_runner"
    ;;
  w3)
    RUNNER_MODULE="bench.workloads.w3_mixed_dashboard_runner"
    ;;
  *)
    echo "Invalid or missing --workload '$WORKLOAD' (expected w1|w2|w3)" >&2
    exit 2
    ;;
esac

case "$SCALE" in
  nightly|release|stress) ;;
  *)
    echo "Invalid --scale '$SCALE' (expected nightly|release|stress)" >&2
    exit 2
    ;;
esac

if [[ -z "$OUTPUT" ]]; then
  OUTPUT="bench/results/${WORKLOAD}_${SCALE}.json"
fi

mkdir -p "$(dirname "$OUTPUT")"

if [[ "$API_BIN" == "./target/release/mlrunx-api" && ! -x "$API_BIN" ]]; then
  echo "[bench] Building release API binary..."
  cargo build --release --bin mlrunx-api
fi

if [[ ! -x "$API_BIN" ]]; then
  echo "API binary not found or not executable: $API_BIN" >&2
  exit 1
fi

DB_PATH="/tmp/mlrunx-${WORKLOAD}-${SCALE}-${RANDOM}.db"
LOG_FILE="/tmp/mlrunx-api-${WORKLOAD}-${SCALE}.log"

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
for ((i=1; i<=HEALTH_TIMEOUT; i++)); do
  if curl -fsS "$SERVER_URL/health" >/dev/null 2>&1; then
    ready="true"
    break
  fi
  if ! kill -0 "$API_PID" >/dev/null 2>&1; then
    echo "[bench] API process exited before health check. Recent logs:" >&2
    tail -n 80 "$LOG_FILE" >&2 || true
    exit 1
  fi
  sleep 1
done

if [[ "$ready" != "true" ]]; then
  echo "[bench] API did not become healthy in ${HEALTH_TIMEOUT}s. Recent logs:" >&2
  tail -n 80 "$LOG_FILE" >&2 || true
  exit 1
fi

CMD=(
  uv run python -m "$RUNNER_MODULE"
  --scale "$SCALE"
  --server-url "$SERVER_URL"
  --output "$OUTPUT"
)

if [[ ${#EXTRA_ARGS[@]} -gt 0 ]]; then
  CMD+=("${EXTRA_ARGS[@]}")
fi

echo "[bench] Running ${WORKLOAD} benchmark (scale=${SCALE})..."
"${CMD[@]}"

echo "[bench] Checking thresholds..."
uv run python bench/run_nightly.py check-threshold \
  --results "$OUTPUT" \
  --thresholds bench/ci_thresholds.yaml

python3 - "$OUTPUT" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, encoding="utf-8") as f:
    payload = json.load(f)

print("[bench] Summary:")
for key, stats in payload.get("results", {}).items():
    if not isinstance(stats, dict):
        continue
    p95 = stats.get("p95_ms", stats.get("p95"))
    p99 = stats.get("p99_ms", stats.get("p99"))
    print(f"  - {key}: p95={p95} p99={p99}")
PY

echo "[bench] Results saved to $OUTPUT"
