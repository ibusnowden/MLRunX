"""
MLRunX Python SDK - async, non-blocking ML experiment tracking.

Example usage:
    import mlrunx

    # Initialize a run
    run = mlrunx.init(
        project_id="project-uuid",
        name="training-run-1",
        tags={"model": "resnet50"},
        config={"lr": 0.001, "batch_size": 32}
    )

    # Log metrics in your training loop (non-blocking)
    for step in range(1000):
        loss = train_step()
        run.log({"loss": loss, "accuracy": acc}, step=step)

    # Log parameters
    run.log_params({"optimizer": "adam", "epochs": 100})

    # Finish the run (flushes all pending data)
    run.finish()

    # Or use as context manager:
    with mlrunx.init(project_id="project-uuid") as run:
        for step in range(1000):
            run.log({"loss": loss}, step=step)
        # Auto-finishes on exit
"""

__version__ = "0.1.1"

from mlrunx.autodebug import AutoDebugHelper
from mlrunx.config import Config, configure, get_config
from mlrunx.run import Run

__all__ = [
    "__version__",
    "init",
    "Run",
    "AutoDebugHelper",
    "Config",
    "configure",
    "get_config",
    "log_event",
]

# Global active run (for convenience API)
_active_run: Run | None = None


def init(
    project: str | None = None,
    project_id: str | None = None,
    name: str | None = None,
    run_id: str | None = None,
    tags: dict[str, str] | None = None,
    config: dict | None = None,
) -> Run:
    """Initialize a new run.

    This is the main entry point for the MLRunX SDK. It creates a new Run
    instance that can be used to log metrics, parameters, and artifacts.

    Args:
        project: Deprecated project value for compatibility. Use project_id.
        project_id: Project ID (preferred for scoped API keys)
        name: Human-readable run name (auto-generated if not provided)
        run_id: Explicit run ID (auto-generated UUID if not provided)
        tags: Initial tags for filtering/organizing runs
        config: Initial configuration/hyperparameters

    Returns:
        A Run instance for logging

    Notes:
        If project_id is not provided, the SDK will use MLRUNX_PROJECT_ID if set.

    Example:
        # Basic usage
        run = mlrunx.init(project_id="project-uuid")

        # With all options
        run = mlrunx.init(
            project_id="project-uuid",
            name="experiment-v2",
            tags={"model": "resnet50", "dataset": "imagenet"},
            config={"lr": 0.001, "batch_size": 32}
        )

        # As context manager
        with mlrunx.init(project_id="project-uuid") as run:
            run.log({"loss": 0.5})
    """
    global _active_run

    # Finish any existing active run
    if _active_run is not None and not _active_run.is_finished:
        _active_run.finish()

    _active_run = Run(
        project=project,
        project_id=project_id,
        name=name,
        run_id=run_id,
        tags=tags,
        config=config,
    )

    return _active_run


def log(data: dict[str, float | int], step: int | None = None) -> None:
    """Log metrics to the active run (convenience function).

    Args:
        data: Dictionary of metric name -> value
        step: Optional step number

    Raises:
        RuntimeError: If no active run exists
    """
    if _active_run is None:
        raise RuntimeError("No active run. Call mlrunx.init() first.")
    _active_run.log(data, step=step)


def log_params(params: dict) -> None:
    """Log parameters to the active run (convenience function).

    Args:
        params: Dictionary of parameter name -> value

    Raises:
        RuntimeError: If no active run exists
    """
    if _active_run is None:
        raise RuntimeError("No active run. Call mlrunx.init() first.")
    _active_run.log_params(params)


def log_event(
    message: str,
    level: str = "info",
    step: int | None = None,
    source: str = "sdk",
) -> None:
    """Log a structured event to the active run (convenience function)."""
    if _active_run is None:
        raise RuntimeError("No active run. Call mlrunx.init() first.")
    _active_run.log_event(message, level=level, step=step, source=source)


def finish() -> None:
    """Finish the active run (convenience function).

    Raises:
        RuntimeError: If no active run exists
    """
    global _active_run
    if _active_run is None:
        raise RuntimeError("No active run. Call mlrunx.init() first.")
    _active_run.finish()
    _active_run = None


def get_run() -> Run | None:
    """Get the currently active run.

    Returns:
        The active Run instance, or None if no run is active
    """
    return _active_run
