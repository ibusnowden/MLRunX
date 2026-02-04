-- ============================================================================
-- MLRunX ClickHouse Schema: Metrics
-- Migration: 001_metrics_schema.sql
-- ============================================================================
-- Designed for high-throughput metric ingestion from ML training jobs.
-- Uses MergeTree for efficient time-series storage with automatic TTL cleanup.
-- ============================================================================

-- Create the mlrunx database if it doesn't exist
CREATE DATABASE IF NOT EXISTS mlrunx;

-- ============================================================================
-- Metrics Table
-- ============================================================================
-- Primary table for storing time-series metrics from training runs.
-- Optimized for:
--   - High-throughput writes (millions of points/sec)
--   - Efficient queries by run_id + name combination
--   - Time-range queries with step ordering
--
-- Partition by month for efficient data lifecycle management.
-- Order by (run_id, name, step) for fast metric-specific queries.

CREATE TABLE IF NOT EXISTS mlrunx.metrics
(
    -- Run identification
    run_id       String,
    project_id   String,

    -- Metric data
    name         String,
    step         Int64,
    value        Float64,

    -- Timestamps
    timestamp    DateTime64(3) DEFAULT now64(3),
    ingested_at  DateTime64(3) DEFAULT now64(3),

    -- Deduplication key (for idempotent writes)
    batch_id     String DEFAULT '',

    -- Index for faster lookups
    INDEX idx_project project_id TYPE bloom_filter GRANULARITY 4,
    INDEX idx_name name TYPE bloom_filter GRANULARITY 4
)
ENGINE = ReplacingMergeTree(ingested_at)
PARTITION BY toYYYYMM(timestamp)
ORDER BY (run_id, name, step, timestamp)
TTL timestamp + INTERVAL 90 DAY
SETTINGS index_granularity = 8192;

-- ============================================================================
-- Metrics Summary Table (Materialized View)
-- ============================================================================
-- Pre-aggregated summary statistics per metric per run.
-- Used for quick dashboard queries without scanning full metrics table.

CREATE TABLE IF NOT EXISTS mlrunx.metrics_summary
(
    run_id       String,
    project_id   String,
    name         String,

    -- Summary statistics
    min_value    Float64,
    max_value    Float64,
    last_value   Float64,
    last_step    Int64,
    count        UInt64,

    -- Time bounds
    first_at     DateTime64(3),
    last_at      DateTime64(3),

    -- Update tracking
    updated_at   DateTime64(3) DEFAULT now64(3)
)
ENGINE = ReplacingMergeTree(updated_at)
ORDER BY (run_id, name)
SETTINGS index_granularity = 8192;

-- Materialized view to auto-populate summary
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
-- Run Metrics Count Table
-- ============================================================================
-- Quick lookup for total metrics per run.
-- Updated via materialized view on insert.

CREATE TABLE IF NOT EXISTS mlrunx.run_metrics_count
(
    run_id       String,
    count        UInt64,
    updated_at   DateTime64(3) DEFAULT now64(3)
)
ENGINE = SummingMergeTree(count)
ORDER BY run_id
SETTINGS index_granularity = 8192;

CREATE MATERIALIZED VIEW IF NOT EXISTS mlrunx.run_metrics_count_mv
TO mlrunx.run_metrics_count
AS SELECT
    run_id,
    count() AS count,
    now64(3) AS updated_at
FROM mlrunx.metrics
GROUP BY run_id;

-- ============================================================================
-- System Metrics Table (Optional)
-- ============================================================================
-- For GPU utilization, memory usage, etc. collected by SDK.
-- Separate from training metrics for different query patterns.

CREATE TABLE IF NOT EXISTS mlrunx.system_metrics
(
    run_id       String,

    -- System metric identification
    metric_type  LowCardinality(String), -- 'gpu_util', 'gpu_memory', 'cpu_util', 'memory'
    device_id    UInt8 DEFAULT 0,        -- GPU index

    -- Values
    value        Float64,
    timestamp    DateTime64(3) DEFAULT now64(3),

    INDEX idx_type metric_type TYPE set(100) GRANULARITY 4
)
ENGINE = MergeTree()
PARTITION BY toYYYYMMDD(timestamp)
ORDER BY (run_id, metric_type, device_id, timestamp)
TTL timestamp + INTERVAL 30 DAY
SETTINGS index_granularity = 8192;
