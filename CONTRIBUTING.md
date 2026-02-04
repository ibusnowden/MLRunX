# Contributing to MLRunX

Thank you for your interest in contributing to MLRunX! This document provides guidelines and information for contributors.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Getting Started](#getting-started)
- [Development Setup](#development-setup)
- [Making Changes](#making-changes)
- [Definition of Done](#definition-of-done)
- [Code Style](#code-style)
- [Testing](#testing)
- [Documentation](#documentation)
- [Pull Request Process](#pull-request-process)
- [Issue Guidelines](#issue-guidelines)

---

## Code of Conduct

We are committed to providing a welcoming and inclusive environment. Please be respectful and constructive in all interactions.

---

## Getting Started

### Prerequisites

- **Rust**: 1.85+ (install via [rustup](https://rustup.rs/))
- **Node.js**: 22+ (recommend using [nvm](https://github.com/nvm-sh/nvm))
- **Python**: 3.10+ with [uv](https://github.com/astral-sh/uv)
- **Docker**: For running infrastructure locally
- **protoc**: For proto compilation (`brew install protobuf` on macOS)

### Quick Start

```bash
# Clone the repository
git clone https://github.com/ibusnowden/MLRunX.git
cd mlrunx

# Build all components
make build

# Run tests
make test

# Start development environment
make dev
```

---

## Development Setup

### Rust Services

```bash
# Build all Rust services
cargo build

# Run API service
cargo run --bin mlrunx-api

# Run tests
cargo test

# Check formatting and linting
cargo fmt --check
cargo clippy
```

### Python SDK

```bash
# Install dependencies
uv sync --all-packages

# Activate virtual environment
source .venv/bin/activate

# Run tests
pytest sdks/

# Check linting
ruff check sdks/
mypy sdks/
```

### UI

```bash
cd apps/ui

# Install dependencies
npm ci

# Start development server
npm run dev

# Run tests
npm test

# Check linting
npm run lint
```

### Infrastructure

```bash
# Start all services (ClickHouse, PostgreSQL, MinIO, Redis)
make infra-up

# View logs
make infra-logs

# Stop services
make infra-down
```

---

## Making Changes

### Branch Naming

Use descriptive branch names following this pattern:

```
{number}-{issue-id}-{short-description}
```

Examples:
- `1-core-001-repo-monorepo-scaffold`
- `15-ingest-003-clickhouse-schema`
- `42-ui-002-runs-table`

### Commit Messages

Follow conventional commits:

```
type(scope): short description

Longer description if needed.

- Bullet points for details
- Another detail

Fixes #123
```

Types:
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation only
- `style`: Formatting, no code change
- `refactor`: Code change that neither fixes nor adds
- `perf`: Performance improvement
- `test`: Adding tests
- `chore`: Maintenance, CI, etc.

---

## Definition of Done

**No code merges to main without meeting these criteria:**

### Required for ALL PRs

- [ ] All CI checks pass (`make check`)
- [ ] Code follows style guidelines (`make fmt`)
- [ ] Tests added/updated for changes
- [ ] No new warnings in linters
- [ ] PR description explains the change

### Required for Feature PRs

- [ ] Acceptance criteria from issue are met
- [ ] Documentation updated if user-facing
- [ ] Spec documents updated if proto changes

### Required for Performance-Critical PRs

- [ ] Benchmark results included
- [ ] Performance targets still met
- [ ] No regression in existing benchmarks

### Proto Changes

**Any change to proto files requires:**

1. Update spec document first (`/docs/spec/`)
2. Update proto to match spec
3. Verify proto compiles (`make proto-check`)
4. Update generated code
5. Note in PR description

---

## Code Style

### Rust

- Follow standard Rust conventions
- Use `cargo fmt` for formatting
- Address all `clippy` warnings
- Configuration in `rustfmt.toml` and `.clippy.toml`

```rust
// Good
fn calculate_metrics(data: &[MetricPoint]) -> Result<Stats, Error> {
    // Implementation
}

// Avoid
fn calc(d: &[MetricPoint]) -> Result<Stats, Error> {
    // Implementation
}
```

### Python

- Follow PEP 8 with Ruff enforcement
- Use type hints everywhere
- Configuration in `pyproject.toml`

```python
# Good
def log_metrics(self, metrics: dict[str, float], step: int | None = None) -> None:
    """Log metrics to MLRunX."""
    pass

# Avoid
def log_metrics(self, metrics, step=None):
    pass
```

### TypeScript

- Use strict TypeScript
- Follow ESLint rules
- Configuration in `eslint.config.mjs`

```typescript
// Good
interface RunProps {
  id: string;
  name: string;
  status: RunStatus;
}

// Avoid
interface RunProps {
  id: any;
  name: any;
  status: any;
}
```

---

## Testing

See [docs/testing.md](docs/testing.md) for the full testing strategy.

### Running Tests

```bash
# All tests
make test

# Specific language
make test-rust
make test-python
make test-ui

# Contract tests (proto validation)
make test-contract

# Integration tests (requires infra)
make test-integration
```

### Writing Tests

- **Unit tests**: Fast, isolated, no external dependencies
- **Integration tests**: Test service interactions
- **Contract tests**: Validate proto/API contracts

Mark Python tests appropriately:

```python
@pytest.mark.unit
def test_batch_buffer():
    pass

@pytest.mark.integration
def test_ingest_flow():
    pass
```

---

## Documentation

### When to Update Docs

- New features or APIs
- Changed behavior
- Configuration changes
- Proto changes (spec docs first!)

### Documentation Locations

| Type | Location |
|------|----------|
| API specs | `/docs/spec/` |
| Testing guide | `/docs/testing.md` |
| Proto docs | `/proto/README.md` |
| SDK docs | `/sdks/*/README.md` |
| Main README | `/README.md` |

---

## Pull Request Process

1. **Create branch** from `main`
2. **Make changes** following guidelines above
3. **Run checks** locally: `make check`
4. **Push branch** and create PR
5. **Fill out PR template** completely
6. **Request review** from code owners
7. **Address feedback** and update
8. **Merge** when approved and CI passes

### PR Size Guidelines

- Keep PRs focused and reviewable
- Aim for <500 lines of code changes
- Split large features into smaller PRs
- Use draft PRs for work-in-progress

---

## Issue Guidelines

### Creating Issues

Use the appropriate template:

- **Feature**: New functionality
- **Bug**: Something broken
- **Epic**: Large initiative (multiple issues)

### Issue Labels

| Label | Description |
|-------|-------------|
| `type:feature` | New feature |
| `type:bug` | Bug report |
| `type:epic` | Large initiative |
| `type:docs` | Documentation |
| `type:chore` | Maintenance |
| `area:core` | Core infrastructure |
| `area:ingest` | Ingest service |
| `area:query` | Query service |
| `area:ui` | Dashboard UI |
| `area:sdk` | Python SDK |
| `prio:P0` | Critical |
| `prio:P1` | High |
| `prio:P2` | Medium |
| `prio:P3` | Low |

---

## Questions?

- Open a [Discussion](https://github.com/ibusnowden/MLRunX/discussions)
- Check existing [Issues](https://github.com/ibusnowden/MLRunX/issues)
- Read the [Documentation](docs/)

Thank you for contributing to MLRunX!
