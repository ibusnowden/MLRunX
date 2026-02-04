"""Run class - represents an active experiment run.

The Run class is the primary interface for logging metrics, parameters,
and artifacts during an experiment.
"""

from __future__ import annotations

import logging
import time
import uuid
from typing import Any

from mlrunx.config import Config, get_config
from mlrunx.queue import Event, EventQueue, EventType
from mlrunx.transport.base import Transport
from mlrunx.transport.http import HttpTransport
from mlrunx.worker import FlushWorker

logger = logging.getLogger(__name__)


class Run:
    """Represents an active experiment run.

    The Run class provides a non-blocking interface for logging:
    - Metrics (time-series data like loss, accuracy)
    - Parameters (hyperparameters, configuration)
    - Tags (metadata for filtering/organizing)
    - Artifacts (files like models, datasets)

    Example:
        run = Run(project="my-project", name="experiment-1")
        run.log({"loss": 0.5, "accuracy": 0.8}, step=1)
        run.log_params({"lr": 0.001, "batch_size": 32})
        run.finish()
    """

    def __init__(
        self,
        project: str,
        name: str | None = None,
        run_id: str | None = None,
        tags: dict[str, str] | None = None,
        config: dict[str, Any] | None = None,
        sdk_config: Config | None = None,
    ) -> None:
        """Initialize a new run.

        Args:
            project: Project name
            name: Human-readable run name (auto-generated if not provided)
            run_id: Explicit run ID (auto-generated if not provided)
            tags: Initial tags for the run
            config: Initial configuration/parameters
            sdk_config: SDK configuration (uses global if not provided)
        """
        self._project = project
        self._name = name or self._generate_name()
        self._run_id = run_id or str(uuid.uuid4())
        self._tags = tags or {}
        self._sdk_config = sdk_config or get_config()
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

        # Initialize run on server
        self._init_run_on_server(config or {})

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
                {
                    "project": self._project,
                    "name": self._name,
                    "run_id": self._run_id,
                    "tags": self._tags,
                    "config": config,
                }
            )

            if response.get("offline"):
                self._offline = True
                logger.warning(
                    "Running in offline mode - data will be synced when server is available"
                )

            # Server may assign a different run_id
            if "run_id" in response and response["run_id"] != self._run_id:
                self._run_id = response["run_id"]

        except Exception as e:
            logger.warning(f"Failed to initialize run on server: {e}")
            self._offline = True

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
        """The project name."""
        return self._project

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
            event = Event(
                type=EventType.PARAM,
                run_id=self._run_id,
                data={
                    "name": name,
                    "value": str(value),
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
    ) -> None:
        """Log an artifact (placeholder for future implementation).

        Args:
            path: Local path to the artifact file
            name: Name for the artifact (defaults to filename)
            artifact_type: Type of artifact ("file", "model", "dataset", etc.)
        """
        if self._finished:
            logger.warning("Attempting to log to a finished run")
            return

        # TODO: Implement artifact upload with presigned URLs
        logger.info(f"Artifact logging not yet implemented: {path}")

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
