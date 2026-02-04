"""
MLRunX integrations for popular ML frameworks.

Example usage:
    # PyTorch Lightning
    from mlrunx_integrations import MLRunXLogger
    trainer = Trainer(logger=MLRunXLogger())

    # HuggingFace Transformers
    from mlrunx_integrations import MLRunXCallback
    trainer.add_callback(MLRunXCallback())

    # Optuna
    from mlrunx_integrations import MLRunXOptunaCallback
    study.optimize(objective, callbacks=[MLRunXOptunaCallback()])
"""

__version__ = "0.1.0"
__all__ = [
    "MLRunXLogger",
    "MLRunXCallback",
    "MLRunXOptunaCallback",
    "MLRunXHydraCallback",
    "__version__",
]


class MLRunXLogger:
    """PyTorch Lightning logger integration."""

    def __init__(self, project: str = "default") -> None:
        self.project = project
        # TODO: Implement Lightning logger interface

    def log_metrics(self, metrics: dict, step: int | None = None) -> None:
        """Log metrics to MLRunX."""
        pass  # TODO: Implement

    def log_hyperparams(self, params: dict) -> None:
        """Log hyperparameters to MLRunX."""
        pass  # TODO: Implement


class MLRunXCallback:
    """HuggingFace Transformers callback integration."""

    def __init__(self, project: str = "default") -> None:
        self.project = project
        # TODO: Implement Transformers callback interface


class MLRunXOptunaCallback:
    """Optuna optimization callback integration."""

    def __init__(self, project: str = "default") -> None:
        self.project = project
        # TODO: Implement Optuna callback interface


class MLRunXHydraCallback:
    """Hydra config capture integration."""

    def __init__(self, project: str = "default") -> None:
        self.project = project
        # TODO: Implement Hydra callback interface
