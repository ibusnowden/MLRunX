import { useMemo, useState } from 'react';
import {
  ApiKeyInfo,
  CreateApiKeyResponse,
  ProjectInfo,
  UiAuthSessionResult,
} from '@/lib/api';

function formatTimestamp(epochSeconds: string | null): string {
  if (!epochSeconds) return 'Never';
  const parsed = Number(epochSeconds);
  if (!Number.isFinite(parsed)) return epochSeconds;
  return new Date(parsed * 1000).toLocaleString();
}

function formatDuration(seconds: number): string {
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  if (seconds < 86_400) return `${Math.floor(seconds / 3600)}h`;
  return `${Math.floor(seconds / 86_400)}d`;
}

function formatExpiryFromNow(days: number): string {
  const millis = Date.now() + days * 24 * 60 * 60 * 1000;
  return new Date(millis).toLocaleString();
}

export function ApiKeysTab({
  session,
  availableProjects,
  newKeyName,
  setNewKeyName,
  newKeyProjectId,
  setNewKeyProjectId,
  scopeRead,
  setScopeRead,
  scopeWrite,
  setScopeWrite,
  keyTtlDays,
  setKeyTtlDays,
  submittingKey,
  loadingKeys,
  createdKey,
  keys,
  keyPolicyMaxTtlSeconds,
  recommendedKeyTtlDays,
  keyTtlOptionsDays,
  formatProjectRef,
  handleCreateKey,
  handleRevokeKey,
  refreshKeys,
  copyToClipboard,
}: {
  session: UiAuthSessionResult | null;
  availableProjects: ProjectInfo[];
  newKeyName: string;
  setNewKeyName: (v: string) => void;
  newKeyProjectId: string;
  setNewKeyProjectId: (v: string) => void;
  scopeRead: boolean;
  setScopeRead: (v: boolean) => void;
  scopeWrite: boolean;
  setScopeWrite: (v: boolean) => void;
  keyTtlDays: string;
  setKeyTtlDays: (v: string) => void;
  submittingKey: boolean;
  loadingKeys: boolean;
  createdKey: CreateApiKeyResponse | null;
  keys: ApiKeyInfo[];
  keyPolicyMaxTtlSeconds: number;
  recommendedKeyTtlDays: number;
  keyTtlOptionsDays: number[];
  formatProjectRef: (projectId: string | null) => string;
  handleCreateKey: (mode: 'recommended' | 'advanced') => void;
  handleRevokeKey: (keyId: string) => void;
  refreshKeys: () => void;
  copyToClipboard: (value: string, successMessage: string) => void;
}) {
  const [showAdvanced, setShowAdvanced] = useState(false);
  const availableProjectIds = availableProjects.map((p) => p.project_id);
  const hasSession = Boolean(session);
  const hasProjectSelection = newKeyProjectId.length > 0;
  const maxTtlDays = Math.max(1, Math.floor(keyPolicyMaxTtlSeconds / (24 * 60 * 60)));
  const recommendedExpiresAt = useMemo(
    () => formatExpiryFromNow(recommendedKeyTtlDays),
    [recommendedKeyTtlDays],
  );
  const advancedTtlDays = Number(keyTtlDays);
  const advancedExpiresAt = useMemo(() => {
    if (!Number.isFinite(advancedTtlDays) || advancedTtlDays <= 0) return null;
    return formatExpiryFromNow(advancedTtlDays);
  }, [advancedTtlDays]);

  return (
    <section className="rounded-xl border border-border bg-surface p-4 sm:p-5 space-y-4">
      <div>
        <h2 className="text-lg font-semibold text-text-primary">SDK Keys</h2>
        <p className="text-sm text-text-secondary mt-1">
          Use your browser session for UI access. Create API keys only for SDK and automation.
        </p>
      </div>

      <div className="rounded-lg border border-border bg-surface-secondary p-3">
        <h3 className="text-sm font-semibold text-text-primary">Browser Session</h3>
        <p className="mt-1 text-sm text-text-secondary">
          You are signed in with secure session cookies. No API key is needed for this dashboard.
        </p>
        <div className="mt-3 grid grid-cols-1 gap-2 text-xs text-text-muted sm:grid-cols-3">
          <div>
            Session TTL:{' '}
            {session ? `${formatDuration(session.ui_session_ttl_seconds)} (sliding)` : 'not signed in'}
          </div>
          <div>Accessible projects: {session ? session.project_ids.length : 0}</div>
          <div>SDK key max TTL policy: {maxTtlDays} days</div>
        </div>
      </div>

      <div className="rounded-lg border border-border bg-surface-secondary p-3 space-y-3">
        <div>
          <label htmlFor="key-project" className="block text-sm font-medium text-text-primary mb-1.5">
            SDK Project
          </label>
          <select
            id="key-project"
            value={newKeyProjectId}
            onChange={(event) => setNewKeyProjectId(event.target.value)}
            className="w-full rounded-lg border border-border bg-surface px-3 py-2.5 text-sm text-text-primary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
            disabled={!hasSession || availableProjectIds.length === 0}
          >
            <option value="">{availableProjectIds.length ? 'Select project' : 'No active session'}</option>
            {availableProjects.map((project) => (
              <option key={project.project_id} value={project.project_id}>
                {formatProjectRef(project.project_id)}
              </option>
            ))}
          </select>
        </div>

        <div className="flex flex-wrap gap-2">
          <button
            type="button"
            onClick={() => handleCreateKey('recommended')}
            disabled={submittingKey || !hasSession || !hasProjectSelection}
            className="px-4 py-2.5 rounded-lg bg-accent text-white text-sm font-medium hover:bg-accent-hover transition-colors disabled:opacity-60 disabled:cursor-not-allowed"
          >
            {submittingKey ? 'Creating...' : 'Create SDK Key (Recommended)'}
          </button>
          <button
            type="button"
            onClick={() => setShowAdvanced((prev) => !prev)}
            disabled={!hasSession}
            className="px-4 py-2.5 rounded-lg border border-border text-sm font-medium text-text-secondary hover:bg-surface hover:text-text-primary transition-colors disabled:opacity-60 disabled:cursor-not-allowed"
          >
            {showAdvanced ? 'Hide Advanced' : 'Show Advanced'}
          </button>
          <button
            type="button"
            onClick={refreshKeys}
            disabled={loadingKeys || !hasSession}
            className="px-4 py-2.5 rounded-lg border border-border text-sm font-medium text-text-secondary hover:bg-surface hover:text-text-primary transition-colors disabled:opacity-60 disabled:cursor-not-allowed"
          >
            {loadingKeys ? 'Loading...' : 'Refresh Keys'}
          </button>
        </div>
        <p className="text-xs text-text-muted">
          Recommended profile: name <code>sdk-agent</code>, scopes <code>read</code> + <code>write</code>, TTL{' '}
          {recommendedKeyTtlDays} days (expires around {recommendedExpiresAt}).
        </p>

        {showAdvanced && (
          <div className="space-y-3 rounded-lg border border-border bg-surface p-3">
            <div>
              <label htmlFor="key-name" className="block text-sm font-medium text-text-primary mb-1.5">
                Key Name
              </label>
              <input
                id="key-name"
                type="text"
                value={newKeyName}
                onChange={(event) => setNewKeyName(event.target.value)}
                placeholder="sdk-agent"
                className="w-full rounded-lg border border-border bg-surface-secondary px-3 py-2.5 text-sm text-text-primary placeholder:text-text-muted focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
              />
            </div>

            <div className="flex flex-wrap gap-4">
              <label className="inline-flex items-center gap-2 text-sm text-text-primary">
                <input
                  type="checkbox"
                  checked={scopeRead}
                  onChange={(event) => setScopeRead(event.target.checked)}
                  className="rounded border-border bg-surface-secondary"
                />
                read
              </label>
              <label className="inline-flex items-center gap-2 text-sm text-text-primary">
                <input
                  type="checkbox"
                  checked={scopeWrite}
                  onChange={(event) => setScopeWrite(event.target.checked)}
                  className="rounded border-border bg-surface-secondary"
                />
                write
              </label>
            </div>

            <div>
              <label htmlFor="key-ttl" className="block text-sm font-medium text-text-primary mb-1.5">
                Key TTL
              </label>
              <select
                id="key-ttl"
                value={keyTtlDays}
                onChange={(event) => setKeyTtlDays(event.target.value)}
                className="w-full rounded-lg border border-border bg-surface-secondary px-3 py-2.5 text-sm text-text-primary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent sm:w-64"
                disabled={!hasSession}
              >
                {keyTtlOptionsDays.map((days) => (
                  <option key={days} value={String(days)}>
                    {days} days
                  </option>
                ))}
              </select>
              <p className="mt-1 text-xs text-text-muted">
                Max TTL policy: {maxTtlDays} days.
                {advancedExpiresAt ? ` Expires around ${advancedExpiresAt}.` : ''}
              </p>
            </div>

            <button
              type="button"
              onClick={() => handleCreateKey('advanced')}
              disabled={submittingKey || !hasSession || !hasProjectSelection}
              className="px-4 py-2.5 rounded-lg border border-border text-sm font-medium text-text-secondary hover:bg-surface-secondary hover:text-text-primary transition-colors disabled:opacity-60 disabled:cursor-not-allowed"
            >
              {submittingKey ? 'Creating...' : 'Create with Advanced Options'}
            </button>
          </div>
        )}
      </div>

      {createdKey && (
        <div className="rounded-lg border border-warning/30 bg-warning/10 p-3">
          <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-2">
            <p className="text-xs text-warning">New key (shown once, not auto-saved in browser storage):</p>
            <button
              type="button"
              onClick={() => copyToClipboard(createdKey.api_key, 'Copied new API key to clipboard.')}
              className="px-3 py-1.5 rounded-md border border-warning/40 text-xs font-medium text-warning hover:bg-warning/10"
            >
              Copy Key
            </button>
          </div>
          <code className="block mt-2 text-xs sm:text-sm break-all text-text-primary">{createdKey.api_key}</code>
          {createdKey.expires_at && (
            <p className="mt-2 text-xs text-text-muted">Expires at {new Date(createdKey.expires_at).toLocaleString()}</p>
          )}
        </div>
      )}

      <div className="overflow-x-auto rounded-lg border border-border">
        <table className="w-full text-left text-sm">
          <thead className="bg-surface-secondary text-text-secondary">
            <tr>
              <th className="px-3 py-2 font-medium">Prefix</th>
              <th className="px-3 py-2 font-medium">Project</th>
              <th className="px-3 py-2 font-medium">Scopes</th>
              <th className="px-3 py-2 font-medium">Status</th>
              <th className="px-3 py-2 font-medium">Last Used</th>
              <th className="px-3 py-2 font-medium">Action</th>
            </tr>
          </thead>
          <tbody>
            {keys.length === 0 ? (
              <tr>
                <td colSpan={6} className="px-3 py-4 text-text-muted">
                  {session ? 'No API keys visible for this session.' : 'Sign in to view keys.'}
                </td>
              </tr>
            ) : (
              keys.map((key) => (
                <tr key={key.key_id} className="border-t border-border">
                  <td className="px-3 py-2 text-text-primary">{key.key_prefix}</td>
                  <td className="px-3 py-2 text-text-secondary">{formatProjectRef(key.project_id)}</td>
                  <td className="px-3 py-2 text-text-secondary">{key.scopes.join(', ')}</td>
                  <td className="px-3 py-2 text-text-secondary">{key.is_revoked ? 'revoked' : 'active'}</td>
                  <td className="px-3 py-2 text-text-secondary">{formatTimestamp(key.last_used_at)}</td>
                  <td className="px-3 py-2">
                    <button
                      type="button"
                      disabled={key.is_revoked}
                      onClick={() => handleRevokeKey(key.key_id)}
                      className="px-2.5 py-1.5 rounded-md border border-border text-xs font-medium text-text-secondary hover:bg-surface-secondary hover:text-text-primary transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                    >
                      Revoke
                    </button>
                  </td>
                </tr>
              ))
            )}
          </tbody>
        </table>
      </div>
    </section>
  );
}
