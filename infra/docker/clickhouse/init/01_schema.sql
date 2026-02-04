-- ============================================================================
-- MLRunX ClickHouse Schema
-- Time-series metrics storage.
-- ============================================================================

-- Create the mlrunx database
CREATE DATABASE IF NOT EXISTS mlrunx;

-- ============================================================================
-- Metrics Table
-- ============================================================================

CREATE TABLE IF NOT EXISTS mlrunx.metrics
(
    run_id       String,
    project_id   String,
    name         String,
    step         Int64,
    value        Float64,
    timestamp    DateTime64(3) DEFAULT now64(3),
    ingested_at  DateTime64(3) DEFAULT now64(3),
    batch_id     String DEFAULT '',

    INDEX idx_project project_id TYPE bloom_filter GRANULARITY 4,
    INDEX idx_name name TYPE bloom_filter GRANULARITY 4
)
ENGINE = ReplacingMergeTree(ingested_at)
PARTITION BY toYYYYMM(timestamp)
ORDER BY (run_id, name, step, timestamp)
TTL timestamp + INTERVAL 90 DAY
SETTINGS index_granularity = 8192;

-- ============================================================================
-- Metrics Summary Table
-- ============================================================================

CREATE TABLE IF NOT EXISTS mlrunx.metrics_summary
(
    run_id       String,
    project_id   String,
    name         String,
    min_value    Float64,
    max_value    Float64,
    last_value   Float64,
    last_step    Int64,
    count        UInt64,
    first_at     DateTime64(3),
    last_at      DateTime64(3),
    updated_at   DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(updated_at)
ORDER BY (run_id, name)
SETTINGS index_granularity = 8192;

-- Materialized view for auto-summary
CREATE MATERIALIZED VIEW IF NOT EXISTS mlrunx.metrics_summary_mv
TO mlrunx.metrics_summary
AS SELECT
    run_id,
    project_id,
    name,
    min(value) AS min_value,
    max(value) AS max_value,
    argMax(value, step) AS last_value,
    max(step) AS last_step,
    count() AS count,
    min(timestamp) AS first_at,
    max(timestamp) AS last_at,
    now64(3) AS updated_at
FROM mlrunx.metrics
GROUP BY run_id, project_id, name;

-- ============================================================================
-- System Events Table (for debugging/audit)
-- ============================================================================

CREATE TABLE IF NOT EXISTS mlrunx.system_events
(
    event_id     UUID DEFAULT generateUUIDv4(),
    event_type   String,
    source       String,
    message      String,
    metadata     String DEFAULT '{}',
    created_at   DateTime64(3) DEFAULT now64(3)
)
ENGINE = MergeTree()
PARTITION BY toYYYYMMDD(created_at)
ORDER BY (created_at, event_type)
TTL created_at + INTERVAL 7 DAY;
