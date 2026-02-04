#!/bin/bash
# =============================================================================
# MLRunX Restore Script
# Restores PostgreSQL, ClickHouse, and MinIO data from backup.
#
# Usage:
#   ./restore.sh /path/to/backup/20240101_120000
#
# WARNING: This will OVERWRITE existing data!
#
# See: docs/ops/local_backup.md for full documentation
# =============================================================================

set -euo pipefail

# Configuration
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DOCKER_DIR="$(dirname "$SCRIPT_DIR")"
BACKUP_DIR="${1:-}"

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

# =============================================================================
# Validation
# =============================================================================
if [ -z "${BACKUP_DIR}" ]; then
    log_error "Usage: ./restore.sh /path/to/backup"
    exit 1
fi

if [ ! -d "${BACKUP_DIR}" ]; then
    log_error "Backup directory not found: ${BACKUP_DIR}"
    exit 1
fi

if [ ! -f "${BACKUP_DIR}/manifest.json" ]; then
    log_warn "No manifest.json found, proceeding anyway..."
fi

echo ""
echo "╔═══════════════════════════════════════════════════════════════╗"
echo "║                     MLRunX Restore                             ║"
echo "╠═══════════════════════════════════════════════════════════════╣"
echo "║  WARNING: This will OVERWRITE existing data!                  ║"
echo "║                                                               ║"
echo "║  It is recommended to STOP the stack before restoring:        ║"
echo "║    docker compose stop api ui                                 ║"
echo "╚═══════════════════════════════════════════════════════════════╝"
echo ""
echo "Backup: ${BACKUP_DIR}"
echo ""

# Show backup contents
echo "Backup contents:"
ls -lh "${BACKUP_DIR}"
echo ""

read -p "Continue with restore? [y/N] " -n 1 -r
echo ""
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    log_info "Restore cancelled."
    exit 0
fi

echo ""

# =============================================================================
# PostgreSQL Restore
# =============================================================================
if [ -f "${BACKUP_DIR}/postgres.sql.gz" ]; then
    log_info "Restoring PostgreSQL..."
    if check_container "${POSTGRES_CONTAINER}"; then
        # Decompress and restore
        gunzip -c "${BACKUP_DIR}/postgres.sql.gz" | \
            docker exec -i "${POSTGRES_CONTAINER}" psql \
                -U "${POSTGRES_USER}" \
                -d "${POSTGRES_DB}" \
                --quiet

        log_info "PostgreSQL restore complete"
    else
        log_error "PostgreSQL container not running!"
        log_info "Start it with: docker compose up -d postgres"
    fi
else
    log_warn "No PostgreSQL backup found, skipping..."
fi

# =============================================================================
# ClickHouse Restore
# =============================================================================
if [ -f "${BACKUP_DIR}/clickhouse_metrics.native.gz" ]; then
    log_info "Restoring ClickHouse..."
    if check_container "${CLICKHOUSE_CONTAINER}"; then
        # Restore schema first (if exists)
        if [ -f "${BACKUP_DIR}/clickhouse_schema.sql" ]; then
            cat "${BACKUP_DIR}/clickhouse_schema.sql" | \
                docker exec -i "${CLICKHOUSE_CONTAINER}" clickhouse-client \
                    --user "${CLICKHOUSE_USER}" \
                    --password "${CLICKHOUSE_PASSWORD}" \
                    --multiquery 2>/dev/null || true
        fi

        # Clear existing data
        docker exec "${CLICKHOUSE_CONTAINER}" clickhouse-client \
            --user "${CLICKHOUSE_USER}" \
            --password "${CLICKHOUSE_PASSWORD}" \
            --query "TRUNCATE TABLE IF EXISTS mlrunx.metrics" 2>/dev/null || true

        docker exec "${CLICKHOUSE_CONTAINER}" clickhouse-client \
            --user "${CLICKHOUSE_USER}" \
            --password "${CLICKHOUSE_PASSWORD}" \
            --query "TRUNCATE TABLE IF EXISTS mlrunx.metrics_summary" 2>/dev/null || true

        docker exec "${CLICKHOUSE_CONTAINER}" clickhouse-client \
            --user "${CLICKHOUSE_USER}" \
            --password "${CLICKHOUSE_PASSWORD}" \
            --query "TRUNCATE TABLE IF EXISTS mlrunx.run_metrics_count" 2>/dev/null || true

        docker exec "${CLICKHOUSE_CONTAINER}" clickhouse-client \
            --user "${CLICKHOUSE_USER}" \
            --password "${CLICKHOUSE_PASSWORD}" \
            --query "TRUNCATE TABLE IF EXISTS mlrunx.system_metrics" 2>/dev/null || true

        # Restore metrics
        gunzip -c "${BACKUP_DIR}/clickhouse_metrics.native.gz" | \
            docker exec -i "${CLICKHOUSE_CONTAINER}" clickhouse-client \
                --user "${CLICKHOUSE_USER}" \
                --password "${CLICKHOUSE_PASSWORD}" \
                --query "INSERT INTO mlrunx.metrics FORMAT Native" 2>/dev/null || true

        # Restore system_metrics (optional)
        if [ -f "${BACKUP_DIR}/clickhouse_system_metrics.native.gz" ]; then
            gunzip -c "${BACKUP_DIR}/clickhouse_system_metrics.native.gz" | \
                docker exec -i "${CLICKHOUSE_CONTAINER}" clickhouse-client \
                    --user "${CLICKHOUSE_USER}" \
                    --password "${CLICKHOUSE_PASSWORD}" \
                    --query "INSERT INTO mlrunx.system_metrics FORMAT Native" 2>/dev/null || true
        fi

        log_info "ClickHouse restore complete"
    else
        log_error "ClickHouse container not running!"
        log_info "Start it with: docker compose up -d clickhouse"
    fi
else
    log_warn "No ClickHouse backup found, skipping..."
fi

# =============================================================================
# MinIO Restore
# =============================================================================
if [ -f "${BACKUP_DIR}/minio.tar.gz" ]; then
    log_info "Restoring MinIO..."
    if check_container "${MINIO_CONTAINER}"; then
        # Create temp directory for extraction
        MINIO_RESTORE_DIR=$(mktemp -d)

        # Extract archive
        tar -xzf "${BACKUP_DIR}/minio.tar.gz" -C "${MINIO_RESTORE_DIR}"

        # Use mc to mirror back to MinIO
        docker run --rm \
            --network mlrunx-network \
            -v "${MINIO_RESTORE_DIR}:/backup:ro" \
            minio/mc:latest \
            sh -c "mc alias set restore http://minio:9000 ${MINIO_ACCESS_KEY} ${MINIO_SECRET_KEY} && \
                   mc mb -p restore/${MINIO_BUCKET} 2>/dev/null || true && \
                   mc mirror /backup/ restore/${MINIO_BUCKET}/"

        # Cleanup
        rm -rf "${MINIO_RESTORE_DIR}"

        log_info "MinIO restore complete"
    else
        log_error "MinIO container not running!"
        log_info "Start it with: docker compose up -d minio"
    fi
else
    log_warn "No MinIO backup found, skipping..."
fi

# =============================================================================
# Summary
# =============================================================================
echo ""
log_info "Restore complete!"
echo ""
echo "Next steps:"
echo "  1. Restart the application services:"
echo "     docker compose restart api ui"
echo ""
echo "  2. Verify data in the UI:"
echo "     http://localhost:3000"
echo ""
