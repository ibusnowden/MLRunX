# Storage Defaults and Resource Caps

This document describes the default storage configuration for MLRunX and how to tune it for different environments.

## Overview

MLRunX uses three storage backends:
- **ClickHouse**: Time-series metrics
- **PostgreSQL**: Relational metadata
- **MinIO/S3**: Binary artifacts

Default settings are optimized for local development on a machine with 4GB+ RAM.

## ClickHouse Defaults

### Memory Limits

| Setting | Default | Description |
|---------|---------|-------------|
| `max_server_memory_usage` | 2GB | Total memory ClickHouse can use |
| `max_memory_usage` | 1GB | Per-query memory limit |
| `mark_cache_size` | 500MB | Cache for index marks |
| `uncompressed_cache_size` | 256MB | Cache for uncompressed blocks |

### Thread Limits

| Setting | Default | Description |
|---------|---------|-------------|
| `max_threads` | 4 | Max threads per query |
| `max_insert_threads` | 2 | Threads for insert operations |
| `background_pool_size` | 4 | Background merge threads |

### Connection Limits

| Setting | Default | Description |
|---------|---------|-------------|
| `max_connections` | 100 | Maximum client connections |
| `max_concurrent_queries` | 20 | Parallel queries allowed |

### Retention

Default TTL in metrics schema: **90 days**

To change retention, modify the `metrics` table:

```sql
ALTER TABLE mlrunx.metrics
    MODIFY TTL timestamp + INTERVAL 30 DAY;
```

## PostgreSQL Defaults

### Connection Pool

| Setting | Default | Description |
|---------|---------|-------------|
| `max_connections` | 100 | Maximum connections |
| `shared_buffers` | 128MB | Shared memory for caching |
| `work_mem` | 4MB | Per-operation memory |

### For Production

```bash
# In postgresql.conf or via environment
POSTGRES_SHARED_BUFFERS=256MB
POSTGRES_WORK_MEM=8MB
POSTGRES_MAX_CONNECTIONS=200
```

## MinIO Defaults

### Bucket Configuration

| Setting | Default | Description |
|---------|---------|-------------|
| `MINIO_BUCKET` | mlrunx-artifacts | Default bucket name |
| `MINIO_PRESIGN_EXPIRY` | 3600 | Presigned URL expiry (seconds) |

### Storage Organization

Artifacts are stored as:
```
mlrunx-artifacts/
  runs/{run_id}/
    {artifact_name}
```

## Docker Volume Recommendations

### Development (4GB RAM)

```yaml
services:
  clickhouse:
    deploy:
      resources:
        limits:
          memory: 2G

  postgres:
    deploy:
      resources:
        limits:
          memory: 512M

  minio:
    deploy:
      resources:
        limits:
          memory: 256M
```

### Disk Space

| Component | Minimum | Recommended |
|-----------|---------|-------------|
| ClickHouse | 10GB | 50GB |
| PostgreSQL | 1GB | 5GB |
| MinIO | 10GB | 100GB |

## Environment Variables

### ClickHouse

```bash
# Override in docker-compose or .env
CLICKHOUSE_MAX_SERVER_MEMORY_USAGE=4294967296  # 4GB
CLICKHOUSE_MAX_THREADS=8
```

### PostgreSQL

```bash
POSTGRES_SHARED_BUFFERS=256MB
POSTGRES_WORK_MEM=8MB
```

### API Server Cardinality Limits

```bash
MLRUNX_MAX_TAG_KEYS_PER_RUN=100
MLRUNX_MAX_METRIC_NAMES_PER_RUN=1000
MLRUNX_MAX_TAGS_PER_PROJECT=10000
```

## Tuning for Production

### Small Production (10 users, ~100 runs/day)

```yaml
# ClickHouse
max_server_memory_usage: 4GB
max_concurrent_queries: 50
max_threads: 8

# PostgreSQL
shared_buffers: 512MB
max_connections: 200
```

### Medium Production (100 users, ~1000 runs/day)

```yaml
# ClickHouse
max_server_memory_usage: 16GB
max_concurrent_queries: 100
max_threads: 16

# PostgreSQL
shared_buffers: 2GB
max_connections: 500
```

## Monitoring Disk Usage

### ClickHouse

```sql
-- Check table sizes
SELECT
    database,
    table,
    formatReadableSize(sum(data_compressed_bytes)) as compressed,
    formatReadableSize(sum(data_uncompressed_bytes)) as uncompressed
FROM system.parts
WHERE database = 'mlrunx'
GROUP BY database, table
ORDER BY sum(data_compressed_bytes) DESC;
```

### PostgreSQL

```sql
-- Check database size
SELECT pg_size_pretty(pg_database_size('mlrunx'));

-- Check table sizes
SELECT
    tablename,
    pg_size_pretty(pg_total_relation_size(schemaname || '.' || tablename))
FROM pg_tables
WHERE schemaname = 'public'
ORDER BY pg_total_relation_size(schemaname || '.' || tablename) DESC;
```

## Troubleshooting

### ClickHouse Out of Memory

1. Reduce `max_memory_usage` per query
2. Reduce `max_concurrent_queries`
3. Add more RAM or use distributed setup

### PostgreSQL Connection Exhausted

1. Increase `max_connections`
2. Use connection pooling (PgBouncer)
3. Check for connection leaks in application

### MinIO Disk Full

1. Implement artifact retention policy
2. Move to larger storage
3. Use S3 lifecycle rules for cleanup
