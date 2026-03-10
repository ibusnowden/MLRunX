"""Tests for the Hugging Face publish hook."""

from __future__ import annotations

import json
from pathlib import Path

from mlrunx_integrations.huggingface import publish as publish_module


class FakeHfApi:
    """Simple fake Hugging Face API client."""

    def __init__(self) -> None:
        self.create_calls: list[dict[str, object]] = []
        self.upload_calls: list[dict[str, object]] = []

    def create_repo(
        self,
        *,
        repo_id: str,
        repo_type: str,
        private: bool,
        exist_ok: bool,
    ) -> str:
        self.create_calls.append(
            {
                "repo_id": repo_id,
                "repo_type": repo_type,
                "private": private,
                "exist_ok": exist_ok,
            }
        )
        return f"https://huggingface.co/{repo_id}"

    def upload_folder(
        self,
        *,
        folder_path: str,
        repo_id: str,
        repo_type: str,
        revision: str,
        commit_message: str,
    ) -> str:
        snapshot: dict[str, bytes] = {}
        root = Path(folder_path)
        for path in sorted(root.rglob("*")):
            if path.is_file():
                snapshot[path.relative_to(root).as_posix()] = path.read_bytes()
        self.upload_calls.append(
            {
                "repo_id": repo_id,
                "repo_type": repo_type,
                "revision": revision,
                "commit_message": commit_message,
                "snapshot": snapshot,
            }
        )
        return "commit"


def test_resolve_hf_token_prefers_env(monkeypatch) -> None:
    monkeypatch.setenv("HF_TOKEN", "  secret-token  ")

    assert publish_module._resolve_hf_token() == "secret-token"


def test_publish_to_huggingface_uploads_selected_files(tmp_path, monkeypatch) -> None:
    weights_dir = tmp_path / "weights"
    weights_dir.mkdir()
    (weights_dir / "pytorch_model.bin").write_bytes(b"weights")
    config_path = tmp_path / "config.json"
    config_path.write_text('{"hidden_size": 768}', encoding="utf-8")

    fake_api = FakeHfApi()
    captured_tokens: list[str | None] = []

    def fake_build_hf_api(token: str | None) -> FakeHfApi:
        captured_tokens.append(token)
        return fake_api

    monkeypatch.setenv("HF_TOKEN", "local-token")
    monkeypatch.setattr(publish_module, "_build_hf_api", fake_build_hf_api)

    result = publish_module.publish_to_huggingface(
        "mlrunx/example-model",
        weights=weights_dir,
        config=config_path,
        run_id="run-123",
        project_id="project-456",
        run_name="demo-run",
    )

    assert captured_tokens == ["local-token"]
    assert fake_api.create_calls == [
        {
            "repo_id": "mlrunx/example-model",
            "repo_type": "model",
            "private": False,
            "exist_ok": True,
        }
    ]
    assert len(fake_api.upload_calls) == 1
    upload_call = fake_api.upload_calls[0]
    snapshot = upload_call["snapshot"]
    assert set(snapshot) == {
        "README.md",
        "config.json",
        "mlrunx-metadata.json",
        "pytorch_model.bin",
    }
    assert snapshot["pytorch_model.bin"] == b"weights"
    assert b"Published from MLRunX." in snapshot["README.md"]
    metadata = json.loads(snapshot["mlrunx-metadata.json"].decode("utf-8"))
    assert metadata["run_id"] == "run-123"
    assert metadata["project_id"] == "project-456"
    assert metadata["run_name"] == "demo-run"
    assert any(
        artifact["repo_path"] == "pytorch_model.bin" and artifact["kind"] == "weights"
        for artifact in metadata["artifacts"]
    )
    assert result.repo_url == "https://huggingface.co/mlrunx/example-model"
    assert result.uploaded_files == (
        "README.md",
        "config.json",
        "mlrunx-metadata.json",
        "pytorch_model.bin",
    )


def test_publish_to_huggingface_maps_model_card_to_readme(tmp_path, monkeypatch) -> None:
    weights_path = tmp_path / "model.safetensors"
    weights_path.write_bytes(b"tensor-data")
    model_card = tmp_path / "card.md"
    model_card.write_text("# Custom card\n", encoding="utf-8")

    fake_api = FakeHfApi()
    monkeypatch.setattr(publish_module, "_build_hf_api", lambda token: fake_api)

    publish_module.publish_to_huggingface(
        "mlrunx/custom-card",
        weights=weights_path,
        model_card=model_card,
        commit_message="custom message",
    )

    upload_call = fake_api.upload_calls[0]
    snapshot = upload_call["snapshot"]
    assert snapshot["README.md"] == b"# Custom card\n"
    assert upload_call["commit_message"] == "custom message"


def test_publish_to_huggingface_detects_repo_path_collision(tmp_path) -> None:
    weights_dir = tmp_path / "weights"
    weights_dir.mkdir()
    (weights_dir / "config.json").write_text("{}", encoding="utf-8")
    config_path = tmp_path / "config.json"
    config_path.write_text("{}", encoding="utf-8")

    try:
        publish_module.publish_to_huggingface(
            "mlrunx/collision",
            weights=weights_dir,
            config=config_path,
        )
    except publish_module.PublishError as exc:
        assert "same repo path 'config.json'" in str(exc)
    else:
        raise AssertionError("expected PublishError for colliding repo paths")


def test_main_requires_at_least_one_artifact(capsys) -> None:
    exit_code = publish_module.main(["--repo-id", "mlrunx/example-model"])

    captured = capsys.readouterr()
    assert exit_code == 2
    assert "at least one of --weights, --config, or --model-card is required" in captured.err


def test_main_invokes_publish_helper(monkeypatch, capsys) -> None:
    captured_args: dict[str, object] = {}

    def fake_publish(repo_id: str, **kwargs: object) -> publish_module.PublishResult:
        captured_args["repo_id"] = repo_id
        captured_args.update(kwargs)
        return publish_module.PublishResult(
            repo_id=repo_id,
            repo_url=f"https://huggingface.co/{repo_id}",
            revision="main",
            uploaded_files=("README.md", "mlrunx-metadata.json"),
        )

    monkeypatch.setattr(publish_module, "publish_to_huggingface", fake_publish)

    exit_code = publish_module.main(
        [
            "--repo-id",
            "mlrunx/example-model",
            "--weights",
            "weights.bin",
            "--run-id",
            "run-123",
        ]
    )

    captured = capsys.readouterr()
    assert exit_code == 0
    assert captured_args == {
        "repo_id": "mlrunx/example-model",
        "weights": "weights.bin",
        "config": None,
        "model_card": None,
        "run_id": "run-123",
        "project_id": None,
        "run_name": None,
        "private": False,
        "revision": "main",
        "commit_message": None,
    }
    assert "Published 2 files to https://huggingface.co/mlrunx/example-model (main)" in captured.out
