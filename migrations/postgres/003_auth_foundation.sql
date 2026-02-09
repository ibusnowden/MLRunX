-- ============================================================================
-- MLRunX PostgreSQL Schema: Auth Foundation (Option 2, Phase 1)
-- Migration: 003_auth_foundation.sql
-- ============================================================================
-- Adds foundational tables for user identities, project memberships, and
-- security audit events. This migration is intentionally additive and keeps
-- existing API-key behavior backward compatible.
-- ============================================================================

-- Membership roles for project-level access control.
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'membership_role') THEN
        CREATE TYPE membership_role AS ENUM ('owner', 'editor', 'viewer');
    END IF;
END $$;

-- ============================================================================
-- Users Table
-- ============================================================================

CREATE TABLE IF NOT EXISTS users (
    id                  UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    email               VARCHAR(320),
    display_name        VARCHAR(256),
    auth_provider       VARCHAR(64) NOT NULL DEFAULT 'local',
    external_subject    VARCHAR(512),
    is_service_account  BOOLEAN NOT NULL DEFAULT FALSE,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    disabled_at         TIMESTAMPTZ,
    UNIQUE (auth_provider, external_subject)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_users_email_active
    ON users(email)
    WHERE email IS NOT NULL AND disabled_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_users_provider_subject
    ON users(auth_provider, external_subject);

-- ============================================================================
-- Project Memberships Table
-- ============================================================================

CREATE TABLE IF NOT EXISTS project_memberships (
    id                  UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    project_id          UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    user_id             UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role                membership_role NOT NULL DEFAULT 'viewer',
    granted_by_user_id  UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    revoked_at          TIMESTAMPTZ
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_project_memberships_active_unique
    ON project_memberships(project_id, user_id)
    WHERE revoked_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_project_memberships_user
    ON project_memberships(user_id, project_id)
    WHERE revoked_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_project_memberships_project
    ON project_memberships(project_id, role)
    WHERE revoked_at IS NULL;

-- ============================================================================
-- API Keys Table Extensions (backward-compatible)
-- ============================================================================

ALTER TABLE api_keys
    ADD COLUMN IF NOT EXISTS created_by_user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS metadata JSONB DEFAULT '{}'::JSONB,
    ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ADD COLUMN IF NOT EXISTS last_used_ip INET,
    ADD COLUMN IF NOT EXISTS last_used_user_agent TEXT;

ALTER TABLE api_keys
    ALTER COLUMN scopes SET DEFAULT '{}'::VARCHAR[];

CREATE INDEX IF NOT EXISTS idx_api_keys_creator
    ON api_keys(created_by_user_id)
    WHERE revoked_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_api_keys_last_used
    ON api_keys(last_used_at DESC)
    WHERE revoked_at IS NULL;

-- ============================================================================
-- Audit Events Table
-- ============================================================================

CREATE TABLE IF NOT EXISTS audit_events (
    id                  BIGSERIAL PRIMARY KEY,
    occurred_at         TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    project_id          UUID REFERENCES projects(id) ON DELETE SET NULL,
    run_id              UUID REFERENCES runs(id) ON DELETE SET NULL,
    actor_user_id       UUID REFERENCES users(id) ON DELETE SET NULL,
    actor_key_id        UUID REFERENCES api_keys(id) ON DELETE SET NULL,
    action              VARCHAR(128) NOT NULL,
    resource_type       VARCHAR(64) NOT NULL,
    resource_id         VARCHAR(128),
    outcome             VARCHAR(32) NOT NULL DEFAULT 'success',
    request_id          VARCHAR(128),
    client_ip           INET,
    user_agent          TEXT,
    metadata            JSONB NOT NULL DEFAULT '{}'::JSONB
);

CREATE INDEX IF NOT EXISTS idx_audit_events_occurred
    ON audit_events(occurred_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_events_project
    ON audit_events(project_id, occurred_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_events_actor_user
    ON audit_events(actor_user_id, occurred_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_events_actor_key
    ON audit_events(actor_key_id, occurred_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_events_run
    ON audit_events(run_id, occurred_at DESC);

-- Keep updated_at fields current for mutable tables.
DROP TRIGGER IF EXISTS users_updated_at ON users;
CREATE TRIGGER users_updated_at
    BEFORE UPDATE ON users
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at();

DROP TRIGGER IF EXISTS project_memberships_updated_at ON project_memberships;
CREATE TRIGGER project_memberships_updated_at
    BEFORE UPDATE ON project_memberships
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at();

DROP TRIGGER IF EXISTS api_keys_updated_at ON api_keys;
CREATE TRIGGER api_keys_updated_at
    BEFORE UPDATE ON api_keys
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at();

COMMENT ON TABLE users IS
'Identity records for human and service users. Added for Option-2 authn/authz rollout.';

COMMENT ON TABLE project_memberships IS
'Maps users to projects with role-based access levels (owner/editor/viewer).';

COMMENT ON TABLE audit_events IS
'Immutable security/audit trail for authenticated actions and authorization decisions.';
