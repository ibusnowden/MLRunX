"""Auto-debug helpers for training-time health checks.

This module provides an opt-in helper that logs tensor and gradient health
metrics to MLRunX at a configurable interval.
"""

from __future__ import annotations

import logging
from collections.abc import Mapping
from typing import Any

logger = logging.getLogger(__name__)

try:
    import torch  # type: ignore[import-not-found]
except ImportError:  # pragma: no cover - exercised in environments without torch
    torch = None


def _safe_name(name: str) -> str:
    """Return a metric-safe name segment."""
    return "".join(ch if ch.isalnum() or ch in "-_/" else "_" for ch in name)


class AutoDebugHelper:
    """Log tensor and gradient diagnostics during training.

    Args:
        run: Active MLRunX run instance.
        log_every_steps: Logging interval. Metrics are emitted when
            ``step % log_every_steps == 0``.
        prefix: Metric namespace prefix.
        log_shapes: Whether to emit tensor shape metrics.
        max_shape_dims: Maximum number of dimensions to emit per tensor.
        log_per_parameter_grad_norms: Whether to emit per-parameter gradient
            norm metrics (can be high-cardinality for large models).

    Example:
        helper = AutoDebugHelper(run, log_every_steps=50)
        ...
        loss.backward()
        helper.log_step(
            step=global_step,
            model=model,
            tensors={"loss": loss, "logits": logits},
        )
    """

    def __init__(
        self,
        run: Any,
        log_every_steps: int = 50,
        prefix: str = "debug",
        log_shapes: bool = True,
        max_shape_dims: int = 8,
        log_per_parameter_grad_norms: bool = False,
    ) -> None:
        if log_every_steps <= 0:
            raise ValueError("log_every_steps must be > 0")
        if max_shape_dims < 0:
            raise ValueError("max_shape_dims must be >= 0")

        self._run = run
        self._log_every_steps = log_every_steps
        self._prefix = prefix.strip("/") or "debug"
        self._log_shapes = log_shapes
        self._max_shape_dims = max_shape_dims
        self._log_per_parameter_grad_norms = log_per_parameter_grad_norms
        self._warned_missing_torch = False

    def should_log(self, step: int) -> bool:
        """Return True when diagnostics should be emitted for this step."""
        return step % self._log_every_steps == 0

    def log_step(
        self,
        step: int,
        model: Any | None = None,
        tensors: Mapping[str, Any] | None = None,
    ) -> dict[str, float | int]:
        """Collect and log diagnostics for a training step.

        Args:
            step: Training step.
            model: Optional model to inspect gradient/parameter health.
            tensors: Optional mapping of tensor names to inspect.

        Returns:
            Logged metric dictionary. Empty when skipped/no data.
        """
        if not self.should_log(step):
            return {}

        if torch is None:
            if (model is not None or tensors) and not self._warned_missing_torch:
                logger.warning(
                    "AutoDebugHelper requires torch; skipping tensor diagnostics"
                )
                self._warned_missing_torch = True
            return {}

        metrics: dict[str, float | int] = {}

        if tensors:
            metrics.update(self._collect_tensor_metrics(tensors))

        if model is not None:
            metrics.update(self._collect_gradient_metrics(model))
            metrics.update(self._collect_parameter_health(model))

        if metrics:
            self._run.log(metrics, step=step)

        return metrics

    def _collect_tensor_metrics(self, tensors: Mapping[str, Any]) -> dict[str, float | int]:
        """Collect NaN/Inf, finite ratio, and optional shape diagnostics."""
        assert torch is not None

        metrics: dict[str, float | int] = {}
        checked_tensors = 0
        non_finite_tensors = 0

        for name, value in tensors.items():
            if not torch.is_tensor(value):
                continue

            tensor = value.detach()
            metric_name = _safe_name(name)
            base = f"{self._prefix}/tensor/{metric_name}"

            checked_tensors += 1

            nan_count = int(torch.isnan(tensor).sum().item())
            inf_count = int(torch.isinf(tensor).sum().item())
            finite_ratio = float(torch.isfinite(tensor).float().mean().item())

            if nan_count > 0 or inf_count > 0:
                non_finite_tensors += 1

            metrics[f"{base}/nan_count"] = nan_count
            metrics[f"{base}/inf_count"] = inf_count
            metrics[f"{base}/finite_ratio"] = finite_ratio
            metrics[f"{base}/numel"] = int(tensor.numel())

            if tensor.numel() > 0:
                abs_tensor = tensor.abs().float()
                metrics[f"{base}/abs_mean"] = float(abs_tensor.mean().item())
                metrics[f"{base}/abs_max"] = float(abs_tensor.max().item())

            if self._log_shapes:
                dims = list(tensor.shape)
                metrics[f"{base}/shape_rank"] = len(dims)
                for idx, dim in enumerate(dims[: self._max_shape_dims]):
                    metrics[f"{base}/shape_dim_{idx}"] = int(dim)

        if checked_tensors > 0:
            metrics[f"{self._prefix}/tensor/checked_tensors"] = checked_tensors
            metrics[f"{self._prefix}/tensor/non_finite_tensors"] = non_finite_tensors

        return metrics

    def _collect_gradient_metrics(self, model: Any) -> dict[str, float | int]:
        """Collect gradient health and norm diagnostics."""
        assert torch is not None

        metrics: dict[str, float | int] = {}
        all_grads: list[Any] = []

        requires_grad_params = 0
        params_with_grad = 0
        params_missing_grad = 0
        zero_grad_tensors = 0
        nan_grad_tensors = 0
        inf_grad_tensors = 0

        for name, param in model.named_parameters():
            if not param.requires_grad:
                continue

            requires_grad_params += 1
            grad = param.grad
            if grad is None:
                params_missing_grad += 1
                continue

            params_with_grad += 1
            grad_detached = grad.detach().float()

            if grad_detached.numel() == 0:
                continue

            grad_norm = float(torch.linalg.vector_norm(grad_detached).item())

            has_nan = bool(torch.isnan(grad_detached).any().item())
            has_inf = bool(torch.isinf(grad_detached).any().item())

            if has_nan:
                nan_grad_tensors += 1
            if has_inf:
                inf_grad_tensors += 1
            if grad_norm == 0.0:
                zero_grad_tensors += 1

            if self._log_per_parameter_grad_norms:
                safe_name = _safe_name(name)
                metrics[f"{self._prefix}/grad_norm/{safe_name}"] = grad_norm

            all_grads.append(grad_detached.reshape(-1))

        metrics[f"{self._prefix}/grad/parameters_requires_grad"] = requires_grad_params
        metrics[f"{self._prefix}/grad/parameters_with_grad"] = params_with_grad
        metrics[f"{self._prefix}/grad/parameters_missing_grad"] = params_missing_grad
        metrics[f"{self._prefix}/grad/zero_grad_tensors"] = zero_grad_tensors
        metrics[f"{self._prefix}/grad/nan_tensors"] = nan_grad_tensors
        metrics[f"{self._prefix}/grad/inf_tensors"] = inf_grad_tensors

        if all_grads:
            global_grad = torch.cat(all_grads)
            metrics[f"{self._prefix}/grad/global_norm"] = float(
                torch.linalg.vector_norm(global_grad).item()
            )
            metrics[f"{self._prefix}/grad/global_abs_mean"] = float(
                global_grad.abs().mean().item()
            )
            metrics[f"{self._prefix}/grad/global_abs_max"] = float(
                global_grad.abs().max().item()
            )
            metrics[f"{self._prefix}/grad/global_finite_ratio"] = float(
                torch.isfinite(global_grad).float().mean().item()
            )

        return metrics

    def _collect_parameter_health(self, model: Any) -> dict[str, float | int]:
        """Collect NaN/Inf diagnostics for model parameters."""
        assert torch is not None

        metrics: dict[str, float | int] = {}

        nan_param_tensors = 0
        inf_param_tensors = 0
        checked_params = 0

        for param in model.parameters():
            checked_params += 1
            p = param.detach()

            has_nan = bool(torch.isnan(p).any().item())
            has_inf = bool(torch.isinf(p).any().item())

            if has_nan:
                nan_param_tensors += 1
            if has_inf:
                inf_param_tensors += 1

        metrics[f"{self._prefix}/param/checked_tensors"] = checked_params
        metrics[f"{self._prefix}/param/nan_tensors"] = nan_param_tensors
        metrics[f"{self._prefix}/param/inf_tensors"] = inf_param_tensors

        return metrics
