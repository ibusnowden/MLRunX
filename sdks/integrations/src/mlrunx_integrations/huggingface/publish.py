"""Publish MLRunX artifact sets to the Hugging Face Hub."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import sys
import tempfile
from collections.abc import Sequence
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

README_NAME = "README.md"
METADATA_NAME = "mlrunx-metadata.json"


class PublishError(RuntimeError):
    """Raised when publish inputs are invalid or the Hub request fails."""


@dataclass(frozen=True)
class PublishResult:
    """Summary of a completed Hugging Face publish."""

    repo_id: str
    repo_url: str
    revision: str
    uploaded_files: tuple[str, ...]


@dataclass(frozen=True)
class _ManifestEntry:
    repo_path: str
    kind: str
    source_path: str | None


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="mlrunx-hf-publish",
        description="Publish selected MLRunX artifacts to a Hugging Face model repo.",
    )
    parser.add_argument(
        "--repo-id",
        required=True,
        help="Target Hugging Face repo, e.g. owner/model",
    )
    parser.add_argument("--weights", help="Local model weights file or directory")
    parser.add_argument("--config", help="Local config file or directory")
    parser.add_argument("--model-card", help="Local model card file to upload as README.md")
    parser.add_argument("--run-id", help="Optional MLRunX run identifier to include in metadata")
    parser.add_argument(
        "--project-id",
        help="Optional MLRunX project identifier to include in metadata",
    )
    parser.add_argument("--run-name", help="Optional MLRunX run name to include in metadata")
    parser.add_argument("--revision", default="main", help="Target repo revision (default: main)")
    parser.add_argument("--commit-message", help="Commit message for the publish operation")
    parser.add_argument(
        "--private",
        action="store_true",
        help="Create the repo as private if it does not already exist",
    )
    return parser


def _resolve_hf_token() -> str | None:
    token = os.getenv("HF_TOKEN", "").strip()
    if token:
        return token
    return None


def _build_hf_api(token: str | None) -> Any:
    try:
        from huggingface_hub import HfApi  # type: ignore[import-not-found]
    except ImportError as exc:
        raise PublishError(
            "huggingface_hub is required for publishing. Install "
            "'mlrunx-integrations[huggingface]' first."
        ) from exc

    return HfApi(token=token)


def _coerce_path(value: os.PathLike[str] | str | None, *, label: str) -> Path | None:
    if value is None:
        return None

    path = Path(value).expanduser().resolve()
    if not path.exists():
        raise PublishError(f"{label} path does not exist: {path}")
    if not path.is_file() and not path.is_dir():
        raise PublishError(f"{label} path must be a file or directory: {path}")
    return path


def _iter_source_files(source: Path, *, label: str) -> list[tuple[Path, str]]:
    if source.is_file():
        return [(source, source.name)]

    files = sorted(path for path in source.rglob("*") if path.is_file())
    if not files:
        raise PublishError(f"{label} directory contains no files: {source}")

    return [(path, path.relative_to(source).as_posix()) for path in files]


def _assert_repo_path_available(
    repo_path: str,
    *,
    source_path: Path,
    occupied_paths: dict[str, str],
    model_card_requested: bool,
) -> None:
    if repo_path == METADATA_NAME:
        raise PublishError(
            f"Reserved repo path '{METADATA_NAME}' cannot be supplied "
            f"by input artifacts: {source_path}"
        )
    if repo_path == README_NAME and model_card_requested:
        raise PublishError(
            f"Input artifact collides with README.md reserved for --model-card: {source_path}"
        )
    if repo_path in occupied_paths:
        existing = occupied_paths[repo_path]
        raise PublishError(
            f"Multiple inputs map to the same repo path '{repo_path}': {existing} and {source_path}"
        )


def _stage_input(
    *,
    stage_root: Path,
    source: Path,
    label: str,
    occupied_paths: dict[str, str],
    manifest: list[_ManifestEntry],
    model_card_requested: bool,
) -> None:
    for file_path, repo_path in _iter_source_files(source, label=label):
        _assert_repo_path_available(
            repo_path,
            source_path=file_path,
            occupied_paths=occupied_paths,
            model_card_requested=model_card_requested,
        )
        destination = stage_root / repo_path
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(file_path, destination)
        occupied_paths[repo_path] = str(file_path)
        manifest.append(
            _ManifestEntry(repo_path=repo_path, kind=label, source_path=str(file_path))
        )


def _render_generated_model_card(
    *,
    repo_id: str,
    run_id: str | None,
    project_id: str | None,
    run_name: str | None,
) -> str:
    lines = [
        f"# {repo_id.split('/')[-1]}",
        "",
        "Published from MLRunX.",
        "",
    ]
    if run_name or run_id or project_id:
        lines.extend(["## Source Run", ""])
        if run_name:
            lines.append(f"- Run name: `{run_name}`")
        if run_id:
            lines.append(f"- Run ID: `{run_id}`")
        if project_id:
            lines.append(f"- Project ID: `{project_id}`")
        lines.append("")
    lines.extend(
        [
            "## Notes",
            "",
            "- This README was generated automatically because no `--model-card` was supplied.",
            f"- Repo ID: `{repo_id}`",
            "",
        ]
    )
    return "\n".join(lines)


def _write_generated_model_card(
    *,
    stage_root: Path,
    manifest: list[_ManifestEntry],
    repo_id: str,
    run_id: str | None,
    project_id: str | None,
    run_name: str | None,
) -> None:
    readme_path = stage_root / README_NAME
    readme_path.write_text(
        _render_generated_model_card(
            repo_id=repo_id,
            run_id=run_id,
            project_id=project_id,
            run_name=run_name,
        ),
        encoding="utf-8",
    )
    manifest.append(
        _ManifestEntry(
            repo_path=README_NAME,
            kind="generated_model_card",
            source_path=None,
        )
    )


def _write_model_card(
    *,
    stage_root: Path,
    source: Path,
    occupied_paths: dict[str, str],
    manifest: list[_ManifestEntry],
) -> None:
    if README_NAME in occupied_paths:
        existing = occupied_paths[README_NAME]
        raise PublishError(
            "Cannot map --model-card to README.md because that path "
            f"is already occupied by {existing}"
        )

    destination = stage_root / README_NAME
    shutil.copy2(source, destination)
    occupied_paths[README_NAME] = str(source)
    manifest.append(
        _ManifestEntry(
            repo_path=README_NAME,
            kind="model_card",
            source_path=str(source),
        )
    )


def _default_commit_message(run_id: str | None) -> str:
    if run_id:
        return f"Publish MLRunX artifacts for run {run_id}"
    return "Publish MLRunX artifacts"


def _write_metadata(
    *,
    stage_root: Path,
    repo_id: str,
    revision: str,
    run_id: str | None,
    project_id: str | None,
    run_name: str | None,
    manifest: list[_ManifestEntry],
) -> None:
    metadata_path = stage_root / METADATA_NAME
    payload = {
        "repo_id": repo_id,
        "repo_type": "model",
        "revision": revision,
        "run_id": run_id,
        "project_id": project_id,
        "run_name": run_name,
        "published_at": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "artifacts": [
            {
                "repo_path": entry.repo_path,
                "kind": entry.kind,
                "source_path": entry.source_path,
            }
            for entry in manifest
        ],
    }
    metadata_path.write_text(
        json.dumps(payload, indent=2, sort_keys=True),
        encoding="utf-8",
    )
    manifest.append(
        _ManifestEntry(repo_path=METADATA_NAME, kind="metadata", source_path=None)
    )


def publish_to_huggingface(
    repo_id: str,
    *,
    weights: os.PathLike[str] | str | None = None,
    config: os.PathLike[str] | str | None = None,
    model_card: os.PathLike[str] | str | None = None,
    run_id: str | None = None,
    project_id: str | None = None,
    run_name: str | None = None,
    private: bool = False,
    revision: str = "main",
    commit_message: str | None = None,
) -> PublishResult:
    """Publish selected local artifacts to a Hugging Face model repo."""

    if not repo_id.strip():
        raise PublishError("repo_id is required")

    weights_path = _coerce_path(weights, label="weights")
    config_path = _coerce_path(config, label="config")
    model_card_path = _coerce_path(model_card, label="model-card")

    if weights_path is None and config_path is None and model_card_path is None:
        raise PublishError(
            "At least one of weights, config, or model_card must be provided"
        )

    if model_card_path is not None and not model_card_path.is_file():
        raise PublishError(f"model-card path must be a file: {model_card_path}")

    manifest: list[_ManifestEntry] = []
    occupied_paths: dict[str, str] = {}

    with tempfile.TemporaryDirectory(prefix="mlrunx-hf-publish-") as temp_dir:
        stage_root = Path(temp_dir)

        if weights_path is not None:
            _stage_input(
                stage_root=stage_root,
                source=weights_path,
                label="weights",
                occupied_paths=occupied_paths,
                manifest=manifest,
                model_card_requested=model_card_path is not None,
            )

        if config_path is not None:
            _stage_input(
                stage_root=stage_root,
                source=config_path,
                label="config",
                occupied_paths=occupied_paths,
                manifest=manifest,
                model_card_requested=model_card_path is not None,
            )

        if model_card_path is not None:
            _write_model_card(
                stage_root=stage_root,
                source=model_card_path,
                occupied_paths=occupied_paths,
                manifest=manifest,
            )
        elif README_NAME not in occupied_paths:
            _write_generated_model_card(
                stage_root=stage_root,
                manifest=manifest,
                repo_id=repo_id,
                run_id=run_id,
                project_id=project_id,
                run_name=run_name,
            )

        _write_metadata(
            stage_root=stage_root,
            repo_id=repo_id,
            revision=revision,
            run_id=run_id,
            project_id=project_id,
            run_name=run_name,
            manifest=manifest,
        )

        token = _resolve_hf_token()
        api = _build_hf_api(token)
        commit_message_value = commit_message or _default_commit_message(run_id)

        try:
            api.create_repo(
                repo_id=repo_id,
                repo_type="model",
                private=private,
                exist_ok=True,
            )
            api.upload_folder(
                folder_path=str(stage_root),
                repo_id=repo_id,
                repo_type="model",
                revision=revision,
                commit_message=commit_message_value,
            )
        except Exception as exc:
            raise PublishError(
                f"Failed to publish to Hugging Face repo '{repo_id}': {exc}"
            ) from exc

    return PublishResult(
        repo_id=repo_id,
        repo_url=f"https://huggingface.co/{repo_id}",
        revision=revision,
        uploaded_files=tuple(sorted(entry.repo_path for entry in manifest)),
    )


def main(argv: Sequence[str] | None = None) -> int:
    """CLI entrypoint for publishing artifacts to the Hugging Face Hub."""

    parser = _build_parser()
    args = parser.parse_args(list(argv) if argv is not None else None)

    if not any([args.weights, args.config, args.model_card]):
        parser.print_usage(sys.stderr)
        print(
            "mlrunx-hf-publish: error: at least one of --weights, --config, "
            "or --model-card is required",
            file=sys.stderr,
        )
        return 2

    try:
        result = publish_to_huggingface(
            args.repo_id,
            weights=args.weights,
            config=args.config,
            model_card=args.model_card,
            run_id=args.run_id,
            project_id=args.project_id,
            run_name=args.run_name,
            private=args.private,
            revision=args.revision,
            commit_message=args.commit_message,
        )
    except PublishError as exc:
        print(f"Error: {exc}", file=sys.stderr)
        return 1

    print(
        f"Published {len(result.uploaded_files)} files "
        f"to {result.repo_url} ({result.revision})"
    )
    for repo_path in result.uploaded_files:
        print(f"- {repo_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
