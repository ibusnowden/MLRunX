# MLRunX Architecture

## Overview

MLRunX is designed as a monolith-first architecture, running as a single binary that serves both HTTP and gRPC protocols. This approach simplifies deployment and debugging while maintaining clear internal boundaries for future microservice decomposition.

## Components

```
┌──────────────────────────────────────────────────────────────┐
│                      MLRunX API Server                         │
│                                                               │
│  ┌─────────────────┐     ┌─────────────────┐                 │
│  │  HTTP (axum)    │     │  gRPC (tonic)   │                 │
│  │  :3001          │     │  :50051         │                 │
│  └────────┬────────┘     └────────┬────────┘                 │
│           │                       │                           │
│           ▼                       ▼                           │
│  ┌────────────────────────────────────────────────────┐      │
│  │                 Shared Services                      │      │
│  │  • Auth (API Keys)                                   │      │
│  │  • Idempotency (Batch Deduplication)                 │      │
│  │  • Cardinality Guardrails                            │      │
│  └────────────────────────────────────────────────────┘      │
│           │                                                   │
│           ▼                                                   │
│  ┌────────────────────────────────────────────────────┐      │
│  │              Storage Layer (Direct Mode)             │      │
│  └────────────────────────────────────────────────────┘      │
└──────────────────────────────────────────────────────────────┘
           │                       │                │
           ▼                       ▼                ▼
    ┌──────────┐           ┌──────────┐      ┌──────────┐
    │PostgreSQL│           │ClickHouse│      │  MinIO   │
    │ metadata │           │  metrics │      │artifacts │
    └──────────┘           └──────────┘      └──────────┘
```

## Ingest Modes

MLRunX supports different ingest modes to accommodate various deployment scenarios:

### Direct Mode (Alpha - Default)

In direct mode, the API server writes directly to storage backends without any intermediate queue. This is the default for alpha releases.

**Configuration:**
```bash
INGEST_MODE=direct  # default
```

**Data Flow:**
```
SDK → API Server → Validate/Auth → Idempotency Check → Storage (CH/PG)
```

**Characteristics:**
- Simplest deployment (no Redis/Kafka required)
- Lower latency for small deployments
- Best for development and single-node production
- Backpressure handled by SDK's offline spool
- Suitable for ~100 concurrent writers

**When to use:**
- Development and testing
- Small to medium deployments
- When simplicity is more important than scale

### Queued Mode (Future)

In queued mode, ingestion goes through a message queue (Redis/Kafka) for buffering and horizontal scaling.

**Configuration:**
```bash
INGEST_MODE=queued  # future
REDIS_URL=redis://localhost:6379
```

**Data Flow:**
```
SDK → API Server → Validate/Auth → Queue → Processor → Storage
```

**Characteristics:**
- Better throughput for high-volume ingestion
- Horizontal scaling of processors
- Better isolation of storage write load
- Required for multi-region deployments

**When to use:**
- High-volume production deployments
- When you need horizontal scaling
- Multi-region setups

## Storage Architecture

### PostgreSQL (Metadata)

Stores structured metadata for relational queries:
- **projects**: Organizational units
- **runs**: Training experiment records
- **parameters**: Hyperparameters and configs
- **artifacts**: File/model metadata pointers
- **api_keys**: Authentication
- **ingest_batches**: Idempotency tracking

### ClickHouse (Metrics)

Stores time-series metrics with efficient compression:
- **metrics**: Individual metric points (MergeTree)
- **metrics_summary**: Aggregated summaries (MaterializedView)
- **system_metrics**: Runtime resource usage

### MinIO/S3 (Artifacts)

Stores binary artifacts with presigned URL access:
- Model checkpoints
- Datasets
- Plots and visualizations
- Logs

## Service Boundaries

### Auth Service
- API key validation and hashing
- Bootstrap key from environment
- Dev mode bypass for testing

### Idempotency Service
- Tracks batches by (run_id, batch_id)
- Verifies payload hash for duplicates
- Rejects conflicting retries
- Enables safe SDK retries

### Cardinality Service
- Enforces tag/metric limits per run
- Enforces total tags per project
- Degrades gracefully (drops with warnings)
- Protects ClickHouse from explosion

## Configuration

All configuration is via environment variables:

### Server
```bash
API_HOST=0.0.0.0
API_HTTP_PORT=3001
API_GRPC_PORT=50051
RUST_LOG=info,mlrunx_api=debug
```

### Authentication
```bash
MLRUNX_API_KEY=mlrunx_<random>  # Bootstrap key
MLRUNX_AUTH_DISABLED=false     # Dev mode only!
```

### Cardinality Limits
```bash
MLRUNX_MAX_TAG_KEYS_PER_RUN=100
MLRUNX_MAX_METRIC_NAMES_PER_RUN=1000
MLRUNX_MAX_TAGS_PER_PROJECT=10000
```

### Storage Backends
```bash
# PostgreSQL
DATABASE_URL=postgres://user:pass@host:5432/mlrunx

# ClickHouse
CLICKHOUSE_HOST=localhost
CLICKHOUSE_PORT=8123
CLICKHOUSE_USER=mlrunx
CLICKHOUSE_PASSWORD=mlrunx_dev

# MinIO
MINIO_ENDPOINT=http://localhost:9000
MINIO_ACCESS_KEY=mlrunx
MINIO_SECRET_KEY=mlrunx_dev_secret
```

## Scaling Guidelines

### Small (1-10 users, ~10 concurrent runs)
- Single API server (2 CPU, 4GB RAM)
- Single PostgreSQL (2 CPU, 4GB RAM)
- Single ClickHouse (4 CPU, 8GB RAM)
- MinIO or local S3-compatible storage
- Direct ingest mode

### Medium (10-100 users, ~100 concurrent runs)
- API server cluster (2-4 instances)
- PostgreSQL with connection pooling
- ClickHouse cluster (2-3 shards)
- Redis for session/caching
- Consider queued ingest mode

### Large (100+ users, ~1000+ concurrent runs)
- API server cluster with load balancer
- PostgreSQL with read replicas
- ClickHouse cluster with proper replication
- Kafka for ingest queue
- Queued ingest mode required
