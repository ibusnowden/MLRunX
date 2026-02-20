import { ApiKeyInfo, CreateApiKeyResponse, ProjectInfo } from '@/lib/api';

function formatTimestamp(epochSeconds: string | null): string {
  if (!epochSeconds) return 'Never';
  const parsed = Number(epochSeconds);
  if (!Number.isFinite(parsed)) return epochSeconds;
  return new Date(parsed * 1000).toLocaleString();
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
  formatProjectRef,
  handleCreateKey,
  handleRevokeKey,
  refreshKeys,
  copyToClipboard,
}: {
  session: unknown;
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
  formatProjectRef: (projectId: string | null) => string;
  handleCreateKey: () => void;
  handleRevokeKey: (keyId: string) => void;
  refreshKeys: () => void;
  copyToClipboard: (value: string, successMessage: string) => void;
}) {
  const availableProjectIds = availableProjects.map((p) => p.project_id);

  return (
    <section className="rounded-xl border border-border bg-surface p-4 sm:p-5 space-y-4">
      <div>
        <h2 className="text-lg font-semibold text-text-primary">API Keys</h2>
        <p className="text-sm text-text-secondary mt-1">
          Create project-scoped read/write keys for SDK usage.
        </p>
      </div>

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
        <div>
          <label htmlFor="key-name" className="block text-sm font-medium text-text-primary mb-1.5">
            Key Name
          </label>
          <input
            id="key-name"
            type="text"
            value={newKeyName}
            onChange={(event) => setNewKeyName(event.target.value)}
            placeholder="training-agent"
            className="w-full rounded-lg border border-border bg-surface-secondary px-3 py-2.5 text-sm text-text-primary placeholder:text-text-muted focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
          />
        </div>

        <div>
          <label htmlFor="key-project" className="block text-sm font-medium text-text-primary mb-1.5">
            Project
          </label>
          <select
            id="key-project"
            value={newKeyProjectId}
            onChange={(event) => setNewKeyProjectId(event.target.value)}
            className="w-full rounded-lg border border-border bg-surface-secondary px-3 py-2.5 text-sm text-text-primary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
            disabled={!session || availableProjectIds.length === 0}
          >
            <option value="">{availableProjectIds.length ? 'Auto / Select project' : 'No active session'}</option>
            {availableProjects.map((project) => (
              <option key={project.project_id} value={project.project_id}>
                {formatProjectRef(project.project_id)}
              </option>
            ))}
          </select>
        </div>
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
          disabled={!session}
        >
          <option value="30">30 days</option>
          <option value="60">60 days</option>
          <option value="90">90 days</option>
        </select>
        <p className="mt-1 text-xs text-text-muted">UI-created keys must expire within policy limits.</p>
      </div>

      <div className="flex flex-wrap gap-2">
        <button
          type="button"
          onClick={handleCreateKey}
          disabled={submittingKey || !session}
          className="px-4 py-2.5 rounded-lg bg-accent text-white text-sm font-medium hover:bg-accent-hover transition-colors disabled:opacity-60 disabled:cursor-not-allowed"
        >
          {submittingKey ? 'Creating...' : 'Create API Key'}
        </button>
        <button
          type="button"
          onClick={refreshKeys}
          disabled={loadingKeys || !session}
          className="px-4 py-2.5 rounded-lg border border-border text-sm font-medium text-text-secondary hover:bg-surface-secondary hover:text-text-primary transition-colors disabled:opacity-60 disabled:cursor-not-allowed"
        >
          {loadingKeys ? 'Loading...' : 'Refresh Keys'}
        </button>
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
