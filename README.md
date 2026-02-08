<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/mlrunx-logo-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="docs/mlrunx-logo-light.svg">
    <img alt="MLRunX" src="docs/mlrunx-logo-light.svg" width="380" />
  </picture>
</p>

<h3 align="center"><b>Open-Source ML Experiment Tracking</b></h3>

<p align="center">
  <a href="https://deepwiki.com/ibusnowden/MLRunX">Documentation</a> &middot;
  <a href="https://github.com/ibusnowden/MLRunX/releases">Releases</a> &middot;
  <a href="https://github.com/ibusnowden/MLRunX">GitHub</a>
</p>

<br/>

<p align="center">
  <a href="https://github.com/ibusnowden/MLRunX/actions/workflows/ci.yml">
    <img src="https://github.com/ibusnowden/MLRunX/actions/workflows/ci.yml/badge.svg" alt="CI" />
  </a>
  <a href="https://github.com/ibusnowden/MLRunX/actions/workflows/release.yml">
    <img src="https://github.com/ibusnowden/MLRunX/actions/workflows/release.yml/badge.svg" alt="Release" />
  </a>
  <a href="https://github.com/ibusnowden/MLRunX/releases/tag/v0.1.0">
    <img src="https://img.shields.io/github/v/release/ibusnowden/MLRunX?label=latest" alt="Latest Release" />
  </a>
  <a href="LICENSE">
    <img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="MIT License" />
  </a>
</p>

<br/>

## News & Updates

- **[02/07/26]** `v0.1.0` is released, featuring the Rust API gateway (Axum + SQLite), Next.js dashboard with dark/light theming, project-scoped API keys, role-based access control, shareable view-only links, key management API, Docker image, and GitHub Actions CI/CD.

---

## Preview

> Compare reward curves across RL training runs -- REINFORCE, GSPO, GRPO, PPO, RLOO, CISPO and more.

<p align="center">
  <img src="docs/screenshots/compare-light.png" alt="Compare view — light mode" width="100%" />
</p>
<p align="center"><em>Expanded compare chart — Light mode</em></p>

<br/>

<p align="center">
  <img src="docs/screenshots/compare-dark.png" alt="Compare view — dark mode" width="100%" />
</p>
<p align="center"><em>Full compare dashboard with run selector and statistics — Dark mode</em></p>

<br/>

<p align="center">
  <img src="docs/screenshots/compare-expanded-dark.png" alt="Expanded compare chart — dark mode" width="100%" />
</p>
<p align="center"><em>Expanded compare chart — Dark mode</em></p>

---

## Why MLRunX?

- **Performance-first**: Sub-200ms UI queries at 10k+ runs, high-throughput ingestion with server-side downsampling
- **AI-native**: Built-in eval harness, agent/tool tracing, prompt versioning
- **Local-first**: Full Docker Compose stack, privacy-first defaults, no vendor lock-in
- **Open**: MIT licensed, OSS stack (ClickHouse + Postgres + MinIO)

## How MLRunX Differs from W&B

### Performance-First Architecture

| Aspect | W&B | MLRunX      |
|--------|-----|------------|
| Backend | Python/Go, proprietary | Rust (Axum/Tonic) - lower latency |
| Metrics DB | Proprietary | ClickHouse - built for analytics at scale |
| Query at 10k runs | Often sluggish | Target: p95 < 200ms |
| Log → visible latency | Variable | Target: p95 < 500ms |

### AI-Native from Day One

W&B bolted on LLM features later. MLRunX builds them in:
- **Eval harness**: First-class prompt sets, graders, regression detection
- **Agent tracing**: Spans for tool calls, nested reasoning steps
- **OTLP compatibility**: Bridge ML and infra observability

### Local-First / Privacy-First

| W&B | MLRunX      |
|-----|------------|
| Cloud-first, self-hosted is enterprise tier | Docker Compose works day one |
| Telemetry on by default | No outbound telemetry by default |
| Vendor lock-in | OSS stack (CH + PG + MinIO) |

### SDK Design

| W&B | MLRunX      |
|-----|------------|
| Sync-heavy, can block training | Async-first, non-blocking |
| Network failure = data loss risk | Offline spool with bounded disk |
| ~1-5% overhead reported | Target: < 1% overhead |

### Transparent Benchmarks

W&B doesn't publish performance numbers. MLRunX does:
- **W1**: Run listing at scale (10k runs)
- **W2**: Ingest throughput + latency
- **W3**: Mixed workloads (metrics + traces + evals)
- Reproducible scripts, published results

### Technical Bets

1. **ClickHouse** - 10-100x faster aggregations than general-purpose DBs
2. **Rust ingest** - Predictable latency, no GC pauses
3. **gRPC-first** - Efficient binary protocol, streaming support
4. **Server-side downsampling** - Never send millions of points to browser

## Architecture

```
┌─────────────┐     ┌─────────────┐     ┌─────────────────┐
│  Python SDK │────▶│   Ingest    │────▶│   ClickHouse    │
│  (async +   │     │  (Rust/gRPC)│     │  (metrics/traces)│
│   spool)    │     └─────────────┘     └─────────────────┘
└─────────────┘            │
                           ▼
┌─────────────┐     ┌─────────────┐     ┌─────────────────┐
│   Next.js   │◀───▶│  API Gateway│◀───▶│    Postgres     │
│     UI      │     │  (Rust/Axum)│     │   (metadata)    │
└─────────────┘     └─────────────┘     └─────────────────┘
                           │
                           ▼
                    ┌─────────────┐     ┌─────────────────┐
                    │  Processor  │     │     MinIO       │
                    │  (rollups)  │     │   (artifacts)   │
                    └─────────────┘     └─────────────────┘
```

## Project Structure

```
MLRunX/
├── apps/
│   ├── ui/                 # Next.js dashboard (TypeScript)
│   └── api/                # Rust API gateway (Axum)
├── services/
│   ├── ingest/             # Rust ingest service (gRPC/HTTP)
│   └── processor/          # Rollups, downsampling, cardinality guards
├── sdks/
│   ├── python/             # Python SDK (async batching + offline spool)
│   └── integrations/       # Lightning, Hydra, Optuna, HuggingFace hooks
├── infra/
│   ├── docker/             # Docker Compose for local dev
│   ├── k8s/                # Helm charts and manifests
│   └── observability/      # OpenTelemetry collector config
├── docs/                   # Documentation
├── bench/
│   ├── generators/         # Synthetic data generators
│   └── workloads/          # W1/W2/W3 benchmark definitions
└── migrations/
    ├── wandb/              # W&B export/import tools
    └── mlflow/             # MLflow adapters
```

## Tech Stack

| Component | Technology |
|-----------|------------|
| **Metrics Storage** | ClickHouse |
| **Metadata Store** | PostgreSQL |
| **Artifact Storage** | MinIO (S3-compatible) |
| **Ingest Service** | Rust + Tonic (gRPC) |
| **API Gateway** | Rust + Axum |
| **Dashboard** | Next.js + TypeScript + Tailwind |
| **Python SDK** | Python 3.10+ (async, httpx, pydantic) |
| **Observability** | OpenTelemetry |

## Quick Start

### Prerequisites

- Docker & Docker Compose (recommended: [OrbStack](https://orbstack.dev/) for macOS)
- (For development) Rust 1.85+, Node.js 22+, Python 3.10+, uv

### Run Locally

```bash
# Start infrastructure services
cd infra/docker
docker compose up -d clickhouse postgres minio redis otel-collector

# Verify all services are healthy
docker compose ps

# View logs
docker compose logs -f
```

### Services & Ports

| Service | Port | Description |
|---------|------|-------------|
| **ClickHouse** | 8123 (HTTP), 9000 (TCP) | Metrics and traces storage |
| **PostgreSQL** | 5432 | Metadata (runs, params, tags, API keys) |
| **MinIO** | 9001 (API), 9002 (Console) | Artifact storage (S3-compatible) |
| **Redis** | 6379 | Queue and cache |
| **OTEL Collector** | 4317 (gRPC), 4318 (HTTP), 8889 (metrics) | Telemetry collection |
| **UI** | 3000 | Next.js dashboard |
| **API** | 3001 | Rust API gateway |
| **Ingest** | 3002 (HTTP), 50051 (gRPC) | Ingest service |

### Insecure Local Dev Defaults (Docker Only)

> ⚠️ These defaults are for local Docker quickstart only. Do **not** use them in staging/production or on internet-exposed hosts.
> For hosted deployments, copy `infra/docker/.env.example` to `infra/docker/.env` and set strong random secrets.

| Service | Env Vars | Local Default |
|---------|----------|---------------|
| ClickHouse | `CLICKHOUSE_USER` / `CLICKHOUSE_PASSWORD` | `track` / `track_dev` |
| PostgreSQL | `POSTGRES_USER` / `POSTGRES_PASSWORD` | `track` / `track_dev` |
| MinIO | `MINIO_ACCESS_KEY` / `MINIO_SECRET_KEY` | `track` / `track_dev_secret` |

### Development Setup

```bash
# Clone the repo
git clone https://github.com/your-org/MLRunX.git
cd mlrunx

# Python SDK development
uv sync --all-packages
source .venv/bin/activate

# Rust services
cargo check

# UI development
cd apps/ui && npm install && npm run dev
```

### Package Release Flow

- Stable vs dev release process: `docs/ops/release_channels.md`

### Testing Connectivity

```bash
# If you changed credentials in infra/docker/.env, replace values below.

# ClickHouse
docker exec track-clickhouse clickhouse-client --user track --password track_dev --query "SELECT 1"

# PostgreSQL
docker exec track-postgres psql -U track -d track -c "SELECT 1"

# Redis
docker exec track-redis redis-cli PING

# MinIO Console
open http://localhost:9002
```

## Roadmap 2026

### Phase 1: MVP + Core (Q1-Q2)
- [X] M0: Project scaffolding + CI
- [X] M1: Local-first single-user alpha
- [ ] M2: High-throughput ingest + ClickHouse schema
- [ ] M3: UI v0 (runs table, compare view, charts)
- [ ] M4: One-click Docker + basic K8s
- [ ] M5: Benchmarks W1/W2 + alpha report

### Phase 2: AI-Native Edge (Q2-Q3)
- [ ] M6: LLM Evals v0 (prompt sets, graders, comparison UI)
- [ ] M7: Agent tracing + OpenTelemetry compatibility
- [ ] M8: Integrations v1 (HF, Optuna, Lightning, Hydra)
- [ ] M9: Reliability (offline spool, retry, retention/rollups)

### Phase 3: OSS + Migration (Q3-Q4)
- [ ] M10: Migration tools (W&B, MLflow importers)
- [ ] M11: OSS release + docs + examples
- [ ] M12: Beta → v1.0 hardening

### Phase 4: Enterprise (Q4+)
- [ ] RBAC, audit logs, federation, multi-region

## Contributors

- Codex

## Benchmarks

MLRunX targets measurable performance:

| Workload | Metric | Target |
|----------|--------|--------|
| **W1**: 10k runs | List/filter p95 | < 200ms |
| **W2**: High-freq ingest | Log → visible p95 | < 500ms |
| **W3**: Mixed (metrics + traces + evals) | Dashboard p95 | < 300ms |

## SDK Usage (Preview)

```python
import mlrunx

# Initialize a run
run = mlrunx.init(project="my-project", name="training-run-1")

# Log metrics (async, batched automatically)
for step in range(1000):
    run.log({"loss": loss, "accuracy": acc}, step=step)

# Log artifacts
run.log_artifact("model.pt", type="model")

# Finish
run.finish()
```

## Integrations (Preview)

```python
# PyTorch Lightning
from mlrunx.integrations import TrackLogger
trainer = Trainer(logger=TrackLogger())

# HuggingFace Transformers
from mlrunx.integrations import TrackCallback
trainer.add_callback(TrackCallback())

# Optuna
from mlrunx.integrations import TrackOptunaCallback
study.optimize(objective, callbacks=[TrackOptunaCallback()])
```
## Contributing

See [CONTRIBUTING.md](docs/CONTRIBUTING.md) for guidelines.

## License

MIT License - see [LICENSE](LICENSE) for details.
