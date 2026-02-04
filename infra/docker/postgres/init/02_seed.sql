-- ============================================================================
-- MLRunX Seed Data
-- Creates default project and API key for local development.
-- ============================================================================

-- Insert default project
INSERT INTO projects (id, name, description, settings)
VALUES (
    '00000000-0000-0000-0000-000000000001',
    'default',
    'Default project for local development',
    '{"retention_days": 90}'::jsonb
)
ON CONFLICT (name) DO NOTHING;

-- Insert development API key
-- Key: mlrunx_dev_key_12345 (for local development only!)
-- Hash: SHA256 of the key (pre-computed for simplicity)
-- In production, users should generate their own keys
INSERT INTO api_keys (id, key_hash, name, project_id, scopes)
VALUES (
    '00000000-0000-0000-0000-000000000002',
    'dev_key_hash_placeholder',
    'Development API Key',
    '00000000-0000-0000-0000-000000000001',
    '["read", "write", "admin"]'::jsonb
)
ON CONFLICT (key_hash) DO NOTHING;

-- Log the seeded data
DO $$
BEGIN
    RAISE NOTICE 'MLRunX seed data initialized:';
    RAISE NOTICE '  - Default project: default';
    RAISE NOTICE '  - Dev API key: mlrunx_dev_key_12345 (use MLRUNX_AUTH_DISABLED=true)';
END $$;
