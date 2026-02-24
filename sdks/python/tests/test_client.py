"""Tests for MLRunX SDK client."""

from __future__ import annotations

import json
import time
from unittest.mock import MagicMock, patch

import pytest

import mlrunx
from mlrunx.config import Config
from mlrunx.queue import Event, EventQueue, EventType
from mlrunx.run import Run
from mlrunx.transport.base import TransportError

TEST_PROJECT_ID = "019c55f9-084a-7511-9393-6c17ede4a70f"


class TestConfig:
    """Tests for configuration."""

    @pytest.mark.unit
    def test_default_config(self) -> None:
        """Test default configuration values."""
        config = Config()
        assert config.server_url == "http://localhost:3001"
        assert config.batch_size == 1000
        assert config.batch_timeout_ms == 1000
        assert config.queue_size == 10000

    @pytest.mark.unit
    def test_config_from_env(self) -> None:
        """Test loading config from environment."""
        with patch.dict(
            "os.environ",
            {
                "MLRUNX_SERVER_URL": "http://custom:8080",
                "MLRUNX_BATCH_SIZE": "500",
            },
        ):
            config = Config.from_env()
            assert config.server_url == "http://custom:8080"
            assert config.batch_size == 500


class TestEventQueue:
    """Tests for the event queue."""

    @pytest.mark.unit
    def test_put_and_drain(self) -> None:
        """Test basic put and drain operations."""
        queue = EventQueue(max_size=100)

        event = Event(
            type=EventType.METRIC,
            run_id="test-run",
            data={"name": "loss", "value": 0.5},
        )

        assert queue.put(event)
        assert queue.size == 1

        events = queue.drain()
        assert len(events) == 1
        assert events[0].data["name"] == "loss"
        assert queue.is_empty()

    @pytest.mark.unit
    def test_queue_full_drops(self) -> None:
        """Test that events are dropped when queue is full."""
        queue = EventQueue(max_size=2)

        for i in range(5):
            event = Event(
                type=EventType.METRIC,
                run_id="test-run",
                data={"value": i},
            )
            queue.put(event)

        assert queue.size == 2
        assert queue.dropped_count == 3

    @pytest.mark.unit
    def test_get_batch_timeout(self) -> None:
        """Test batch retrieval with timeout."""
        queue = EventQueue()

        # Add some events
        for i in range(3):
            event = Event(
                type=EventType.METRIC,
                run_id="test-run",
                data={"value": i},
            )
            queue.put(event)

        # Get batch with short timeout
        events = queue.get_batch(max_items=10, timeout_ms=100)
        assert len(events) == 3


class TestRun:
    """Tests for the Run class."""

    @pytest.mark.unit
    @patch("mlrunx.run.HttpTransport")
    def test_run_init(self, mock_transport_cls: MagicMock) -> None:
        """Test run initialization."""
        mock_transport = MagicMock()
        mock_transport.init_run.return_value = {"run_id": "test-123"}
        mock_transport_cls.return_value = mock_transport

        run = Run(project_id=TEST_PROJECT_ID, name="test-run")

        assert run.project == TEST_PROJECT_ID
        assert run.name == "test-run"
        assert not run.is_finished

        run.finish()

    @pytest.mark.unit
    @patch("mlrunx.run.HttpTransport")
    def test_run_log_metrics(self, mock_transport_cls: MagicMock) -> None:
        """Test logging metrics."""
        mock_transport = MagicMock()
        mock_transport.init_run.return_value = {"run_id": "test-123"}
        mock_transport_cls.return_value = mock_transport

        run = Run(project_id=TEST_PROJECT_ID)

        # Log some metrics
        run.log({"loss": 0.5, "accuracy": 0.8}, step=0)
        run.log({"loss": 0.3, "accuracy": 0.9}, step=1)

        # Events should be queued
        assert run._queue.size > 0

        run.finish()

    @pytest.mark.unit
    @patch("mlrunx.run.HttpTransport")
    def test_run_context_manager(self, mock_transport_cls: MagicMock) -> None:
        """Test run as context manager."""
        mock_transport = MagicMock()
        mock_transport.init_run.return_value = {"run_id": "test-123"}
        mock_transport_cls.return_value = mock_transport

        with Run(project_id=TEST_PROJECT_ID) as run:
            run.log({"loss": 0.5})
            assert not run.is_finished

        assert run.is_finished

    @pytest.mark.unit
    @patch("mlrunx.run.HttpTransport")
    def test_run_offline_mode(self, mock_transport_cls: MagicMock) -> None:
        """Test offline mode when server is unavailable."""
        mock_transport = MagicMock()
        mock_transport.init_run.return_value = {"run_id": "offline-123", "offline": True}
        mock_transport_cls.return_value = mock_transport

        run = Run(project_id=TEST_PROJECT_ID)

        assert run.is_offline
        run.finish()

    @pytest.mark.unit
    @patch("mlrunx.run.HttpTransport")
    def test_run_init_non_retryable_error_raises(self, mock_transport_cls: MagicMock) -> None:
        """Non-retryable init failures should fail fast, not silently fall back offline."""
        mock_transport = MagicMock()
        mock_transport.init_run.side_effect = TransportError(
            "Client error: 400 - project_id is required",
            status_code=400,
            retryable=False,
        )
        mock_transport_cls.return_value = mock_transport

        with pytest.raises(RuntimeError, match="Failed to initialize run on server"):
            Run(project_id=TEST_PROJECT_ID)

        mock_transport.close.assert_called_once()

    @pytest.mark.unit
    @patch("mlrunx.run.HttpTransport")
    def test_run_init_retryable_error_falls_back_offline(
        self, mock_transport_cls: MagicMock
    ) -> None:
        """Retryable init failures should continue in offline mode."""
        mock_transport = MagicMock()
        mock_transport.init_run.side_effect = TransportError(
            "Request timed out",
            status_code=504,
            retryable=True,
        )
        mock_transport_cls.return_value = mock_transport

        run = Run(project_id=TEST_PROJECT_ID)
        assert run.is_offline
        run.finish()

    @pytest.mark.unit
    def test_run_init_rejects_project_name_without_id(self) -> None:
        """Project boundaries require explicit project_id values."""
        with pytest.raises(ValueError, match="project_id is required"):
            Run(project="test-project-name")

    @pytest.mark.unit
    @patch("mlrunx.run.HttpTransport")
    def test_run_init_accepts_uuid_in_project_for_compatibility(
        self, mock_transport_cls: MagicMock
    ) -> None:
        """Legacy project arg should map UUID values to project_id."""
        mock_transport = MagicMock()
        mock_transport.init_run.return_value = {"run_id": "test-123"}
        mock_transport_cls.return_value = mock_transport

        run = Run(project=TEST_PROJECT_ID)
        assert run.project_id == TEST_PROJECT_ID

        payload = mock_transport.init_run.call_args.args[0]
        assert payload["project_id"] == TEST_PROJECT_ID
        assert "project" not in payload

        run.finish()

    @pytest.mark.unit
    @patch("mlrunx.run.HttpTransport")
    def test_run_dict_style_logging(self, mock_transport_cls: MagicMock) -> None:
        """Dictionary-style buckets should route to existing logging APIs."""
        mock_transport = MagicMock()
        mock_transport.init_run.return_value = {"run_id": "test-123"}
        mock_transport_cls.return_value = mock_transport

        run = Run(project_id=TEST_PROJECT_ID)
        run["parameters"] = {"lr": 0.001, "epochs": 3}
        run["tags"] = {"framework": "scratch", "seed": 42}
        run["metrics"] = {"loss": 1.23}

        assert run["parameters"]["lr"] == "0.001"
        assert run["parameters"]["epochs"] == "3"
        assert run["tags"]["framework"] == "scratch"
        assert run["tags"]["seed"] == "42"
        assert run._queue.size > 0
        run.finish()

    @pytest.mark.unit
    @patch("mlrunx.run.HttpTransport")
    def test_run_dict_style_validation(self, mock_transport_cls: MagicMock) -> None:
        """Dictionary-style buckets should reject unsupported shapes."""
        mock_transport = MagicMock()
        mock_transport.init_run.return_value = {"run_id": "test-123"}
        mock_transport_cls.return_value = mock_transport

        run = Run(project_id=TEST_PROJECT_ID)

        with pytest.raises(KeyError):
            run["unknown"] = {"a": 1}
        with pytest.raises(TypeError):
            run["parameters"] = "not-a-dict"  # type: ignore[assignment]
        with pytest.raises(TypeError):
            run["metrics"] = {"loss": True}
        with pytest.raises(KeyError):
            _ = run["metrics"]

        run.finish()

    @pytest.mark.unit
    @patch("mlrunx.run.HttpTransport")
    def test_run_structured_media_and_artifact_events(
        self, mock_transport_cls: MagicMock
    ) -> None:
        """Image/chart/artifact helpers should emit structured event envelopes."""
        mock_transport = MagicMock()
        mock_transport.init_run.return_value = {"run_id": "test-123"}
        mock_transport_cls.return_value = mock_transport

        run = Run(project_id=TEST_PROJECT_ID)
        run.log_image(name="sample", path="plots/loss.png", step=9, caption="loss curve")
        run.log_chart(
            name="eval_curve",
            data={"x": [1, 2], "y": [0.5, 0.3]},
            chart_type="line",
            step=10,
        )
        run.log_artifact(path="models/model.bin", name="checkpoint")

        events = [event for event in run._queue.drain() if event.type == EventType.EVENT]
        assert len(events) == 3
        payloads = [json.loads(event.data["message"]) for event in events]
        assert payloads[0]["kind"] == "image"
        assert payloads[1]["kind"] == "chart"
        assert payloads[2]["kind"] == "artifact"
        run.finish()


class TestModuleAPI:
    """Tests for the module-level API."""

    @pytest.mark.unit
    @patch("mlrunx.run.HttpTransport")
    def test_init_and_log(self, mock_transport_cls: MagicMock) -> None:
        """Test module-level init and log."""
        mock_transport = MagicMock()
        mock_transport.init_run.return_value = {"run_id": "test-123"}
        mock_transport_cls.return_value = mock_transport

        run = mlrunx.init(project_id=TEST_PROJECT_ID)
        assert run is not None

        # Should work via module-level API
        mlrunx.log({"loss": 0.5})
        mlrunx.log_params({"lr": 0.001})
        mlrunx.log_image(name="sample", path="plot.png")
        mlrunx.log_chart(
            name="line",
            data={"x": [1, 2, 3], "y": [3, 2, 1]},
            chart_type="line",
        )
        mlrunx.log_artifact(path="model.bin", name="model")

        mlrunx.finish()

    @pytest.mark.unit
    def test_log_without_init_raises(self) -> None:
        """Test that logging without init raises an error."""
        # Reset any active run
        mlrunx._active_run = None

        with pytest.raises(RuntimeError, match="No active run"):
            mlrunx.log({"loss": 0.5})


class TestNonBlocking:
    """Tests for non-blocking behavior."""

    @pytest.mark.unit
    @patch("mlrunx.run.HttpTransport")
    def test_log_is_fast(self, mock_transport_cls: MagicMock) -> None:
        """Test that logging is non-blocking."""
        mock_transport = MagicMock()
        mock_transport.init_run.return_value = {"run_id": "test-123"}
        mock_transport_cls.return_value = mock_transport

        run = Run(project_id=TEST_PROJECT_ID)

        # Log many metrics and measure time
        start = time.perf_counter()
        for i in range(1000):
            run.log({"loss": 0.5, "accuracy": 0.8, "step": i}, step=i)
        elapsed = time.perf_counter() - start

        # Should complete quickly (queue operations only).
        # Allow headroom for slower CI runners.
        assert elapsed < 0.5, f"Logging took too long: {elapsed:.3f}s"

        run.finish()


class TestCompression:
    """Tests for compression configuration."""

    @pytest.mark.unit
    def test_compression_config_defaults(self) -> None:
        """Test default compression configuration."""
        config = Config()
        assert config.compression_enabled is True
        assert config.compression_level == 6
        assert config.compression_min_bytes == 1000

    @pytest.mark.unit
    def test_compression_config_from_env(self) -> None:
        """Test compression config from environment."""
        with patch.dict(
            "os.environ",
            {
                "MLRUNX_COMPRESSION": "false",
                "MLRUNX_COMPRESSION_LEVEL": "9",
                "MLRUNX_COMPRESSION_MIN_BYTES": "5000",
            },
        ):
            config = Config.from_env()
            assert config.compression_enabled is False
            assert config.compression_level == 9
            assert config.compression_min_bytes == 5000

    @pytest.mark.unit
    def test_coalescing_config_defaults(self) -> None:
        """Test default coalescing configuration."""
        config = Config()
        assert config.coalesce_metrics is True
        assert config.dedupe_params is True
        assert config.dedupe_tags is True

    @pytest.mark.unit
    def test_coalescing_config_from_env(self) -> None:
        """Test coalescing config from environment."""
        with patch.dict(
            "os.environ",
            {
                "MLRUNX_COALESCE_METRICS": "false",
                "MLRUNX_DEDUPE_PARAMS": "0",
                "MLRUNX_DEDUPE_TAGS": "no",
            },
        ):
            config = Config.from_env()
            assert config.coalesce_metrics is False
            assert config.dedupe_params is False
            assert config.dedupe_tags is False
