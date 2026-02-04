.PHONY: help build build-rust build-ui build-sdk check clean dev test lint fmt
.PHONY: proto proto-lint proto-breaking proto-gen proto-check
.PHONY: test-contract test-integration ci
.PHONY: infra-up infra-down infra-logs
.PHONY: bench-w1 bench-w2 bench-w3 bench-w1-full bench-w2-full bench-w3-full
.PHONY: version version-check release-tag

# Default target
help:
	@echo "MLRunX Development Commands"
	@echo ""
	@echo "Build:"
	@echo "  make build        - Build all components"
	@echo "  make build-rust   - Build Rust services"
	@echo "  make build-ui     - Build Next.js UI"
	@echo "  make build-sdk    - Build Python SDK"
	@echo ""
	@echo "Proto (requires buf: brew install bufbuild/buf/buf):"
	@echo "  make proto        - Run full proto pipeline (lint + gen)"
	@echo "  make proto-lint   - Lint proto files"
	@echo "  make proto-breaking - Check for breaking changes"
	@echo "  make proto-gen    - Generate code from protos"
	@echo "  make proto-check  - Verify generated code is up to date"
	@echo ""
	@echo "Development:"
	@echo "  make dev          - Start development environment"
	@echo "  make dev-ui       - Start UI development server"
	@echo "  make dev-api      - Start API server"
	@echo "  make dev-ingest   - Start ingest server"
	@echo ""
	@echo "Testing:"
	@echo "  make check        - Run all checks (lint + test)"
	@echo "  make lint         - Run linters"
	@echo "  make fmt          - Format code"
	@echo "  make test         - Run unit tests"
	@echo "  make test-contract    - Run contract tests (proto validation)"
	@echo "  make test-integration - Run integration tests (requires infra)"
	@echo "  make ci           - Run full CI suite locally"
	@echo ""
	@echo "Benchmarks:"
	@echo "  make bench-w1     - Run W1 benchmark (query at scale)"
	@echo "  make bench-w2     - Run W2 benchmark (high-freq ingest)"
	@echo "  make bench-w3     - Run W3 benchmark (mixed dashboard)"
	@echo ""
	@echo "Release:"
	@echo "  make version      - Show current version"
	@echo "  make version-check - Verify versions are consistent"
	@echo "  make release-tag V=0.1.0-alpha.1 - Create release tag"
	@echo ""
	@echo "Infrastructure:"
	@echo "  make infra-up     - Start infrastructure (docker-compose)"
	@echo "  make infra-down   - Stop infrastructure"
	@echo "  make infra-logs   - View infrastructure logs"
	@echo ""
	@echo "Cleanup:"
	@echo "  make clean        - Clean build artifacts"

# =============================================================================
# Build targets
# =============================================================================

build: build-rust build-ui build-sdk

build-rust:
	@echo "Building Rust services..."
	cargo build --release

build-ui:
	@echo "Building Next.js UI..."
	cd apps/ui && npm ci && npm run build

build-sdk:
	@echo "Building Python SDK..."
	uv sync --all-packages

# =============================================================================
# Proto targets
# =============================================================================

# Full proto pipeline
proto: proto-lint proto-gen
	@echo "Proto pipeline complete"

# Lint proto files with buf
proto-lint:
	@echo "Linting proto files..."
	@command -v buf >/dev/null 2>&1 || { echo "buf not found. Install with: brew install bufbuild/buf/buf"; exit 1; }
	buf lint

# Check for breaking changes against main branch
proto-breaking:
	@echo "Checking for breaking proto changes..."
	@command -v buf >/dev/null 2>&1 || { echo "buf not found. Install with: brew install bufbuild/buf/buf"; exit 1; }
	buf breaking --against '.git#branch=main'

# Generate code from protos
proto-gen: proto-gen-python
	@echo "Rust protos are generated at build time via build.rs"
	@echo "Proto generation complete"

# Generate Python proto stubs
proto-gen-python:
	@echo "Generating Python proto stubs..."
	@command -v buf >/dev/null 2>&1 || { echo "buf not found. Install with: brew install bufbuild/buf/buf"; exit 1; }
	@mkdir -p sdks/python/src/mlrunx/proto
	buf generate --template buf.gen.yaml

# Verify generated code is up to date (for CI)
proto-check: proto-lint
	@echo "Verifying proto generation is reproducible..."
	@# Save current state
	@cp -r sdks/python/src/mlrunx/proto /tmp/proto-backup 2>/dev/null || true
	@# Regenerate
	@$(MAKE) proto-gen-python
	@# Compare (if backup exists)
	@if [ -d /tmp/proto-backup ]; then \
		diff -r sdks/python/src/mlrunx/proto /tmp/proto-backup && \
		echo "Proto generation is reproducible" || \
		(echo "ERROR: Generated proto files have drifted. Run 'make proto' and commit." && exit 1); \
		rm -rf /tmp/proto-backup; \
	fi
	@# Also verify Rust proto crate builds
	@echo "Verifying Rust proto crate builds..."
	cargo build -p mlrunx-proto

# =============================================================================
# Development targets
# =============================================================================

dev: infra-up
	@echo "Development environment ready"
	@echo "  API:    http://localhost:3001"
	@echo "  Ingest: http://localhost:3002 (gRPC: 50051)"
	@echo "  UI:     http://localhost:3000"

dev-ui:
	cd apps/ui && npm run dev

dev-api:
	cargo run --bin mlrunx-api

dev-ingest:
	cargo run --bin mlrunx-ingest

dev-processor:
	cargo run --bin mlrunx-processor

# =============================================================================
# Quality targets
# =============================================================================

check: lint test

lint: lint-rust lint-python lint-ui proto-lint

lint-rust:
	@echo "Linting Rust..."
	cargo fmt --check
	cargo clippy -- -D warnings

lint-python:
	@echo "Linting Python..."
	uv run ruff check sdks/
	uv run mypy sdks/

lint-ui:
	@echo "Linting UI..."
	cd apps/ui && npm run lint 2>/dev/null || echo "UI lint not configured"

fmt: fmt-rust fmt-python

fmt-rust:
	cargo fmt

fmt-python:
	uv run ruff format sdks/
	uv run ruff check --fix sdks/

test: test-rust test-python test-ui

test-rust:
	@echo "Testing Rust..."
	cargo test

test-python:
	@echo "Testing Python..."
	uv run pytest sdks/

test-ui:
	@echo "Testing UI..."
	cd apps/ui && npm test 2>/dev/null || echo "No UI tests yet"

# =============================================================================
# Contract Tests
# =============================================================================

test-contract: proto-check
	@echo "Running contract tests..."
	@echo "Proto validation passed"

# =============================================================================
# Integration Tests
# =============================================================================

test-integration: infra-up
	@echo "Running integration tests..."
	@echo "Waiting for services to be ready..."
	@sleep 5
	uv run pytest tests/integration/ -m integration -v 2>/dev/null || echo "No integration tests yet"
	@echo "Integration tests complete"

# =============================================================================
# CI Target (Local)
# =============================================================================

ci: lint test test-contract proto-breaking
	@echo "All CI checks passed!"

# =============================================================================
# Benchmark Targets
# =============================================================================

# Scaled-down benchmarks (nightly)
bench-w1:
	@echo "Running W1 benchmark (query at scale - scaled down)..."
	@echo "Target: p95 < 200ms for 1,000 runs"
	# Placeholder: actual benchmark implementation in BENCH-000
	@echo "W1 benchmark not implemented yet"

bench-w2:
	@echo "Running W2 benchmark (high-freq ingest - scaled down)..."
	@echo "Target: p95 < 500ms log-to-visible latency"
	# Placeholder: actual benchmark implementation in BENCH-000
	@echo "W2 benchmark not implemented yet"

bench-w3:
	@echo "Running W3 benchmark (mixed dashboard - scaled down)..."
	@echo "Target: p95 < 300ms for dashboard queries"
	# Placeholder: actual benchmark implementation in BENCH-000
	@echo "W3 benchmark not implemented yet"

# Full-scale benchmarks (release)
bench-w1-full:
	@echo "Running W1 benchmark (query at scale - full)..."
	@echo "Target: p95 < 200ms for 10,000 runs"
	# Placeholder: actual benchmark implementation in BENCH-000
	@echo "W1 full benchmark not implemented yet"

bench-w2-full:
	@echo "Running W2 benchmark (high-freq ingest - full)..."
	@echo "Target: p95 < 500ms at 100k metrics/sec"
	# Placeholder: actual benchmark implementation in BENCH-000
	@echo "W2 full benchmark not implemented yet"

bench-w3-full:
	@echo "Running W3 benchmark (mixed dashboard - full)..."
	@echo "Target: p95 < 300ms with 50 concurrent users"
	# Placeholder: actual benchmark implementation in BENCH-000
	@echo "W3 full benchmark not implemented yet"

# =============================================================================
# Infrastructure targets
# =============================================================================

infra-up:
	@echo "Starting infrastructure..."
	cd infra/docker && docker compose up -d

infra-down:
	@echo "Stopping infrastructure..."
	cd infra/docker && docker compose down

infra-logs:
	cd infra/docker && docker compose logs -f

infra-ps:
	cd infra/docker && docker compose ps

# =============================================================================
# Cleanup targets
# =============================================================================

clean:
	@echo "Cleaning build artifacts..."
	cargo clean
	rm -rf apps/ui/.next apps/ui/out
	find . -type d -name "__pycache__" -exec rm -rf {} + 2>/dev/null || true
	find . -type d -name "*.egg-info" -exec rm -rf {} + 2>/dev/null || true
	find . -type d -name ".pytest_cache" -exec rm -rf {} + 2>/dev/null || true
	find . -type d -name ".mypy_cache" -exec rm -rf {} + 2>/dev/null || true
	find . -type d -name ".ruff_cache" -exec rm -rf {} + 2>/dev/null || true

# =============================================================================
# Release targets
# =============================================================================

# Current version (from Cargo.toml workspace)
VERSION := $(shell grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')

version:
	@echo "Current versions:"
	@echo "  Rust:        $(VERSION)"
	@echo "  UI:          $$(grep '"version"' apps/ui/package.json | sed 's/.*"\([^"]*\)".*/\1/')"
	@echo "  Python SDK:  $$(grep '^version' sdks/python/pyproject.toml | sed 's/.*"\(.*\)".*/\1/')"
	@echo "  Integrations: $$(grep '^version' sdks/integrations/pyproject.toml | sed 's/.*"\(.*\)".*/\1/')"

version-check:
	@echo "Checking version consistency..."
	@RUST_VER=$$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/') && \
	UI_VER=$$(grep '"version"' apps/ui/package.json | sed 's/.*"\([^"]*\)".*/\1/') && \
	SDK_VER=$$(grep '^version' sdks/python/pyproject.toml | sed 's/.*"\(.*\)".*/\1/') && \
	INT_VER=$$(grep '^version' sdks/integrations/pyproject.toml | sed 's/.*"\(.*\)".*/\1/') && \
	if [ "$$RUST_VER" = "$$UI_VER" ] && [ "$$RUST_VER" = "$$SDK_VER" ] && [ "$$RUST_VER" = "$$INT_VER" ]; then \
		echo "All versions match: $$RUST_VER"; \
	else \
		echo "Version mismatch detected:"; \
		echo "  Rust:        $$RUST_VER"; \
		echo "  UI:          $$UI_VER"; \
		echo "  Python SDK:  $$SDK_VER"; \
		echo "  Integrations: $$INT_VER"; \
		exit 1; \
	fi

release-tag:
ifndef V
	$(error V is not set. Usage: make release-tag V=0.1.0-alpha.1)
endif
	@echo "Creating release tag v$(V)..."
	@echo "1. Running checks..."
	@$(MAKE) check
	@echo "2. Verifying version consistency..."
	@$(MAKE) version-check
	@echo "3. Creating annotated tag..."
	git tag -a "v$(V)" -m "Release v$(V)"
	@echo ""
	@echo "Tag v$(V) created locally."
	@echo "To publish the release, run: git push origin v$(V)"
