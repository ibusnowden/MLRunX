# MLRunX Release Channels (Stable vs Dev)

This guide defines how MLRunX ships Python packages while we move from wheel-first delivery to PyPI.

## Channel Model

| Channel | Audience | Install Method | Cadence |
|---------|----------|----------------|---------|
| Stable | Most users | Wheel now; `pip install mlrunx` later on PyPI | Tagged releases |
| Dev | Contributors and testers | Git install pinned to branch/commit | Any merge |

## Install Commands

### Stable (Current: wheel-first)

```bash
cd sdks/python
uv build
pip install dist/mlrunx-*.whl
```

```bash
cd sdks/integrations
uv build
pip install dist/mlrunx_integrations-*.whl
```

### Dev (Latest source)

```bash
pip install "git+https://github.com/<org>/<repo>.git@<commit>#subdirectory=sdks/python"
pip install "git+https://github.com/<org>/<repo>.git@<commit>#subdirectory=sdks/integrations"
```

Use a commit SHA for reproducibility.

## Versioning Rules

- Stable releases: `X.Y.Z` (PEP 440 compliant).
- Dev snapshots: `X.Y.Z.devN` (or install by commit SHA).
- Keep `sdks/python/pyproject.toml` and `sdks/integrations/pyproject.toml` on the same version.

## Common Release Checklist

- [ ] `uv sync --all-packages`
- [ ] `uv run pytest sdks/python/tests -q`
- [ ] `uv run pytest sdks/integrations/tests -q`
- [ ] `cargo build --workspace`
- [ ] `make proto`
- [ ] `git status` is clean except intentional version/changelog changes

## Stable Release Checklist (Before PyPI)

- [ ] Bump version in `sdks/python/pyproject.toml`
- [ ] Bump version in `sdks/integrations/pyproject.toml`
- [ ] Update `CHANGELOG.md`
- [ ] Create and push annotated tag: `git tag -a vX.Y.Z -m "MLRunX vX.Y.Z"`
- [ ] Trigger/verify `.github/workflows/release.yml`
- [ ] Confirm wheel artifacts are attached to GitHub Release
- [ ] Smoke test in a clean env: `pip install <wheel>` then `mlrunx --version`

## Dev Release Checklist

- [ ] Merge to the target dev branch
- [ ] Share pinned install commands (commit SHA)
- [ ] Smoke test `pip install git+...@<commit>` in a clean env
- [ ] Post compatibility notes if schema/env vars changed

## PyPI Enablement Checklist (Future)

- [ ] Create PyPI projects: `mlrunx`, `mlrunx-integrations`
- [ ] Configure Trusted Publishing for this GitHub repo
- [ ] Add publish step in `.github/workflows/release.yml` (`uv publish`)
- [ ] Dry run on TestPyPI with a pre-release version
- [ ] Publish first stable to PyPI
- [ ] Update README install docs to prefer `pip install mlrunx`
