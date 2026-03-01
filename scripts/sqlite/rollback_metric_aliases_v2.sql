-- Rollback metric alias v2 schema extensions (unit/description/is_active).
-- Usage:
--   sqlite3 /path/to/mlrunx.db < scripts/sqlite/rollback_metric_aliases_v2.sql

BEGIN TRANSACTION;

DROP INDEX IF EXISTS idx_metric_aliases_project_display_active;

CREATE TABLE IF NOT EXISTS metric_aliases_rollback (
    project_id TEXT NOT NULL,
    metric_name TEXT NOT NULL,
    display_name TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY(project_id, metric_name),
    FOREIGN KEY (project_id) REFERENCES projects(id)
);

INSERT INTO metric_aliases_rollback (
    project_id,
    metric_name,
    display_name,
    created_at,
    updated_at
)
SELECT
    project_id,
    metric_name,
    display_name,
    created_at,
    updated_at
FROM metric_aliases;

DROP TABLE metric_aliases;

ALTER TABLE metric_aliases_rollback RENAME TO metric_aliases;

CREATE INDEX IF NOT EXISTS idx_metric_aliases_project_id ON metric_aliases(project_id);

COMMIT;
