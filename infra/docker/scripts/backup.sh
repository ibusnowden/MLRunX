#!/bin/bash
# =============================================================================
# MLRunX Backup Script
# Creates backups of PostgreSQL, ClickHouse, and MinIO data.
#
# Usage:
#   ./backup.sh                    # Backup to ./backups/<timestamp>/
#   ./backup.sh /path/to/backup    # Backup to specified directory
#
# See: docs/ops/local_backup.md for full documentation
# =============================================================================

set -euo pipefail

# Configuration
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DOCKER_DIR="$(dirname "$SCRIPT_DIR")"
BACKUP_BASE="${1:-${DOCKER_DIR}/backups}"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
BACKUP_DIR="${BACKUP_BASE}/${TIMESTAMP}"

# Container names (from docker-compose.yml)
POSTGRES_CONTAINER="mlrunx-postgres"
CLICKHOUSE_CONTAINER="mlrunx-clickhouse"
MINIO_CONTAINER="mlrunx-minio"

# Credentials (read from .env or use defaults)
if [ -f "${DOCKER_DIR}/.env" ]; then
    source "${DOCKER_DIR}/.env"
fi
POSTGRES_USER="${POSTGRES_USER:-mlrunx}"
POSTGRES_PASSWORD="${POSTGRES_PASSWORD:-mlrunx_dev}"
POSTGRES_DB="${POSTGRES_DATABASE:-mlrunx}"
CLICKHOUSE_USER="${CLICKHOUSE_USER:-mlrunx}"
CLICKHOUSE_PASSWORD="${CLICKHOUSE_PASSWORD:-mlrunx_dev}"
MINIO_ACCESS_KEY="${MINIO_ACCESS_KEY:-mlrunx}"
MINIO_SECRET_KEY="${MINIO_SECRET_KEY:-mlrunx_dev_secret}"
MINIO_BUCKET="${MINIO_BUCKET:-mlrunx-artifacts}"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Check if container is running
check_container() {
    local container=$1
    if ! docker ps --format '{{.Names}}' | grep -q "^${container}$"; then
        return 1
    fi
    return 0
}

# Create backup directory
mkdir -p "${BACKUP_DIR}"
log_info "Backup directory: ${BACKUP_DIR}"

# =============================================================================
# PostgreSQL Backup
# =============================================================================
log_info "Backing up PostgreSQL..."
if check_container "${POSTGRES_CONTAINER}"; then
    docker exec "${POSTGRES_CONTAINER}" pg_dump \
        -U "${POSTGRES_USER}" \
        -d "${POSTGRES_DB}" \
        --clean \
        --if-exists \
        > "${BACKUP_DIR}/postgres.sql"

    # Compress
    gzip "${BACKUP_DIR}/postgres.sql"
    log_info "PostgreSQL backup complete: postgres.sql.gz"
else
    log_warn "PostgreSQL container not running, skipping..."
fi

# =============================================================================
# ClickHouse Backup
# =============================================================================
log_info "Backing up ClickHouse..."
if check_container "${CLICKHOUSE_CONTAINER}"; then
    # Export metrics table
    docker exec "${CLICKHOUSE_CONTAINER}" clickhouse-client \
        --user "${CLICKHOUSE_USER}" \
        --password "${CLICKHOUSE_PASSWORD}" \
        --query "SELECT * FROM mlrunx.metrics FORMAT Native" \
        > "${BACKUP_DIR}/clickhouse_metrics.native" 2>/dev/null || true

    # Export system_metrics table (optional)
    docker exec "${CLICKHOUSE_CONTAINER}" clickhouse-client \
        --user "${CLICKHOUSE_USER}" \
        --password "${CLICKHOUSE_PASSWORD}" \
        --query "SELECT * FROM mlrunx.system_metrics FORMAT Native" \
        > "${BACKUP_DIR}/clickhouse_system_metrics.native" 2>/dev/null || true

    # Export schema (prefer repo migration file for full schema)
    CLICKHOUSE_SCHEMA_SRC="${DOCKER_DIR}/../../migrations/clickhouse/001_metrics_schema.sql"
    if [ -f "${CLICKHOUSE_SCHEMA_SRC}" ]; then
        cp "${CLICKHOUSE_SCHEMA_SRC}" "${BACKUP_DIR}/clickhouse_schema.sql"
    else
        docker exec "${CLICKHOUSE_CONTAINER}" clickhouse-client \
            --user "${CLICKHOUSE_USER}" \
            --password "${CLICKHOUSE_PASSWORD}" \
            --query "SHOW CREATE TABLE mlrunx.metrics" \
            > "${BACKUP_DIR}/clickhouse_schema.sql" 2>/dev/null || true
    fi

    # Compress
    gzip "${BACKUP_DIR}/clickhouse_metrics.native" 2>/dev/null || true
    gzip "${BACKUP_DIR}/clickhouse_system_metrics.native" 2>/dev/null || true

    log_info "ClickHouse backup complete"
else
    log_warn "ClickHouse container not running, skipping..."
fi

# =============================================================================
# MinIO Backup
# =============================================================================
log_info "Backing up MinIO..."
if check_container "${MINIO_CONTAINER}"; then
    MINIO_BACKUP_DIR="${BACKUP_DIR}/minio"
    mkdir -p "${MINIO_BACKUP_DIR}"

    # Use mc (MinIO client) to mirror the bucket
    docker run --rm \
        --network mlrunx-network \
        -v "${MINIO_BACKUP_DIR}:/backup" \
        minio/mc:latest \
        sh -c "mc alias set backup http://minio:9000 ${MINIO_ACCESS_KEY} ${MINIO_SECRET_KEY} && \
               mc mirror backup/${MINIO_BUCKET} /backup/ 2>/dev/null || echo 'Bucket may be empty'"

    # Create tar archive
    if [ -d "${MINIO_BACKUP_DIR}" ] && [ "$(ls -A ${MINIO_BACKUP_DIR})" ]; then
        tar -czf "${BACKUP_DIR}/minio.tar.gz" -C "${MINIO_BACKUP_DIR}" .
        rm -rf "${MINIO_BACKUP_DIR}"
        log_info "MinIO backup complete: minio.tar.gz"
    else
        rm -rf "${MINIO_BACKUP_DIR}"
        log_warn "MinIO bucket is empty, skipping..."
    fi
else
    log_warn "MinIO container not running, skipping..."
fi

# =============================================================================
# Create Backup Manifest
# =============================================================================
cat > "${BACKUP_DIR}/manifest.json" << EOF
{
    "timestamp": "${TIMESTAMP}",
    "created_at": "$(date -Iseconds)",
    "version": "1.0",
    "components": {
        "postgres": $([ -f "${BACKUP_DIR}/postgres.sql.gz" ] && echo "true" || echo "false"),
        "clickhouse": $([ -f "${BACKUP_DIR}/clickhouse_metrics.native.gz" ] && echo "true" || echo "false"),
        "minio": $([ -f "${BACKUP_DIR}/minio.tar.gz" ] && echo "true" || echo "false")
    },
    "files": [
        $(ls -1 "${BACKUP_DIR}" | grep -v manifest.json | sed 's/.*/"&"/' | tr '\n' ',' | sed 's/,$//')
    ]
}
EOF

# =============================================================================
# Summary
# =============================================================================
echo ""
log_info "Backup complete!"
echo ""
echo "Location: ${BACKUP_DIR}"
echo ""
echo "Files:"
ls -lh "${BACKUP_DIR}"
echo ""

# Calculate total size
TOTAL_SIZE=$(du -sh "${BACKUP_DIR}" | cut -f1)
log_info "Total backup size: ${TOTAL_SIZE}"

echo ""
echo "To restore, run:"
echo "  ./restore.sh ${BACKUP_DIR}"
