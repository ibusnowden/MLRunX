-- ============================================================================
-- MLRunX PostgreSQL Migration Ledger
-- Migration: 004_schema_migrations.sql
-- ============================================================================
-- Provides an explicit migration history table so production upgrades can be
-- reasoned about safely over long-lived deployments.

CREATE TABLE IF NOT EXISTS schema_migrations (
    version TEXT PRIMARY KEY,
    description TEXT NOT NULL,
    applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

INSERT INTO schema_migrations (version, description)
VALUES ('004_schema_migrations', 'create migration ledger table')
ON CONFLICT (version) DO NOTHING;
