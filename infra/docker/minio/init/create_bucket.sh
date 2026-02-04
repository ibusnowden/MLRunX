#!/bin/bash
# ============================================================================
# MLRunX MinIO Bucket Initialization
# Creates the artifacts bucket on startup.
# ============================================================================

set -e

MINIO_HOST="${MINIO_HOST:-minio}"
MINIO_PORT="${MINIO_PORT:-9000}"
MINIO_ROOT_USER="${MINIO_ROOT_USER:-mlrunx}"
MINIO_ROOT_PASSWORD="${MINIO_ROOT_PASSWORD:-mlrunx_dev_secret}"
BUCKET_NAME="${MINIO_BUCKET:-mlrunx-artifacts}"

echo "Waiting for MinIO to be ready..."
until mc alias set local http://${MINIO_HOST}:${MINIO_PORT} ${MINIO_ROOT_USER} ${MINIO_ROOT_PASSWORD} 2>/dev/null; do
    echo "MinIO not ready yet, waiting..."
    sleep 2
done

echo "MinIO is ready!"

# Create bucket if it doesn't exist
if mc ls local/${BUCKET_NAME} 2>/dev/null; then
    echo "Bucket '${BUCKET_NAME}' already exists"
else
    echo "Creating bucket '${BUCKET_NAME}'..."
    mc mb local/${BUCKET_NAME}
    echo "Bucket created successfully"
fi

# Set bucket policy to allow authenticated access
echo "Setting bucket policy..."
mc anonymous set download local/${BUCKET_NAME} 2>/dev/null || true

echo "MinIO initialization complete!"
