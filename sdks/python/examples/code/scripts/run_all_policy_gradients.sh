#!/usr/bin/env bash
# Run policy gradient algorithms with torchrun (sequential configs).
# Algorithms: RLOO, PPO, GRPO, Dr. GRPO, GSPO, CISPO, REINFORCE.

#SBATCH --job-name=policy-gradients
#SBATCH --partition=bigTiger
#SBATCH --nodes=1
#SBATCH --ntasks-per-node=1
#SBATCH --gres=gpu:8
#SBATCH --cpus-per-task=40
#SBATCH --mem=300G
#SBATCH --output=slurm-%j.out
#SBATCH --error=slurm-%j.err

set -euo pipefail

# If launched directly from a login node, submit this script through Slurm.
# If already inside a Slurm allocation, continue with training.
if [[ -z "${SLURM_JOB_ID:-}" ]]; then
    if ! command -v sbatch >/dev/null 2>&1; then
        echo "sbatch is not available in PATH."
        echo "Run this script on a Slurm login node."
        exit 1
    fi
    echo "No active Slurm job detected. Submitting via sbatch..."
    sbatch "$0" "$@"
    exit $?
fi

if [[ -n "${SLURM_SUBMIT_DIR:-}" ]]; then
    cd "$SLURM_SUBMIT_DIR"
else
    cd "$(dirname "$0")/.."
fi

PROJECT_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || true)"
if [[ -z "$PROJECT_ROOT" ]]; then
    PROJECT_ROOT="$(cd ../../../../ && pwd)"
fi

PYTHON_BIN="$PROJECT_ROOT/.venv/bin/python"

if [[ ! -x "$PYTHON_BIN" ]]; then
    echo "Project virtualenv not found at: $PROJECT_ROOT/.venv"
    echo "Create and sync it first:"
    echo "  cd $PROJECT_ROOT"
    echo "  uv sync --all-packages"
    exit 1
fi

check_policy_gradient_deps() {
    "$PYTHON_BIN" - <<'PY'
import importlib
import sys

if sys.version_info < (3, 12):
    print(f"Python 3.12+ is required, found: {sys.version.split()[0]}")
    print("Recreate MLRunX/.venv with Python 3.12+.")
    sys.exit(1)

required = {
    "numpy": "numpy",
    "torch": "torch",
    "transformers": "transformers",
    "reasoning-gym": "reasoning_gym",
    "rich": "rich",
    "pyyaml": "yaml",
    "mlrunx": "mlrunx",
}

missing = []
for package_name, import_name in required.items():
    try:
        importlib.import_module(import_name)
    except Exception:
        missing.append(package_name)

if missing:
    print("Missing required packages for policy_gradients:")
    print("  " + ", ".join(missing))
    print("")
    print("Install them into the MLRunX project venv, then rerun:")
    print(f"  uv pip install --python {sys.executable} " + " ".join(missing))
    sys.exit(1)
PY
}

check_policy_gradient_deps

infer_nproc_per_node() {
    local raw="${NPROC_PER_NODE:-}"
    if [[ -z "$raw" && -n "${SLURM_GPUS_ON_NODE:-}" ]]; then
        raw="${SLURM_GPUS_ON_NODE}"
    fi
    if [[ -z "$raw" && -n "${SLURM_GPUS_PER_NODE:-}" ]]; then
        raw="${SLURM_GPUS_PER_NODE}"
    fi
    if [[ -z "$raw" ]]; then
        raw="1"
    fi
    if [[ "$raw" =~ ^[0-9]+$ ]]; then
        echo "$raw"
        return
    fi
    if [[ "$raw" =~ ^[0-9]+(,[0-9]+)+$ ]]; then
        local ids
        IFS=',' read -r -a ids <<<"$raw"
        echo "${#ids[@]}"
        return
    fi
    if [[ "$raw" == *:* ]]; then
        local tail="${raw##*:}"
        tail="${tail%%(*}"
        if [[ "$tail" =~ ^[0-9]+$ ]]; then
            echo "$tail"
            return
        fi
    fi
    if [[ "$raw" =~ ^([0-9]+)\(x[0-9]+\)$ ]]; then
        echo "${BASH_REMATCH[1]}"
        return
    fi
    echo "Unable to parse NPROC_PER_NODE from: $raw" >&2
    exit 1
}

NNODES="${NNODES:-${SLURM_NNODES:-1}}"
if [[ ! "$NNODES" =~ ^[0-9]+$ ]]; then
    echo "NNODES must be an integer, got: $NNODES" >&2
    exit 1
fi
NPROC_PER_NODE="$(infer_nproc_per_node)"

# Cap local ranks by default for better comm/compute balance on policy-gradient runs.
# Override with MAX_NPROC_PER_NODE if you want to use all visible GPUs.
MAX_NPROC_PER_NODE="${MAX_NPROC_PER_NODE:-4}"
if [[ ! "$MAX_NPROC_PER_NODE" =~ ^[0-9]+$ ]] || (( MAX_NPROC_PER_NODE < 1 )); then
    echo "MAX_NPROC_PER_NODE must be a positive integer, got: $MAX_NPROC_PER_NODE" >&2
    exit 1
fi
if (( NPROC_PER_NODE > MAX_NPROC_PER_NODE )); then
    echo "Capping NPROC_PER_NODE from $NPROC_PER_NODE to $MAX_NPROC_PER_NODE (MAX_NPROC_PER_NODE)"
    NPROC_PER_NODE="$MAX_NPROC_PER_NODE"
fi

# Balance worker count against available CPU to reduce CPU starvation and comm overhead.
# With torchrun, all local ranks share SLURM_CPUS_PER_TASK (single slurm task here).
TARGET_CPUS_PER_RANK="${TARGET_CPUS_PER_RANK:-2}"
if [[ -n "${SLURM_CPUS_PER_TASK:-}" ]] && [[ "$SLURM_CPUS_PER_TASK" =~ ^[0-9]+$ ]]; then
    if [[ ! "$TARGET_CPUS_PER_RANK" =~ ^[0-9]+$ ]] || (( TARGET_CPUS_PER_RANK < 1 )); then
        echo "TARGET_CPUS_PER_RANK must be a positive integer, got: $TARGET_CPUS_PER_RANK" >&2
        exit 1
    fi
    max_ranks_by_cpu=$((SLURM_CPUS_PER_TASK / TARGET_CPUS_PER_RANK))
    if (( max_ranks_by_cpu < 1 )); then
        max_ranks_by_cpu=1
    fi
    if (( NPROC_PER_NODE > max_ranks_by_cpu )); then
        echo "Capping NPROC_PER_NODE from $NPROC_PER_NODE to $max_ranks_by_cpu"
        echo "Reason: SLURM_CPUS_PER_TASK=$SLURM_CPUS_PER_TASK and TARGET_CPUS_PER_RANK=$TARGET_CPUS_PER_RANK"
        NPROC_PER_NODE="$max_ranks_by_cpu"
    fi
fi

# Set explicit CPU-thread defaults before torchrun to avoid implicit OMP=1 throttling.
if [[ -z "${OMP_NUM_THREADS:-}" ]]; then
    if [[ -n "${SLURM_CPUS_PER_TASK:-}" ]] && [[ "$SLURM_CPUS_PER_TASK" =~ ^[0-9]+$ ]]; then
        OMP_NUM_THREADS="$((SLURM_CPUS_PER_TASK / NPROC_PER_NODE))"
        if (( OMP_NUM_THREADS < 1 )); then
            OMP_NUM_THREADS=1
        fi
    else
        OMP_NUM_THREADS=1
    fi
fi
export OMP_NUM_THREADS
export MKL_NUM_THREADS="${MKL_NUM_THREADS:-$OMP_NUM_THREADS}"
export OPENBLAS_NUM_THREADS="${OPENBLAS_NUM_THREADS:-$OMP_NUM_THREADS}"
export NUMEXPR_NUM_THREADS="${NUMEXPR_NUM_THREADS:-$OMP_NUM_THREADS}"
export TOKENIZERS_PARALLELISM="${TOKENIZERS_PARALLELISM:-false}"

WORLD_SIZE="$((NNODES * NPROC_PER_NODE))"

if [[ -z "${MASTER_ADDR:-}" ]]; then
    if [[ -n "${SLURM_JOB_NODELIST:-}" ]] && command -v scontrol >/dev/null 2>&1; then
        MASTER_ADDR="$(scontrol show hostnames "$SLURM_JOB_NODELIST" | head -n 1)"
    else
        MASTER_ADDR="127.0.0.1"
    fi
fi
MASTER_PORT="${MASTER_PORT:-29500}"
RDZV_ID="${RDZV_ID:-policy-gradients-${SLURM_JOB_ID:-local}}"

if (( NPROC_PER_NODE == 1 )) && [[ -z "${CUDA_VISIBLE_DEVICES:-}" ]]; then
    export CUDA_VISIBLE_DEVICES="${GPU_ID:-0}"
fi

export MLRUNX_SERVER_URL="${MLRUNX_SERVER_URL:-${MLRUNX_API_URL:-http://127.0.0.1:3001}}"
export MLRUNX_API_URL="$MLRUNX_SERVER_URL"
export MLRUNX_PROJECT="${MLRUNX_PROJECT:-policy-gradients}"
export MLRUNX_SPOOL_ENABLED="${MLRUNX_SPOOL_ENABLED:-true}"
export MLRUNX_SPOOL_DIR="${MLRUNX_SPOOL_DIR:-$PROJECT_ROOT/.mlrunx/spool/slurm-${SLURM_JOB_ID:-local}}"
mkdir -p "$MLRUNX_SPOOL_DIR"

if [[ "$MLRUNX_SERVER_URL" =~ ^https?://(127\.0\.0\.1|localhost)(:[0-9]+)?(/.*)?$ ]]; then
    echo "WARNING: MLRUNX_SERVER_URL=$MLRUNX_SERVER_URL points to loopback."
    echo "On Slurm compute nodes this usually causes offline logging."
    echo "Set MLRUNX_SERVER_URL to a cluster-reachable MLRunX API endpoint."
    echo ""
fi

# Auto-switch to explicit offline mode when API endpoint is unreachable.
# This avoids repeated connection-refused retries in logs while preserving local spool files.
if [[ "${MLRUNX_AUTO_OFFLINE:-true}" == "true" ]] && [[ -z "${MLRUNX_OFFLINE:-}" ]]; then
    if ! "$PYTHON_BIN" - "$MLRUNX_SERVER_URL" <<'PY'
import sys
import urllib.request

base = sys.argv[1].rstrip("/")
for suffix in ("/health", "/"):
    url = base + suffix
    try:
        with urllib.request.urlopen(url, timeout=2.0) as resp:
            # Any HTTP response means endpoint is reachable.
            if resp.status >= 100:
                sys.exit(0)
    except Exception:
        pass
sys.exit(1)
PY
    then
        export MLRUNX_OFFLINE=true
        export MLRUNX_MAX_RETRIES="${MLRUNX_MAX_RETRIES:-0}"
        echo "MLRunX endpoint unreachable ($MLRUNX_SERVER_URL); enabling MLRUNX_OFFLINE=true"
    fi
fi

configs=(rloo.yaml ppo.yaml grpo.yaml drgrpo.yaml gspo.yaml cispo.yaml reinforce.yaml)
algos=("RLOO" "PPO" "GRPO" "Dr.GRPO" "GSPO" "CISPO" "REINFORCE")

echo "========================================================"
echo "  Policy Gradient Training via torchrun (sequential)"
echo "========================================================"
echo "MLRunX API: $MLRUNX_SERVER_URL"
echo "MLRunX Project: $MLRUNX_PROJECT"
echo "MLRunX Spool Dir: $MLRUNX_SPOOL_DIR"
echo "MLRUNX_OFFLINE: ${MLRUNX_OFFLINE:-false}"
echo "NNODES: $NNODES"
echo "NPROC_PER_NODE: $NPROC_PER_NODE"
echo "MAX_NPROC_PER_NODE: $MAX_NPROC_PER_NODE"
echo "WORLD_SIZE: $WORLD_SIZE"
echo "MASTER_ADDR: $MASTER_ADDR"
echo "MASTER_PORT: $MASTER_PORT"
if [[ -n "${SLURM_CPUS_PER_TASK:-}" ]]; then
    echo "SLURM_CPUS_PER_TASK: $SLURM_CPUS_PER_TASK"
fi
echo "TARGET_CPUS_PER_RANK: $TARGET_CPUS_PER_RANK"
echo "OMP_NUM_THREADS: $OMP_NUM_THREADS"
if [[ -n "${CUDA_VISIBLE_DEVICES:-}" ]]; then
    echo "CUDA_VISIBLE_DEVICES: $CUDA_VISIBLE_DEVICES"
fi
echo ""
echo "Algorithms: ${algos[*]}"
echo ""

run_with_torchrun() {
    local cfg="$1"
    local cfg_path="policy_gradients/configs/$cfg"
    local cfg_name="${cfg%.yaml}"
    local cfg_rdzv_id="${RDZV_ID}-${cfg_name}"

    if (( NNODES > 1 )); then
        if ! command -v srun >/dev/null 2>&1; then
            echo "srun is required for multi-node launches but is not available." >&2
            exit 1
        fi
        srun --nodes="$NNODES" --ntasks="$NNODES" --ntasks-per-node=1 \
            "$PYTHON_BIN" -m torch.distributed.run \
            --nnodes="$NNODES" \
            --nproc_per_node="$NPROC_PER_NODE" \
            --rdzv_backend=c10d \
            --rdzv_endpoint="${MASTER_ADDR}:${MASTER_PORT}" \
            --rdzv_id="$cfg_rdzv_id" \
            --max_restarts=0 \
            -m policy_gradients.train \
            --config "$cfg_path"
    else
        "$PYTHON_BIN" -m torch.distributed.run \
            --nnodes=1 \
            --nproc_per_node="$NPROC_PER_NODE" \
            --master_addr="$MASTER_ADDR" \
            --master_port="$MASTER_PORT" \
            --max_restarts=0 \
            -m policy_gradients.train \
            --config "$cfg_path"
    fi
}

for i in "${!configs[@]}"; do
    algo="${algos[$i]}"
    cfg="${configs[$i]}"
    echo "========================================================"
    echo "[$((i + 1))/${#configs[@]}] Starting $algo with torchrun"
    echo "========================================================"
    run_with_torchrun "$cfg"
done

echo ""
echo "========================================================"
echo "All training runs complete!"
echo "========================================================"
echo ""
echo "View results in MLRunX:"
echo "  - Project: $MLRUNX_PROJECT"
echo "  - API: $MLRUNX_API_URL"
echo "  - UI: http://localhost:3000 (start with: cd apps/ui && npm run dev)"
