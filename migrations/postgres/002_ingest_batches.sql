-- ============================================================================
-- MLRunX PostgreSQL Schema: Ingest Batches (Idempotency)
-- Migration: 002_ingest_batches.sql
-- ============================================================================
-- Tracks ingested batches for idempotency and deduplication.
-- Ensures SDK retries don't create duplicate data.
-- ============================================================================

-- ============================================================================
-- Ingest Batches Table
-- ============================================================================
-- Records each batch received from SDKs for idempotency checking.

CREATE TABLE IF NOT EXISTS ingest_batches (
    id              BIGSERIAL PRIMARY KEY,

    -- Batch identification
    project_id      UUID NOT NULL,
    run_id          UUID NOT NULL,
    batch_id        VARCHAR(64) NOT NULL,  -- SDK-provided batch identifier

    -- Sequencing
    seq             BIGINT NOT NULL DEFAULT 0,  -- Sequence number within run

    -- Idempotency
    payload_hash    VARCHAR(64) NOT NULL,  -- SHA-256 hash of batch payload

    -- Stats
    metric_count    INTEGER NOT NULL DEFAULT 0,
    param_count     INTEGER NOT NULL DEFAULT 0,
    tag_count       INTEGER NOT NULL DEFAULT 0,

    -- Timestamps
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Unique constraint: one batch_id per run
    UNIQUE (run_id, batch_id)
);

-- Performance indexes
CREATE INDEX idx_ingest_batches_run ON ingest_batches(run_id, seq);
CREATE INDEX idx_ingest_batches_project ON ingest_batches(project_id, created_at DESC);
CREATE INDEX idx_ingest_batches_hash ON ingest_batches(run_id, payload_hash);

-- ============================================================================
-- Comments
-- ============================================================================

COMMENT ON TABLE ingest_batches IS
'Tracks ingested batches for idempotency. Each batch_id per run is unique.';

COMMENT ON COLUMN ingest_batches.batch_id IS
'SDK-provided unique identifier for the batch within a run.';

COMMENT ON COLUMN ingest_batches.seq IS
'Sequence number for ordering batches. May have gaps due to offline spool.';

COMMENT ON COLUMN ingest_batches.payload_hash IS
'SHA-256 hash of the batch payload for detecting conflicting retries.';
