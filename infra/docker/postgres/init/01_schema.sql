-- ============================================================================
-- MLRunX PostgreSQL Initialization
-- This script runs on first container startup.
-- ============================================================================

-- Enable UUID extension
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- ============================================================================
-- Projects Table
-- ============================================================================

CREATE TABLE IF NOT EXISTS projects (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name            VARCHAR(256) NOT NULL UNIQUE,
    description     TEXT,
    owner_id        UUID,
    settings        JSONB DEFAULT '{}',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted_at      TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_projects_name ON projects(name) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_projects_owner ON projects(owner_id) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_projects_created ON projects(created_at DESC) WHERE deleted_at IS NULL;

-- ============================================================================
-- Runs Table
-- ============================================================================

DO $$ BEGIN
    CREATE TYPE run_status AS ENUM (
        'pending',
        'running',
        'finished',
        'failed',
        'killed'
    );
EXCEPTION
    WHEN duplicate_object THEN null;
END $$;

CREATE TABLE IF NOT EXISTS runs (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    project_id      UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name            VARCHAR(256),
    description     TEXT,
    status          run_status NOT NULL DEFAULT 'pending',
    exit_code       INTEGER,
    error_message   TEXT,
    parent_run_id   UUID REFERENCES runs(id) ON DELETE SET NULL,
    resume_token    VARCHAR(256),
    tags            JSONB DEFAULT '{}',
    system_info     JSONB DEFAULT '{}',
    git_info        JSONB DEFAULT '{}',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    started_at      TIMESTAMPTZ,
    finished_at     TIMESTAMPTZ,
    deleted_at      TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_runs_project ON runs(project_id, created_at DESC) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_runs_status ON runs(status) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_runs_parent ON runs(parent_run_id) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_runs_created ON runs(created_at DESC) WHERE deleted_at IS NULL;

-- ============================================================================
-- Parameters Table
-- ============================================================================

CREATE TABLE IF NOT EXISTS parameters (
    id          UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    run_id      UUID NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    name        VARCHAR(256) NOT NULL,
    value       TEXT NOT NULL,
    value_type  VARCHAR(32) DEFAULT 'string',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_parameters_run ON parameters(run_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_parameters_run_name ON parameters(run_id, name);

-- ============================================================================
-- Artifacts Table
-- ============================================================================

CREATE TABLE IF NOT EXISTS artifacts (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    run_id          UUID NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    name            VARCHAR(512) NOT NULL,
    artifact_type   VARCHAR(64) NOT NULL DEFAULT 'file',
    storage_path    VARCHAR(1024) NOT NULL,
    size_bytes      BIGINT,
    checksum        VARCHAR(128),
    metadata        JSONB DEFAULT '{}',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_artifacts_run ON artifacts(run_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_artifacts_run_name ON artifacts(run_id, name);

-- ============================================================================
-- API Keys Table
-- ============================================================================

CREATE TABLE IF NOT EXISTS api_keys (
    id          UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    key_hash    VARCHAR(128) NOT NULL UNIQUE,
    name        VARCHAR(256),
    project_id  UUID REFERENCES projects(id) ON DELETE CASCADE,
    scopes      JSONB DEFAULT '["read", "write"]',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at  TIMESTAMPTZ,
    last_used   TIMESTAMPTZ,
    revoked_at  TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_api_keys_project ON api_keys(project_id) WHERE revoked_at IS NULL;

-- ============================================================================
-- Ingest Batches Table (for idempotency)
-- ============================================================================

CREATE TABLE IF NOT EXISTS ingest_batches (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    project_id      UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    run_id          UUID NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    batch_id        VARCHAR(256) NOT NULL,
    sequence_num    BIGINT NOT NULL DEFAULT 0,
    payload_hash    VARCHAR(128) NOT NULL,
    metric_count    INTEGER NOT NULL DEFAULT 0,
    param_count     INTEGER NOT NULL DEFAULT 0,
    tag_count       INTEGER NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_batches_unique ON ingest_batches(run_id, batch_id);
CREATE INDEX IF NOT EXISTS idx_batches_run ON ingest_batches(run_id, sequence_num);
