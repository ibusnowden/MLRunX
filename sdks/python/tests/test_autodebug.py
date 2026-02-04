"""Tests for AutoDebugHelper."""

from __future__ import annotations

import pytest

from mlrunx.autodebug import AutoDebugHelper

try:
    import torch
except ImportError:  # pragma: no cover - environment dependent
    torch = None


class DummyRun:
    """Simple run stub that records metric logs."""

    def __init__(self) -> None:
        self.logged: list[tuple[int, dict[str, float | int]]] = []

    def log(self, data: dict[str, float | int], step: int) -> None:
        self.logged.append((step, dict(data)))


@pytest.mark.unit
class TestAutoDebugHelper:
    """Tests for interval, tensor, and gradient diagnostics."""

    def test_interval_skip_and_emit(self) -> None:
        """Diagnostics are emitted only on configured interval."""
        if torch is None:
            pytest.skip("torch is not installed")

        run = DummyRun()
        helper = AutoDebugHelper(run=run, log_every_steps=2)

        skipped = helper.log_step(step=1, tensors={"loss": torch.tensor(1.0)})
        assert skipped == {}
        assert run.logged == []

        emitted = helper.log_step(step=2, tensors={"loss": torch.tensor(1.0)})
        assert emitted
        assert len(run.logged) == 1
        assert run.logged[0][0] == 2
        assert "debug/tensor/loss/finite_ratio" in emitted

    def test_tensor_non_finite_and_shape_metrics(self) -> None:
        """Tensor checks capture NaN/Inf counts and shape dimensions."""
        if torch is None:
            pytest.skip("torch is not installed")

        run = DummyRun()
        helper = AutoDebugHelper(run=run, log_every_steps=1)

        logits = torch.tensor([[1.0, float("nan")], [float("inf"), -2.0]])
        metrics = helper.log_step(step=0, tensors={"logits": logits})

        assert metrics["debug/tensor/logits/nan_count"] == 1
        assert metrics["debug/tensor/logits/inf_count"] == 1
        assert metrics["debug/tensor/logits/finite_ratio"] == pytest.approx(0.5)
        assert metrics["debug/tensor/logits/shape_rank"] == 2
        assert metrics["debug/tensor/logits/shape_dim_0"] == 2
        assert metrics["debug/tensor/logits/shape_dim_1"] == 2
        assert metrics["debug/tensor/checked_tensors"] == 1
        assert metrics["debug/tensor/non_finite_tensors"] == 1

    def test_gradient_and_parameter_health_metrics(self) -> None:
        """Model checks emit gradient and parameter health diagnostics."""
        if torch is None:
            pytest.skip("torch is not installed")

        run = DummyRun()
        helper = AutoDebugHelper(run=run, log_every_steps=1)

        model = torch.nn.Linear(3, 2)
        x = torch.ones((4, 3), dtype=torch.float32)
        loss = model(x).sum()
        loss.backward()

        metrics = helper.log_step(step=0, model=model)

        assert metrics["debug/grad/parameters_requires_grad"] > 0
        assert metrics["debug/grad/parameters_with_grad"] > 0
        assert metrics["debug/grad/global_norm"] > 0
        assert metrics["debug/param/checked_tensors"] > 0

    def test_per_parameter_grad_norms_optional(self) -> None:
        """Per-parameter grad norms are emitted only when enabled."""
        if torch is None:
            pytest.skip("torch is not installed")

        run = DummyRun()
        helper = AutoDebugHelper(
            run=run,
            log_every_steps=1,
            log_per_parameter_grad_norms=True,
        )

        model = torch.nn.Linear(3, 2)
        x = torch.ones((2, 3), dtype=torch.float32)
        loss = model(x).sum()
        loss.backward()

        metrics = helper.log_step(step=0, model=model)

        assert any(key.startswith("debug/grad_norm/") for key in metrics)

    def test_invalid_interval_raises(self) -> None:
        """Invalid interval values are rejected."""
        run = DummyRun()
        with pytest.raises(ValueError, match="log_every_steps"):
            AutoDebugHelper(run=run, log_every_steps=0)

    def test_missing_torch_is_noop(self, monkeypatch: pytest.MonkeyPatch) -> None:
        """Helper is a safe no-op when torch is unavailable."""
        run = DummyRun()
        helper = AutoDebugHelper(run=run, log_every_steps=1)

        monkeypatch.setattr("mlrunx.autodebug.torch", None)
        metrics = helper.log_step(step=0, tensors={"loss": object()})

        assert metrics == {}
        assert run.logged == []
