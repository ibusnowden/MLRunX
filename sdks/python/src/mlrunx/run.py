"""Run class - represents an active experiment run.

The Run class is the primary interface for logging metrics, parameters,
and artifacts during an experiment.
"""

from __future__ import annotations

import json
import logging
import time
import uuid
from typing import Any

from mlrunx.config import Config, get_config
from mlrunx.queue import Event, EventQueue, EventType
from mlrunx.transport.base import Transport, TransportError
from mlrunx.transport.http import HttpTransport
from mlrunx.worker import FlushWorker

logger = logging.getLogger(__name__)


class Run:
    """Represents an active experiment run.

    The Run class provides a non-blocking interface for logging:
    - Metrics (time-series data like loss, accuracy)
    - Parameters (hyperparameters, configuration)
    - Tags (metadata for filtering/organizing)
    - Structured log events (trainer/runtime/system)
    - Artifacts (files like models, datasets)

    Example:
        run = Run(project_id="project-uuid", name="experiment-1")
        run.log({"loss": 0.5, "accuracy": 0.8}, step=1)
        run.log_params({"lr": 0.001, "batch_size": 32})
        run.finish()
    """

    def __init__(
        self,
        project: str | None = None,
        project_id: str | None = None,
        name: str | None = None,
        run_id: str | None = None,
        tags: dict[str, str] | None = None,
        config: dict[str, Any] | None = None,
        sdk_config: Config | None = None,
    ) -> None:
        """Initialize a new run.

        Args:
            project: Optional project name for validation/compatibility.
            project_id: Project ID (preferred for scoped API keys)
            name: Human-readable run name (auto-generated if not provided)
            run_id: Explicit run ID (auto-generated if not provided)
            tags: Initial tags for the run
            config: Initial configuration/parameters
            sdk_config: SDK configuration (uses global if not provided)
        """
        self._sdk_config = sdk_config or get_config()

        resolved_project_id = project_id
        project_name = project

        if resolved_project_id is None and project_name is None:
            resolved_project_id = self._sdk_config.project_id

        if resolved_project_id is None and project_name is not None:
            try:
                uuid.UUID(project_name)
                logger.warning(
                    "project argument looks like a UUID; treating it as project_id. "
                    "Pass project_id explicitly to avoid ambiguity."
                )
                resolved_project_id = project_name
                project_name = None
            except ValueError as exc:
                raise ValueError(
                    "project_id is required. Project names are no longer accepted "
                    "for run initialization. Use mlrunx.init(project_id=...) or "
                    "set MLRUNX_PROJECT_ID."
                ) from exc

        if resolved_project_id is None and project_name is None:
            raise ValueError(
                "project_id is required. Set MLRUNX_PROJECT_ID or pass "
                "mlrunx.init(project_id=...)."
            )

        self._project_name = project_name
        self._project_id = resolved_project_id
        self._name = name or self._generate_name()
        self._run_id = run_id or str(uuid.uuid4())
        self._tags = tags or {}
        self._params: dict[str, str] = {}
        self._finished = False
        self._offline = False
        self._step = 0
        self._start_time = time.time()

        # Initialize components
        self._queue = EventQueue(max_size=self._sdk_config.queue_size)
        self._transport = self._create_transport()
        self._worker = FlushWorker(
            queue=self._queue,
            transport=self._transport,
            config=self._sdk_config,
        )

        # Start the background worker
        self._worker.start()

        # Initialize run on server unless explicit offline mode is requested.
        if self._sdk_config.offline_mode:
            self._offline = True
            self._worker.set_offline()
            logger.warning("MLRUNX_OFFLINE enabled - running in offline mode")
            if config:
                self.log_params(config)
        else:
            try:
                self._init_run_on_server(config or {})
            except Exception:
                # Ensure partially initialized resources do not leak if init fails.
                self._worker.stop()
                self._transport.close()
                raise

        logger.info(f"Run started: {self._run_id} ({self._name})")

    def _generate_name(self) -> str:
        """Generate a human-readable run name."""
        import random
        import string

        suffix = "".join(random.choices(string.ascii_lowercase + string.digits, k=6))
        return f"run-{suffix}"

    def _create_transport(self) -> Transport:
        """Create the transport instance."""
        return HttpTransport(
            base_url=self._sdk_config.server_url,
            api_key=self._sdk_config.api_key,
        )

    def _init_run_on_server(self, config: dict[str, Any]) -> None:
        """Initialize the run on the server."""
        try:
            response = self._transport.init_run(
                self._build_init_payload(config)
            )

            if response.get("offline"):
                self._offline = True
                self._worker.set_offline()
                logger.warning(
                    "Running in offline mode - data will be synced when server is available"
                )

            # Server may assign a different run_id
            if "run_id" in response and response["run_id"] != self._run_id:
                self._run_id = response["run_id"]

        except TransportError as e:
            # Non-retryable init failures indicate invalid auth/config and should fail fast.
            if not e.retryable:
                guidance = (
                    " Verify MLRUNX_API_KEY and MLRUNX_PROJECT_ID. "
                    "If your environment reports 'unexpected keyword project_id', "
                    "upgrade the Python SDK."
                )
                raise RuntimeError(f"Failed to initialize run on server: {e}.{guidance}") from e

            logger.warning(f"Failed to initialize run on server: {e}")
            self._offline = True
            self._worker.set_offline()
        except Exception as e:
            logger.warning(f"Failed to initialize run on server: {e}")
            self._offline = True
            self._worker.set_offline()

        # Log initial config as params
        if config:
            self.log_params(config)

    @property
    def run_id(self) -> str:
        """The unique run identifier."""
        return self._run_id

    @property
    def name(self) -> str:
        """The run name."""
        return self._name

    @property
    def project(self) -> str:
        """The resolved project identifier."""
        return self._project_name or self._project_id or ""

    @property
    def project_id(self) -> str | None:
        """The project ID (if known)."""
        return self._project_id

    def _build_init_payload(self, config: dict[str, Any]) -> dict[str, Any]:
        payload: dict[str, Any] = {
            "name": self._name,
            "run_id": self._run_id,
            "tags": self._tags,
            "config": config,
        }
        if self._project_id is not None:
            payload["project_id"] = self._project_id
        if self._project_name is not None:
            payload["project"] = self._project_name
        return payload

    @property
    def is_offline(self) -> bool:
        """Whether the run is in offline mode."""
        return self._offline

    @property
    def is_finished(self) -> bool:
        """Whether the run has been finished."""
        return self._finished

    def log(
        self,
        data: dict[str, float | int],
        step: int | None = None,
        timestamp: float | None = None,
    ) -> None:
        """Log metrics (non-blocking).

        Args:
            data: Dictionary of metric name -> value
            step: Optional step number (auto-incremented if not provided)
            timestamp: Optional timestamp (uses current time if not provided)

        Example:
            run.log({"loss": 0.5, "accuracy": 0.8}, step=100)
        """
        if self._finished:
            logger.warning("Attempting to log to a finished run")
            return

        if step is None:
            step = self._step
            self._step += 1
        else:
            self._step = max(self._step, step + 1)

        ts = timestamp or time.time()

        # Queue each metric as a separate event for flexibility
        for name, value in data.items():
            event = Event(
                type=EventType.METRIC,
                run_id=self._run_id,
                timestamp=ts,
                data={
                    "name": name,
                    "value": float(value),
                    "step": step,
                    "timestamp": ts,
                },
            )
            if not self._queue.put(event):
                logger.warning(f"Queue full, metric dropped: {name}")

    def log_event(
        self,
        message: str,
        level: str = "info",
        step: int | None = None,
        source: str = "sdk",
        timestamp: float | None = None,
    ) -> None:
        """Log a structured run event (non-blocking).

        Args:
            message: Event message
            level: Severity level ("debug", "info", "warn", "error")
            step: Optional training step associated with the event
            source: Event source ("sdk", "trainer", etc.)
            timestamp: Optional Unix timestamp in seconds
        """
        if self._finished:
            logger.warning("Attempting to log to a finished run")
            return

        cleaned_message = message.strip()
        if not cleaned_message:
            return

        normalized_level = level.strip().lower()
        if normalized_level not in {"debug", "info", "warn", "error"}:
            normalized_level = "info"

        cleaned_source = source.strip() or "sdk"
        ts = timestamp or time.time()

        event = Event(
            type=EventType.EVENT,
            run_id=self._run_id,
            timestamp=ts,
            data={
                "level": normalized_level,
                "source": cleaned_source,
                "message": cleaned_message,
                "step": step,
                "timestamp": ts,
            },
        )
        if not self._queue.put(event):
            logger.warning("Queue full, run event dropped")

    @staticmethod
    def _serialize_structured_message(
        kind: str,
        payload: dict[str, Any],
        max_chars: int = 1800,
    ) -> str:
        envelope = {"kind": kind, "payload": payload}
        encoded = json.dumps(envelope, ensure_ascii=True, separators=(",", ":"), default=str)
        if len(encoded) <= max_chars:
            return encoded

        preview_len = max(0, max_chars - 220)
        preview = encoded[:preview_len]
        truncated = {
            "kind": kind,
            "truncated": True,
            "original_size": len(encoded),
            "preview": preview,
        }
        return json.dumps(truncated, ensure_ascii=True, separators=(",", ":"))

    def log_params(self, params: dict[str, Any]) -> None:
        """Log parameters/hyperparameters (non-blocking).

        Args:
            params: Dictionary of parameter name -> value

        Example:
            run.log_params({"lr": 0.001, "batch_size": 32, "optimizer": "adam"})
        """
        if self._finished:
            logger.warning("Attempting to log to a finished run")
            return

        for name, value in params.items():
            value_text = str(value)
            self._params[name] = value_text
            event = Event(
                type=EventType.PARAM,
                run_id=self._run_id,
                data={
                    "name": name,
                    "value": value_text,
                },
            )
            if not self._queue.put(event):
                logger.warning(f"Queue full, param dropped: {name}")

    def log_tags(self, tags: dict[str, str]) -> None:
        """Log or update tags (non-blocking).

        Args:
            tags: Dictionary of tag key -> value

        Example:
            run.log_tags({"model": "resnet50", "dataset": "imagenet"})
        """
        if self._finished:
            logger.warning("Attempting to log to a finished run")
            return

        self._tags.update(tags)

        for key, value in tags.items():
            event = Event(
                type=EventType.TAG,
                run_id=self._run_id,
                data={
                    "key": key,
                    "value": str(value),
                },
            )
            if not self._queue.put(event):
                logger.warning(f"Queue full, tag dropped: {key}")

    def log_artifact(
        self,
        path: str,
        name: str | None = None,
        artifact_type: str = "file",
        metadata: dict[str, Any] | None = None,
    ) -> None:
        """Log artifact metadata as a structured run event.

        Args:
            path: Local path to the artifact file
            name: Name for the artifact (defaults to filename)
            artifact_type: Type of artifact ("file", "model", "dataset", etc.)
            metadata: Optional artifact metadata payload
        """
        if self._finished:
            logger.warning("Attempting to log to a finished run")
            return

        artifact_name = name or path
        message = self._serialize_structured_message(
            "artifact",
            {
                "name": artifact_name,
                "path": path,
                "artifact_type": artifact_type,
                "metadata": metadata or {},
            },
        )
        self.log_event(message, level="info", source="artifact")

    def log_image(
        self,
        name: str,
        path: str,
        step: int | None = None,
        caption: str | None = None,
        metadata: dict[str, Any] | None = None,
    ) -> None:
        """Log image metadata as a structured run event."""
        if self._finished:
            logger.warning("Attempting to log to a finished run")
            return

        message = self._serialize_structured_message(
            "image",
            {
                "name": name,
                "path": path,
                "caption": caption,
                "metadata": metadata or {},
            },
        )
        self.log_event(message, level="info", source="image", step=step)

    def log_chart(
        self,
        name: str,
        data: dict[str, Any],
        chart_type: str = "custom",
        step: int | None = None,
        layout: dict[str, Any] | None = None,
        metadata: dict[str, Any] | None = None,
    ) -> None:
        """Log chart metadata/spec payload as a structured run event."""
        if self._finished:
            logger.warning("Attempting to log to a finished run")
            return

        message = self._serialize_structured_message(
            "chart",
            {
                "name": name,
                "chart_type": chart_type,
                "data": data,
                "layout": layout or {},
                "metadata": metadata or {},
            },
        )
        self.log_event(message, level="info", source="chart", step=step)

    def __setitem__(self, bucket: str, value: Any) -> None:
        """Dictionary-style logging shortcuts.

        Supported buckets:
        - ``run["parameters"] = {...}``
        - ``run["params"] = {...}``
        - ``run["tags"] = {...}``
        - ``run["metrics"] = {...}``
        """
        if not isinstance(value, dict):
            raise TypeError(f"run['{bucket}'] expects a dictionary value")

        key = bucket.strip().lower()
        if key in {"parameters", "params"}:
            self.log_params(value)
            return
        if key == "tags":
            normalized = {str(k): str(v) for k, v in value.items()}
            self.log_tags(normalized)
            return
        if key in {"metrics", "metric"}:
            numeric_values: dict[str, float | int] = {}
            for metric_name, metric_value in value.items():
                if isinstance(metric_value, bool) or not isinstance(metric_value, (int, float)):
                    raise TypeError(
                        f"run['{bucket}'] accepts numeric metric values only; "
                        f"got {type(metric_value).__name__} for '{metric_name}'"
                    )
                numeric_values[str(metric_name)] = metric_value
            self.log(numeric_values)
            return

        raise KeyError(
            f"Unsupported run bucket '{bucket}'. Use one of: "
            "parameters, params, tags, metrics."
        )

    def __getitem__(self, bucket: str) -> dict[str, str]:
        """Read local run metadata caches for dictionary-style usage."""
        key = bucket.strip().lower()
        if key in {"parameters", "params"}:
            return dict(self._params)
        if key == "tags":
            return dict(self._tags)
        raise KeyError(
            f"Unsupported run bucket '{bucket}'. Use one of: parameters, params, tags."
        )

    def finish(self, status: str = "finished") -> None:
        """Finish the run and flush all pending data.

        Args:
            status: Final run status ("finished", "failed", "killed")
        """
        if self._finished:
            return

        self._finished = True
        duration = time.time() - self._start_time

        logger.info(f"Finishing run: {self._run_id}")

        # Flush remaining events
        self._worker.flush()
        self._worker.stop(timeout=10.0)

        # Mark run as finished on server
        try:
            self._transport.finish_run(self._run_id, status)
        except Exception as e:
            logger.warning(f"Failed to finish run on server: {e}")

        # Cleanup
        self._transport.close()

        # Log summary
        dropped = self._queue.dropped_count
        if dropped > 0:
            logger.warning(f"Dropped {dropped} events due to full queue")

        logger.info(
            f"Run {self._run_id} finished in {duration:.2f}s "
            f"({self._worker.batch_count} batches sent)"
        )

    def __enter__(self) -> Run:
        """Context manager entry."""
        return self

    def __exit__(self, exc_type: Any, exc_val: Any, exc_tb: Any) -> None:
        """Context manager exit - auto-finish with appropriate status."""
        if exc_type is not None:
            self.finish(status="failed")
        else:
            self.finish(status="finished")
