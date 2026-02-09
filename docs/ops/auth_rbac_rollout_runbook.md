# Auth RBAC Rollout Runbook (PR4)

## Purpose

Safely roll out endpoint RBAC enforcement in three phases:
1. Read-only endpoints (`viewer+`)
2. Mutating endpoints (`editor+`)
3. Admin/sensitive operations (`owner`)

This runbook assumes API-key auth remains the baseline safety net until all phases are complete.

## Required Feature Flags

Master gate:
- `MLRUNX_RBAC_ENDPOINT_ENFORCEMENT_ENABLED`

Per-tier gates:
- `MLRUNX_RBAC_READ_ENFORCEMENT_ENABLED`
- `MLRUNX_RBAC_WRITE_ENFORCEMENT_ENABLED`
- `MLRUNX_RBAC_ADMIN_ENFORCEMENT_ENABLED`

## Deployment Order

1. Run migrations first.
2. Deploy application with baseline-safe flags.
3. Enable rollout tiers in canary.
4. Promote to full fleet only after alert windows stay green.

## Baseline (Pre-Canary)

Set:
- `MLRUNX_RBAC_ENDPOINT_ENFORCEMENT_ENABLED=true`
- `MLRUNX_RBAC_READ_ENFORCEMENT_ENABLED=false`
- `MLRUNX_RBAC_WRITE_ENFORCEMENT_ENABLED=false`
- `MLRUNX_RBAC_ADMIN_ENFORCEMENT_ENABLED=false`

Expected behavior:
- API-key path remains fully scope enforced.
- UI JWT/session path remains available with project access checks, while tier gates are staged.

## Canary Sequence

Canary duration per stage:
- Minimum 30 minutes and at least one peak-traffic window.

Phase 1 (Read):
- `READ=true`, `WRITE=false`, `ADMIN=false`
- Watch denied rates and support incidents.

Phase 2 (Write):
- `READ=true`, `WRITE=true`, `ADMIN=false`
- Confirm mutating workflows (run init/finish, ingest) are stable.

Phase 3 (Admin):
- `READ=true`, `WRITE=true`, `ADMIN=true`
- Validate key/share-token admin operations and denials.

## Dashboard Queries

Use `audit_events` for all dashboards and alerts.

PostgreSQL: denied actions by reason (last 15m)
```sql
SELECT
  COALESCE(metadata->>'reason', 'unknown') AS reason,
  COUNT(*) AS denied_count
FROM audit_events
WHERE occurred_at >= NOW() - INTERVAL '15 minutes'
  AND outcome = 'denied'
GROUP BY 1
ORDER BY denied_count DESC;
```

PostgreSQL: sensitive success/denied trends (5m buckets, last 2h)
```sql
SELECT
  date_trunc('minute', occurred_at) AS minute,
  action,
  outcome,
  COUNT(*) AS events
FROM audit_events
WHERE occurred_at >= NOW() - INTERVAL '2 hours'
  AND action IN (
    'run.init', 'run.finish', 'run.delete',
    'api_key.create', 'api_key.revoke',
    'share_token.create', 'share_token.revoke'
  )
GROUP BY 1, 2, 3
ORDER BY minute DESC, action, outcome;
```

SQLite: denied actions by reason (last 15m)
```sql
SELECT
  COALESCE(json_extract(metadata, '$.reason'), 'unknown') AS reason,
  COUNT(*) AS denied_count
FROM audit_events
WHERE datetime(occurred_at) >= datetime('now', '-15 minutes')
  AND outcome = 'denied'
GROUP BY reason
ORDER BY denied_count DESC;
```

SQLite: auth mode split for denials (last 15m)
```sql
SELECT
  COALESCE(json_extract(metadata, '$.auth_mode'), 'unknown') AS auth_mode,
  COUNT(*) AS denied_count
FROM audit_events
WHERE datetime(occurred_at) >= datetime('now', '-15 minutes')
  AND outcome = 'denied'
GROUP BY auth_mode
ORDER BY denied_count DESC;
```

## Alert Thresholds

Start with conservative thresholds for canary:
- `rbac_denied_rate_spike`: denied events > 5% of total audited events for 10m.
- `rbac_scope_denied_spike`: `reason=scope_denied` count > 20 in 10m.
- `rbac_project_mismatch_spike`: project mismatch denials > 10 in 10m.
- `sensitive_action_drop`: success count for sensitive actions drops > 30% vs previous 30m baseline.

During full rollout, tighten thresholds and add per-project monitors for high-traffic projects.

## Promotion Criteria

Promote a phase only if all are true:
- No sustained 5xx increase.
- No sustained denied-rate alert.
- No regression in run init/finish success SLO.
- No new auth support incidents tied to that tier.

## Rollback

Immediate rollback lever:
- `MLRUNX_RBAC_ENDPOINT_ENFORCEMENT_ENABLED=false`

Tier rollback levers:
- Disable the last enabled tier flag first (`ADMIN`, then `WRITE`, then `READ`).

Post-rollback actions:
1. Keep API-key auth path active.
2. Capture top denial reasons from `audit_events`.
3. File remediation issue with endpoint/action breakdown.
