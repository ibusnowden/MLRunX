-- ============================================================================
-- MLRunX PostgreSQL Schema: Metadata
-- Migration: 001_metadata_schema.sql
-- ============================================================================
-- Stores structured metadata for projects, runs, artifacts, and parameters.
-- Designed for relational queries, ACID guarantees, and rich indexing.
-- ============================================================================

-- Enable UUID extension
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- ============================================================================
-- Projects Table
-- ============================================================================
-- Projects are the top-level organizational unit for ML experiments.

CREATE TABLE IF NOT EXISTS projects (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name            VARCHAR(256) NOT NULL UNIQUE,
    description     TEXT,

    -- Ownership
    owner_id        UUID,

    -- Settings (JSON for flexibility)
    settings        JSONB DEFAULT '{}',

    -- Timestamps
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Soft delete
    deleted_at      TIMESTAMPTZ
);

CREATE INDEX idx_projects_name ON projects(name) WHERE deleted_at IS NULL;
CREATE INDEX idx_projects_owner ON projects(owner_id) WHERE deleted_at IS NULL;
CREATE INDEX idx_projects_created ON projects(created_at DESC) WHERE deleted_at IS NULL;

-- ============================================================================
-- Runs Table
-- ============================================================================
-- Runs represent individual ML training experiments.

CREATE TYPE run_status AS ENUM (
    'pending',
    'running',
    'finished',
    'failed',
    'killed'
);

CREATE TABLE IF NOT EXISTS runs (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    project_id      UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,

    -- Run identification
    name            VARCHAR(256),
    description     TEXT,

    -- Status tracking
    status          run_status NOT NULL DEFAULT 'pending',
    exit_code       INTEGER,
    error_message   TEXT,

    -- Parent run for nested runs (hyperparameter sweeps, etc.)
    parent_run_id   UUID REFERENCES runs(id) ON DELETE SET NULL,

    -- Resume support
    resume_token    VARCHAR(256),

    -- Tags (key-value pairs)
    tags            JSONB DEFAULT '{}',

    -- System information (SDK auto-captured)
    system_info     JSONB DEFAULT '{}',

    -- Git information
    git_info        JSONB DEFAULT '{}',

    -- Timestamps
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    started_at      TIMESTAMPTZ,
    finished_at     TIMESTAMPTZ,

    -- Computed duration (in seconds)
    duration_seconds DOUBLE PRECISION GENERATED ALWAYS AS (
        EXTRACT(EPOCH FROM (COALESCE(finished_at, NOW()) - COALESCE(started_at, created_at)))
    ) STORED,

    -- Soft delete
    deleted_at      TIMESTAMPTZ
);

-- Performance indexes
CREATE INDEX idx_runs_project ON runs(project_id, created_at DESC) WHERE deleted_at IS NULL;
CREATE INDEX idx_runs_status ON runs(status) WHERE deleted_at IS NULL;
CREATE INDEX idx_runs_parent ON runs(parent_run_id) WHERE deleted_at IS NULL;
CREATE INDEX idx_runs_created ON runs(created_at DESC) WHERE deleted_at IS NULL;
CREATE INDEX idx_runs_tags ON runs USING GIN(tags) WHERE deleted_at IS NULL;

-- ============================================================================
-- Parameters Table
-- ============================================================================
-- Hyperparameters and configuration values for runs.

CREATE TABLE IF NOT EXISTS parameters (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    run_id          UUID NOT NULL REFERENCES runs(id) ON DELETE CASCADE,

    -- Parameter identification
    name            VARCHAR(256) NOT NULL,

    -- Value storage (supports multiple types)
    value_string    TEXT,
    value_float     DOUBLE PRECISION,
    value_int       BIGINT,
    value_bool      BOOLEAN,
    value_json      JSONB,

    -- Type indicator
    value_type      VARCHAR(32) NOT NULL DEFAULT 'string',

    -- Timestamps
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Unique constraint per run
    UNIQUE (run_id, name)
);

CREATE INDEX idx_params_run ON parameters(run_id);
CREATE INDEX idx_params_name ON parameters(name);

-- ============================================================================
-- Artifacts Table
-- ============================================================================
-- Files and objects produced by runs (models, datasets, etc.).

CREATE TYPE artifact_type AS ENUM (
    'model',
    'dataset',
    'plot',
    'table',
    'file',
    'directory',
    'other'
);

CREATE TABLE IF NOT EXISTS artifacts (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    run_id          UUID NOT NULL REFERENCES runs(id) ON DELETE CASCADE,

    -- Artifact identification
    name            VARCHAR(256) NOT NULL,
    type            artifact_type NOT NULL DEFAULT 'file',
    description     TEXT,

    -- Storage location
    storage_path    VARCHAR(1024) NOT NULL,  -- e.g., s3://bucket/path or minio://bucket/path
    storage_type    VARCHAR(32) NOT NULL DEFAULT 'minio',  -- minio, s3, gcs, local

    -- File metadata
    size_bytes      BIGINT,
    mime_type       VARCHAR(256),
    checksum_md5    VARCHAR(32),
    checksum_sha256 VARCHAR(64),

    -- Additional metadata
    metadata        JSONB DEFAULT '{}',

    -- Timestamps
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Unique constraint per run
    UNIQUE (run_id, name)
);

CREATE INDEX idx_artifacts_run ON artifacts(run_id);
CREATE INDEX idx_artifacts_type ON artifacts(type);
CREATE INDEX idx_artifacts_created ON artifacts(created_at DESC);

-- ============================================================================
-- Run Summaries Table
-- ============================================================================
-- Quick-access summary statistics for runs (synced from ClickHouse).

CREATE TABLE IF NOT EXISTS run_summaries (
    run_id          UUID PRIMARY KEY REFERENCES runs(id) ON DELETE CASCADE,

    -- Counts
    total_metrics   BIGINT DEFAULT 0,
    total_params    INTEGER DEFAULT 0,
    total_artifacts INTEGER DEFAULT 0,

    -- Best metric values (for leaderboards)
    best_metrics    JSONB DEFAULT '{}',

    -- Last metric values (for quick display)
    last_metrics    JSONB DEFAULT '{}',

    -- Timestamps
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ============================================================================
-- API Keys Table (for authentication)
-- ============================================================================

CREATE TABLE IF NOT EXISTS api_keys (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),

    -- Key data
    key_hash        VARCHAR(128) NOT NULL UNIQUE,  -- SHA-256 hash of the API key
    key_prefix      VARCHAR(8) NOT NULL,           -- First 8 chars for identification

    -- Ownership
    project_id      UUID REFERENCES projects(id) ON DELETE CASCADE,

    -- Metadata
    name            VARCHAR(256),
    description     TEXT,

    -- Permissions
    scopes          VARCHAR(256)[] DEFAULT '{}',

    -- Rate limiting
    rate_limit      INTEGER DEFAULT 1000,  -- requests per minute

    -- Timestamps
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_used_at    TIMESTAMPTZ,
    expires_at      TIMESTAMPTZ,

    -- Soft delete
    revoked_at      TIMESTAMPTZ
);

CREATE INDEX idx_api_keys_hash ON api_keys(key_hash) WHERE revoked_at IS NULL;
CREATE INDEX idx_api_keys_project ON api_keys(project_id) WHERE revoked_at IS NULL;

-- ============================================================================
-- Functions and Triggers
-- ============================================================================

-- Auto-update updated_at timestamp
CREATE OR REPLACE FUNCTION update_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER projects_updated_at
    BEFORE UPDATE ON projects
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at();

CREATE TRIGGER runs_updated_at
    BEFORE UPDATE ON runs
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at();

-- Auto-set started_at when run starts
CREATE OR REPLACE FUNCTION set_run_started_at()
RETURNS TRIGGER AS $$
BEGIN
    IF OLD.status = 'pending' AND NEW.status = 'running' AND NEW.started_at IS NULL THEN
        NEW.started_at = NOW();
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER runs_started_at
    BEFORE UPDATE ON runs
    FOR EACH ROW
    EXECUTE FUNCTION set_run_started_at();

-- Auto-set finished_at when run ends
CREATE OR REPLACE FUNCTION set_run_finished_at()
RETURNS TRIGGER AS $$
BEGIN
    IF OLD.status = 'running' AND NEW.status IN ('finished', 'failed', 'killed') AND NEW.finished_at IS NULL THEN
        NEW.finished_at = NOW();
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER runs_finished_at
    BEFORE UPDATE ON runs
    FOR EACH ROW
    EXECUTE FUNCTION set_run_finished_at();
