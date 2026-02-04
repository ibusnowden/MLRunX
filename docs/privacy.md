# MLRunX Privacy Policy

MLRunX is designed with **privacy-first defaults**. Your experiment data stays on your infrastructure, and no information is sent to external services unless you explicitly enable it.

## Core Privacy Principles

1. **Local-First**: All data is stored locally on your infrastructure
2. **No Outbound Calls**: No network requests to external services by default
3. **Opt-In Telemetry**: Any telemetry features are disabled by default
4. **Data Ownership**: You own and control all your experiment data

## Data Storage Locations

When running the default Docker Compose stack, your data is stored in the following Docker volumes:

| Data Type | Storage | Volume | Description |
|-----------|---------|--------|-------------|
| Metrics | ClickHouse | `clickhouse-data` | Time-series training metrics (loss, accuracy, etc.) |
| Metadata | PostgreSQL | `postgres-data` | Projects, runs, parameters, API keys |
| Artifacts | MinIO | `minio-data` | Model files, datasets, checkpoints |
| Cache | Redis | `redis-data` | Temporary cache data |

### Default Storage Paths

On a typical Docker installation:
- Linux: `/var/lib/docker/volumes/`
- macOS: `~/Library/Containers/com.docker.docker/Data/vms/0/data/docker/volumes/`
- Windows: `\\wsl$\docker-desktop-data\data\docker\volumes\`

## Data Retention

MLRunX currently enforces metrics retention via ClickHouse TTL (90 days by default). Other retention settings are planned; until then, artifacts and logs are kept until manually deleted.

| Data Type | Default Retention | Configuration |
|-----------|------------------|---------------|
| Metrics | 90 days | ClickHouse schema TTL |
| Artifacts | Manual deletion | N/A (planned) |
| System Logs | Manual deletion | N/A (planned) |
| Metadata | Manual deletion | N/A |

## Telemetry Configuration

All telemetry is **disabled by default**. The following toggles are available:

```bash
# All disabled by default (true = disabled)
MLRUNX_TELEMETRY_DISABLED=true
MLRUNX_CRASH_REPORTING_DISABLED=true
MLRUNX_ANALYTICS_DISABLED=true
MLRUNX_UPDATE_CHECK_DISABLED=true

# Next.js telemetry is also disabled
NEXT_TELEMETRY_DISABLED=1
```

### What Would Telemetry Collect?

If you choose to enable telemetry in the future (to help improve MLRunX), it would only collect:
- Aggregate usage statistics (run counts, metric counts)
- Error reports (stack traces without experiment data)
- Feature usage patterns

Telemetry would **never** collect:
- Experiment data (metrics, parameters, artifacts)
- Model weights or training data
- API keys or credentials
- Project names or custom metadata

## Network Isolation

The default Docker Compose setup uses an isolated bridge network (`mlrunx-network`). Services communicate internally and expose the following ports on the host by default:

| Port | Service | Access |
|------|---------|--------|
| 3000 | UI | `http://localhost:3000` |
| 3001 | API (HTTP) | `http://localhost:3001` |
| 50051 | API (gRPC) | `localhost:50051` |
| 8123 | ClickHouse | `localhost:8123` |
| 5432 | PostgreSQL | `localhost:5432` |
| 9001 | MinIO API | `localhost:9001` |
| 9002 | MinIO Console | `localhost:9002` |
| 6379 | Redis | `localhost:6379` |

To restrict access further, modify the port bindings in `docker-compose.yml` to bind only to `127.0.0.1`:

```yaml
ports:
  - "127.0.0.1:3001:3001"  # Only accessible from localhost
```

## SDK Privacy

The Python SDK is designed to work completely offline:

```python
import mlrunx

# Configure for offline mode
mlrunx.configure(
    server_url="http://localhost:3001",  # Local server
    offline_mode=True,  # Enable offline spool if server unavailable
)
```

When `offline_mode=True`:
- Data is spooled locally if the server is unreachable
- No network errors interrupt your training
- Data syncs when the server becomes available

## Data Export

You can export all your data at any time:

```bash
# PostgreSQL metadata
docker exec mlrunx-postgres pg_dump -U mlrunx mlrunx > backup.sql

# ClickHouse metrics
docker exec mlrunx-clickhouse clickhouse-client \
  --user mlrunx --password mlrunx_dev \
  --query "SELECT * FROM mlrunx.metrics FORMAT JSONEachRow" > metrics.json

# MinIO artifacts
docker run --rm --network mlrunx-network \
  minio/mc mc mirror minio/mlrunx-artifacts ./artifacts/
```

## Data Deletion

To completely remove all MLRunX data:

```bash
# Stop all services
docker compose down

# Remove all volumes (DESTRUCTIVE!)
docker compose down -v

# Or selectively remove specific volumes
docker volume rm mlrunx_clickhouse-data
docker volume rm mlrunx_postgres-data
docker volume rm mlrunx_minio-data
```

## Security Recommendations

1. **Change default passwords** before deploying:
   ```bash
   CLICKHOUSE_PASSWORD=<secure-password>
   POSTGRES_PASSWORD=<secure-password>
   MINIO_SECRET_KEY=<secure-password>
   ```

2. **Generate a secure API key**:
   ```bash
   MLRUNX_API_KEY=$(openssl rand -hex 32)
   ```

3. **Disable dev mode in production**:
   ```bash
   MLRUNX_AUTH_DISABLED=false
   ```

4. **Use network isolation** in production deployments

5. **Enable TLS** for external access (see deployment docs)

## Questions?

If you have privacy concerns or questions, please:
- Open an issue on GitHub
- Review our source code (fully open source)
- Run MLRunX in an air-gapped environment if needed
