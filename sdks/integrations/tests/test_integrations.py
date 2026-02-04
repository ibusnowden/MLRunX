"""Unit tests for MLRunX integrations."""

import pytest

from mlrunx_integrations import (
    MLRunXCallback,
    MLRunXHydraCallback,
    MLRunXLogger,
    MLRunXOptunaCallback,
)


class TestMLRunXLogger:
    """Tests for PyTorch Lightning integration."""

    @pytest.mark.unit
    def test_logger_initialization(self) -> None:
        """Test MLRunXLogger can be initialized."""
        logger = MLRunXLogger(project="test-project")
        assert logger is not None

    @pytest.mark.unit
    def test_logger_has_required_methods(self) -> None:
        """Test MLRunXLogger has required Lightning logger methods."""
        logger = MLRunXLogger(project="test-project")
        assert hasattr(logger, "log_metrics")
        assert hasattr(logger, "log_hyperparams")


class TestMLRunXCallback:
    """Tests for HuggingFace Transformers integration."""

    @pytest.mark.unit
    def test_callback_initialization(self) -> None:
        """Test MLRunXCallback can be initialized."""
        callback = MLRunXCallback(project="test-project")
        assert callback is not None


class TestMLRunXOptunaCallback:
    """Tests for Optuna integration."""

    @pytest.mark.unit
    def test_optuna_callback_initialization(self) -> None:
        """Test MLRunXOptunaCallback can be initialized."""
        callback = MLRunXOptunaCallback(project="test-project")
        assert callback is not None


class TestMLRunXHydraCallback:
    """Tests for Hydra integration."""

    @pytest.mark.unit
    def test_hydra_callback_initialization(self) -> None:
        """Test MLRunXHydraCallback can be initialized."""
        callback = MLRunXHydraCallback(project="test-project")
        assert callback is not None
