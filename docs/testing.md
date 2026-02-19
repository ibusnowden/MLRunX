# MLRunX Testing Strategy

> **Status**: Alpha
> **Last Updated**: 2026-01

This document defines the testing strategy, test layers, and CI gates for MLRunX.

## Table of Contents

- [Overview](#overview)
- [Test Layers](#test-layers)
- [CI Gates](#ci-gates)
- [Test Locations](#test-locations)
- [Canonical Commands](#canonical-commands)
- [Performance Benchmarks](#performance-benchmarks)
- [Writing Tests](#writing-tests)

---

## Overview

MLRunX uses a layered testing strategy that balances fast feedback with thorough validation:

```
┌─────────────────────────────────────────────────────────────────┐
│                         Release Gate                            │
│  Integration + Performance thresholds must pass                 │
├─────────────────────────────────────────────────────────────────┤
│                         Nightly                                 │
│  Scaled-down W1/W2/W3 performance suite                         │
├─────────────────────────────────────────────────────────────────┤
│                         PR Gate (Phase 2)                       │
│  Integration tests (docker-compose stack)                       │
├─────────────────────────────────────────────────────────────────┤
│                         PR Gate (Required)                      │
│  Lint + Unit + Contract tests                                   │
└─────────────────────────────────────────────────────────────────┘
                              ▲
                              │
                         Every Commit
```

### Principles

1. **Fast feedback**: PR gates complete in <5 minutes
2. **Deterministic**: No flaky tests in required gates
3. **Canonical commands**: CI uses same `make` targets as developers
4. **Shift left**: Catch issues as early as possible
5. **No soft-fail gates**: lint/test targets must fail hard on regressions

---

## Test Layers

### Unit Tests

**Purpose**: Test individual functions, modules, and components in isolation.

| Property | Value |
|----------|-------|
| Trigger | Every PR |
| Runtime target | < 2 minutes |
| Dependencies | None (mocked) |
| Parallelism | Full |

**Characteristics**:
- No network calls
- No database connections
- No file system (except temp)
- Fully deterministic

**Examples**:
- Metric batching logic
- Downsampling algorithms (LTTB, MIN_MAX)
- Proto serialization
- SDK batching/buffering
- UI component rendering

### Contract Tests

**Purpose**: Verify proto definitions compile and services implement contracts correctly.

| Property | Value |
|----------|-------|
| Trigger | Every PR |
| Runtime target | < 1 minute |
| Dependencies | None |
| Parallelism | Full |

**Characteristics**:
- Proto compilation checks
- Request/response schema validation
- Wire format compatibility
- Generated code verification

**Examples**:
- Proto syntax validation
- Enum value consistency
- Required field presence
- Breaking change detection

### Integration Tests

**Purpose**: Test service interactions with real dependencies (databases, message queues).

| Property | Value |
|----------|-------|
| Trigger | PR (phase-in), Nightly, Release |
| Runtime target | < 10 minutes |
| Dependencies | Docker Compose stack |
| Parallelism | Limited (shared resources) |

**Characteristics**:
- Real database connections
- Actual service communication
- End-to-end flows
- Data persistence verification

**Examples**:
- Ingest → ClickHouse write → Query read
- Artifact upload → MinIO → Download
- Run lifecycle (init → log → finish)
- API authentication flows

### Performance/Benchmark Tests

**Purpose**: Validate performance targets and detect regressions.

| Property | Value |
|----------|-------|
| Trigger | Nightly, Release |
| Runtime target | 15-30 minutes |
| Dependencies | Full stack + load generators |
| Parallelism | Serial (for consistent measurements) |

**Workloads** (see [BENCH-000]):

| Workload | Description | Target |
|----------|-------------|--------|
| **W1** | 10k runs list/filter | p95 < 200ms |
| **W2** | High-frequency ingest | p95 < 500ms log-to-visible |
| **W3** | Mixed dashboard queries | p95 < 300ms |

---

## CI Gates

### PR Gate (Required)

Every pull request must pass:

```yaml
PR Gate:
  - lint-rust      # cargo fmt --check, cargo clippy
  - lint-python    # ruff check, mypy
  - lint-ui        # eslint, tsc
  - test-unit      # cargo test, pytest, jest
  - test-contract  # proto compilation
```

**Blocking**: PRs cannot merge without passing.

**Runtime**: < 5 minutes total.

### PR Gate (Phase-in)

After integration tests stabilize:

```yaml
PR Gate (Extended):
  - all of above
  - test-integration  # docker-compose based
```

**Status**: Not required until M3 milestone.

### Nightly Gate

Runs every night at 02:00 UTC:

```yaml
Nightly:
  - all PR gates
  - test-integration
  - bench-w1        # Scaled-down W1
  - bench-w2        # Scaled-down W2
  - bench-w3        # Scaled-down W3
```

**Alerting**: Failures notify #mlrunx-ci Slack channel.

### Release Gate

Runs on version tags (v*):

```yaml
Release:
  - all PR gates
  - test-integration
  - bench-w1-full   # Full-scale W1
  - bench-w2-full   # Full-scale W2
  - bench-w3-full   # Full-scale W3
  - threshold check # Performance must meet targets
```

**Blocking**: Release artifacts not published without passing.

---

## Test Locations

```
mlrunx/
├── tests/
│   ├── unit/           # Unit tests (all languages)
│   │   ├── rust/       # Rust unit tests (also in-crate)
│   │   ├── python/     # Python SDK unit tests
│   │   └── ui/         # React component tests
│   ├── contract/       # Proto contract tests
│   │   ├── proto/      # Proto compilation tests
│   │   └── schemas/    # Schema validation tests
│   └── integration/    # Integration tests
│       ├── ingest/     # Ingest service tests
│       ├── query/      # Query service tests
│       └── e2e/        # End-to-end flows
├── bench/
│   ├── generators/     # Synthetic data generators
│   └── workloads/      # W1/W2/W3 workload definitions
│       ├── w1/         # 10k runs queries
│       ├── w2/         # High-freq ingest
│       └── w3/         # Mixed dashboard
├── apps/
│   └── ui/
│       └── __tests__/  # Co-located UI tests
├── sdks/
│   └── python/
│       └── tests/      # Co-located SDK tests
└── services/
    ├── ingest/
    │   └── src/        # In-crate Rust tests (#[cfg(test)])
    └── processor/
        └── src/        # In-crate Rust tests
```

### Rust Tests

Rust tests follow the standard conventions:

```rust
// In-crate unit tests (src/lib.rs or src/*.rs)
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_something() {
        // ...
    }
}

// Integration tests (tests/*.rs)
// tests/integration_test.rs
```

### Python Tests

Python tests use pytest with markers:

```python
# sdks/python/tests/test_client.py
import pytest

@pytest.mark.unit
def test_batch_buffer():
    """Unit test - no external deps."""
    pass

@pytest.mark.integration
def test_ingest_flow():
    """Integration test - requires running services."""
    pass
```

### UI Tests

UI tests use Vitest/Jest with React Testing Library:

```typescript
// apps/ui/__tests__/components/RunsTable.test.tsx
import { render, screen } from '@testing-library/react'
import { RunsTable } from '@/components/RunsTable'

describe('RunsTable', () => {
  it('renders runs', () => {
    render(<RunsTable runs={mockRuns} />)
    expect(screen.getByRole('table')).toBeInTheDocument()
  })
})
```

---

## Canonical Commands

All CI workflows use these `make` targets. Developers use the same commands locally.

### Quick Reference

| Command | Description | CI Gate |
|---------|-------------|---------|
| `make lint` | Run all linters | PR |
| `make test` | Run all unit tests | PR |
| `make test-contract` | Run contract tests | PR |
| `make test-integration` | Run integration tests | Nightly/Release |
| `make bench-w1` | Run W1 benchmark | Nightly |
| `make bench-w2` | Run W2 benchmark | Nightly |
| `make bench-w3` | Run W3 benchmark | Nightly |
| `make check` | Run lint + test | PR |

### Detailed Commands

```bash
# === Linting ===
make lint               # All linters
make lint-rust          # cargo fmt --check && cargo clippy
make lint-python        # ruff check && mypy
make lint-ui            # eslint && tsc --noEmit

# === Unit Tests ===
make test               # All unit tests
make test-rust          # cargo test
make test-python        # uv run pytest sdks/
make test-ui            # cd apps/ui && npm test

# === Contract Tests ===
make test-contract      # Proto compilation + schema validation
make proto-check        # Verify protos compile

# === Integration Tests ===
make test-integration   # Full integration suite (requires infra)
make infra-up           # Start test infrastructure
make infra-down         # Stop test infrastructure

# === Benchmarks ===
make bench-w1           # 10k runs query benchmark (scaled-down)
make bench-w2           # High-freq ingest benchmark (scaled-down)
make bench-w3           # Mixed dashboard benchmark (scaled-down)
make bench-w1-full      # Full-scale W1 (release only)
make bench-w2-full      # Full-scale W2 (release only)
make bench-w3-full      # Full-scale W3 (release only)

# === Combined ===
make check              # lint + test (PR gate locally)
make ci                 # Full CI suite (lint + test + contract)
```

---

## Performance Benchmarks

### Workload W1: Query at Scale

Tests query performance with 10,000 runs.

**Scaled-down** (nightly):
- 1,000 runs
- 100 queries
- Target: p95 < 200ms

**Full-scale** (release):
- 10,000 runs
- 1,000 queries
- Target: p95 < 200ms

### Workload W2: High-Frequency Ingest

Tests ingest throughput and log-to-visible latency.

**Scaled-down** (nightly):
- 10 concurrent runs
- 1,000 metrics/sec total
- Target: p95 < 500ms

**Full-scale** (release):
- 100 concurrent runs
- 100,000 metrics/sec total
- Target: p95 < 500ms

### Workload W3: Mixed Dashboard

Tests realistic dashboard usage patterns.

**Scaled-down** (nightly):
- 5 concurrent users
- Mixed read/write
- Target: p95 < 300ms

**Full-scale** (release):
- 50 concurrent users
- Mixed read/write
- Target: p95 < 300ms

### Threshold Enforcement

Release builds fail if benchmarks exceed targets:

```yaml
# In release gate
- name: Check performance thresholds
  run: |
    if [ "$W1_P95" -gt 200 ]; then
      echo "W1 p95 ($W1_P95 ms) exceeds 200ms threshold"
      exit 1
    fi
```

---

## Writing Tests

### Unit Test Guidelines

1. **One assertion per test** (when practical)
2. **Descriptive names**: `test_batch_flushes_when_buffer_full`
3. **AAA pattern**: Arrange, Act, Assert
4. **No shared state** between tests
5. **Mock external dependencies**

```rust
#[test]
fn test_lttb_preserves_endpoints() {
    // Arrange
    let points = vec![
        MetricPoint { step: 0, value: 1.0 },
        MetricPoint { step: 100, value: 2.0 },
    ];

    // Act
    let downsampled = lttb(&points, 2);

    // Assert
    assert_eq!(downsampled[0].step, 0);
    assert_eq!(downsampled[1].step, 100);
}
```

### Integration Test Guidelines

1. **Setup/teardown**: Clean state before each test
2. **Timeouts**: Set reasonable timeouts for async operations
3. **Retries**: Use retry logic for eventual consistency
4. **Isolation**: Tests should not depend on order

```python
@pytest.fixture
async def clean_project(test_client):
    """Create a clean project for each test."""
    project_id = f"test-{uuid.uuid4()}"
    yield project_id
    # Cleanup after test
    await test_client.delete_project(project_id)

@pytest.mark.integration
async def test_run_lifecycle(clean_project, test_client):
    # Init
    run = await test_client.init_run(clean_project, name="test-run")
    assert run.status == RunStatus.RUNNING

    # Log
    await test_client.log_metrics(run.id, [{"loss": 1.0}])

    # Finish
    await test_client.finish_run(run.id)

    # Verify
    result = await test_client.get_run(run.id)
    assert result.status == RunStatus.FINISHED
```

### Benchmark Guidelines

1. **Warm-up**: Run warm-up iterations before measuring
2. **Stable environment**: Use dedicated benchmark runners
3. **Statistical significance**: Multiple iterations, report percentiles
4. **Baseline comparison**: Compare against previous runs

---

## Related Documents

- [CI Workflow](.github/workflows/ci.yml)
- [Nightly Workflow](.github/workflows/nightly.yml)
- [Benchmark Definitions](bench/workloads/)
- [Contributing Guide](CONTRIBUTING.md)
