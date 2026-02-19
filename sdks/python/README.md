# MLRunX Python SDK

Async, non-blocking ML experiment tracking SDK for Python.

## Installation

```bash
pip install mlrunx
```

Verify your installed SDK supports `project_id`:

```bash
python - <<'PY'
import inspect
import mlrunx

print("mlrunx:", getattr(mlrunx, "__version__", "unknown"))
print("init signature:", inspect.signature(mlrunx.init))
if "project_id" not in inspect.signature(mlrunx.init).parameters:
    raise SystemExit(
        "Outdated SDK build detected. Upgrade mlrunx before running training."
    )
PY
```

## Quick Start

```python
import mlrunx

# Initialize a run (works offline if server unavailable)
run = mlrunx.init(
    project_id="019c63ba-ce30-7610-8500-18c31bc665de",
    name="training-run-1",
    tags={"model": "resnet50", "dataset": "imagenet"},
)

# Log hyperparameters
run.log_params({
    "learning_rate": 0.001,
    "batch_size": 32,
    "optimizer": "adam",
})

# Training loop - logging is non-blocking!
status = "finished"
try:
    for step in range(1000):
        loss, accuracy = train_step()
        run.log({"loss": loss, "accuracy": accuracy}, step=step)
except KeyboardInterrupt:
    status = "killed"
    raise
except Exception:
    status = "failed"
    raise
finally:
    # Always flush pending events, even on Ctrl+C.
    run.finish(status=status)
```

### Using Context Manager

```python
import mlrunx

with mlrunx.init(project_id="019c63ba-ce30-7610-8500-18c31bc665de") as run:
    run.log_params({"lr": 0.001})

    for step in range(1000):
        run.log({"loss": loss}, step=step)

    # Automatically finished on exit
```

## Features

### Non-Blocking Logging

All `log()` calls are non-blocking. Events are queued in memory and flushed in the background, ensuring your training loop runs at full speed.

```python
# This won't slow down your training!
for step in range(100000):
    loss = train_step()  # Your expensive computation
    run.log({"loss": loss}, step=step)  # < 1μs overhead
```

### Adaptive Batching

Events are batched intelligently before being sent:

- **Size trigger**: Flush when batch reaches max items (default: 1000)
- **Bytes trigger**: Flush when batch reaches max bytes (default: 1MB)
- **Time trigger**: Flush after max age (default: 1 second)

Configure via environment variables:
```bash
export MLRUNX_BATCH_SIZE=500
export MLRUNX_BATCH_MAX_BYTES=500000
export MLRUNX_BATCH_TIMEOUT_MS=2000
```

### Metric Coalescing

When logging the same metric multiple times at the same step, only the latest value is sent (configurable):

```python
# Only the last value (0.3) is sent for step 0
run.log({"loss": 0.5}, step=0)
run.log({"loss": 0.4}, step=0)
run.log({"loss": 0.3}, step=0)
```

Disable with:
```bash
export MLRUNX_COALESCE_METRICS=false
```

### Offline Mode & Disk Spool

If the server is unavailable, events are automatically spooled to disk and synced when the connection is restored:

```python
# Works even if server is down!
run = mlrunx.init(project_id="019c63ba-ce30-7610-8500-18c31bc665de")
print(run.is_offline)  # True if server unavailable

# Events are saved to ~/.mlrunx/spool/
for step in range(1000):
    run.log({"loss": loss}, step=step)

# When server comes back online, data syncs automatically
```

If logs show `Run not found` for a different run ID than your current run, your local spool likely contains stale files from older/deleted runs. Current SDK mainline discards those stale spool batches automatically during replay. On older builds, clear local pending spool files once:

```bash
rm -f ~/.mlrunx/spool/*.spool ~/.mlrunx/spool/*.pending
```

Configure spool settings:
```bash
export MLRUNX_SPOOL_ENABLED=true
export MLRUNX_SPOOL_DIR=~/.mlrunx/spool
export MLRUNX_SPOOL_MAX_SIZE=100000000  # 100MB
```

### Compression

Large batches are automatically compressed with gzip:

```bash
export MLRUNX_COMPRESSION=true
export MLRUNX_COMPRESSION_LEVEL=6  # 1-9
export MLRUNX_COMPRESSION_MIN_BYTES=1000  # Only compress if > 1KB
```

## API Reference

### `mlrunx.init()`

Initialize a new run.

```python
run = mlrunx.init(
    project_id="019c63ba-ce30-7610-8500-18c31bc665de",  # Required: project ID
    name="experiment-1",        # Optional: run name (auto-generated if not provided)
    tags={"key": "value"},      # Optional: initial tags
    config={"lr": 0.001},       # Optional: initial config (logged as params)
)
```

### `run.log()`

Log metrics (non-blocking).

```python
run.log(
    {"loss": 0.5, "accuracy": 0.8},  # Metrics dict
    step=100,                         # Optional: step number
    timestamp=time.time(),            # Optional: custom timestamp
)
```

### `run.log_params()`

Log hyperparameters (non-blocking).

```python
run.log_params({
    "learning_rate": 0.001,
    "batch_size": 32,
    "model": "resnet50",
})
```

### `run.log_tags()`

Log or update tags (non-blocking).

```python
run.log_tags({
    "status": "running",
    "gpu": "A100",
})
```

### `run.finish()`

Finish the run and flush all pending data.

```python
run.finish(status="finished")  # or "failed", "killed"
```

`run.finish()` should be called from a `finally` block (or use `with mlrunx.init(...) as run`) so interrupted runs still flush metrics.

## Examples

### Simple Training Loop

```python
import mlrunx

run = mlrunx.init(project_id="019c63ba-ce30-7610-8500-18c31bc665de")
run.log_params({"lr": 0.001, "epochs": 10})

for epoch in range(10):
    for batch in dataloader:
        loss = train_step(batch)
        run.log({"train/loss": loss})

    val_loss = validate()
    run.log({"val/loss": val_loss}, step=epoch)

run.finish()
```

### PyTorch Integration

See [examples/pytorch_mnist.py](examples/pytorch_mnist.py) for a complete example.

```python
import mlrunx
import torch

with mlrunx.init(project_id="019c63ba-ce30-7610-8500-18c31bc665de", tags={"framework": "pytorch"}) as run:
    run.log_params({"lr": 0.01, "epochs": 10})

    for epoch in range(10):
        for batch_idx, (data, target) in enumerate(train_loader):
            loss = train_step(data, target)
            run.log({"train/loss": loss.item()}, step=epoch * len(train_loader) + batch_idx)

        val_loss, val_acc = validate()
        run.log({"val/loss": val_loss, "val/accuracy": val_acc}, step=epoch)
```

### HuggingFace Transformers

See [examples/huggingface_text_classification.py](examples/huggingface_text_classification.py) for a complete example.

```python
import mlrunx
from transformers import Trainer, TrainerCallback

class MLRunCallback(TrainerCallback):
    def __init__(self, run):
        self.run = run

    def on_log(self, args, state, control, logs=None, **kwargs):
        if logs:
            self.run.log(logs, step=state.global_step)

with mlrunx.init(project_id="019c63ba-ce30-7610-8500-18c31bc665de", tags={"framework": "transformers"}) as run:
    callback = MLRunCallback(run)
    trainer = Trainer(..., callbacks=[callback])
    trainer.train()
```

## Configuration

All settings can be configured via environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `MLRUNX_SERVER_URL` | `http://localhost:3001` | Server URL |
| `MLRUNX_API_KEY` | None | API key for authentication |
| `MLRUNX_PROJECT_ID` | None | Project ID used when `project_id` is not passed to `mlrunx.init()` |
| `MLRUNX_BATCH_SIZE` | `1000` | Max events per batch |
| `MLRUNX_BATCH_MAX_BYTES` | `1000000` | Max batch size in bytes |
| `MLRUNX_BATCH_TIMEOUT_MS` | `1000` | Max time before flush (ms) |
| `MLRUNX_COALESCE_METRICS` | `true` | Merge same metric at same step |
| `MLRUNX_DEDUPE_PARAMS` | `true` | Keep only last value for params |
| `MLRUNX_COMPRESSION` | `true` | Enable gzip compression |
| `MLRUNX_SPOOL_ENABLED` | `true` | Enable disk spooling |
| `MLRUNX_SPOOL_DIR` | `~/.mlrunx/spool` | Spool directory |
| `MLRUNX_OFFLINE` | `false` | Force offline mode |
| `MLRUNX_DEBUG` | `false` | Enable debug logging |

## Development

```bash
# Clone the repository
git clone https://github.com/your-org/mlrunx.git
cd mlrunx/sdks/python

# Create virtual environment
python -m venv .venv
source .venv/bin/activate

# Install in development mode
pip install -e ".[dev]"

# Run tests
pytest tests/ -v

# Run examples
python examples/simple_training.py
python examples/pytorch_mnist.py
```

## License

MIT License - see [LICENSE](../../LICENSE) for details.
