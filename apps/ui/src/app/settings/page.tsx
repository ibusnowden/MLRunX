'use client';

import Link from 'next/link';
import { useRouter } from 'next/navigation';
import { useEffect, useMemo, useState } from 'react';
import {
  ApiKeyInfo,
  CreateApiKeyResponse,
  UiAuthSessionResult,
  api,
  clearStoredApiConfig,
} from '@/lib/api';
import { getSupabaseUserIdentity, signOutSupabase, type SupabaseUserIdentity } from '@/lib/auth/supabase';

function formatTimestamp(epochSeconds: string | null): string {
  if (!epochSeconds) return 'Never';
  const parsed = Number(epochSeconds);
  if (!Number.isFinite(parsed)) return epochSeconds;
  return new Date(parsed * 1000).toLocaleString();
}

function describeIdentity(identity: SupabaseUserIdentity | null): string {
  if (!identity) return 'No Supabase profile loaded';
  if (identity.displayName && identity.email) return `${identity.displayName} (${identity.email})`;
  return identity.displayName || identity.email || identity.id;
}

export default function SettingsPage() {
  const router = useRouter();

  const [apiBaseUrl, setApiBaseUrl] = useState('');
  const [session, setSession] = useState<UiAuthSessionResult | null>(null);
  const [identity, setIdentity] = useState<SupabaseUserIdentity | null>(null);
  const [keys, setKeys] = useState<ApiKeyInfo[]>([]);
  const [newKeyName, setNewKeyName] = useState('training-agent');
  const [newKeyProjectId, setNewKeyProjectId] = useState('');
  const [scopeRead, setScopeRead] = useState(true);
  const [scopeWrite, setScopeWrite] = useState(true);
  const [createdKey, setCreatedKey] = useState<CreateApiKeyResponse | null>(null);
  const [loadingSession, setLoadingSession] = useState(false);
  const [loadingKeys, setLoadingKeys] = useState(false);
  const [submittingKey, setSubmittingKey] = useState(false);
  const [status, setStatus] = useState<string | null>(null);

  const refreshKeys = async () => {
    if (!session) {
      setKeys([]);
      return;
    }

    setLoadingKeys(true);
    try {
      const result = await api.listApiKeys();
      setKeys(result.keys);
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to load API keys.';
      setStatus(message);
    } finally {
      setLoadingKeys(false);
    }
  };

  const refreshSession = async () => {
    setLoadingSession(true);
    try {
      const [sessionResult, identityResult] = await Promise.all([
        api.getUiSession(),
        getSupabaseUserIdentity().catch(() => null),
      ]);
      setSession(sessionResult);
      setIdentity(identityResult);
      setStatus(null);
    } catch {
      setSession(null);
      setIdentity(null);
      setKeys([]);
    } finally {
      setLoadingSession(false);
    }
  };

  useEffect(() => {
    if (typeof window !== 'undefined') {
      setApiBaseUrl(window.location.origin);
    }
    void refreshSession();
  }, []);

  useEffect(() => {
    if (!session) {
      setNewKeyProjectId('');
      return;
    }

    if (session.project_ids.length === 1) {
      setNewKeyProjectId(session.project_ids[0]);
      return;
    }

    if (!session.project_ids.includes(newKeyProjectId)) {
      setNewKeyProjectId('');
    }
  }, [session, newKeyProjectId]);

  useEffect(() => {
    if (session) {
      void refreshKeys();
    }
  }, [session]);

  const sdkApiKey = useMemo(() => createdKey?.api_key || '<paste-api-key>', [createdKey]);
  const sdkProjectId = useMemo(() => {
    if (newKeyProjectId) return newKeyProjectId;
    if (session?.project_ids?.length === 1) return session.project_ids[0];
    return '<project-id>';
  }, [newKeyProjectId, session]);
  const pipCommand = useMemo(
    () =>
      `pip install mlrunx\nexport MLRUNX_SERVER_URL=${apiBaseUrl || 'https://mlrunx.example.com'}\nexport MLRUNX_API_KEY=${sdkApiKey}\nexport MLRUNX_PROJECT_ID=${sdkProjectId}`,
    [apiBaseUrl, sdkApiKey, sdkProjectId]
  );
  const uvCommand = useMemo(
    () =>
      `uv pip install mlrunx\nMLRUNX_SERVER_URL=${apiBaseUrl || 'https://mlrunx.example.com'} MLRUNX_API_KEY=${sdkApiKey} MLRUNX_PROJECT_ID=${sdkProjectId} python train.py`,
    [apiBaseUrl, sdkApiKey, sdkProjectId]
  );
  const copyToClipboard = async (value: string, successMessage: string) => {
    try {
      await navigator.clipboard.writeText(value);
      setStatus(successMessage);
    } catch {
      setStatus('Copy failed. Clipboard permission may be blocked.');
    }
  };

  const handleSessionLogout = async () => {
    try {
      await api.logoutUiSession();
    } catch {
      // Continue with best-effort logout even if cookie/session is already gone.
    }

    try {
      await signOutSupabase();
    } catch {
      // Continue with local cleanup even if provider logout fails.
    }

    clearStoredApiConfig();
    setSession(null);
    setIdentity(null);
    setCreatedKey(null);
    setKeys([]);
    setStatus('Logged out successfully.');
    router.replace('/login');
  };

  const handleCreateKey = async () => {
    if (!session) {
      setStatus('Sign in before creating API keys.');
      return;
    }

    const scopes: string[] = [];
    if (scopeRead) scopes.push('read');
    if (scopeWrite) scopes.push('write');

    if (scopes.length === 0) {
      setStatus('Choose at least one scope.');
      return;
    }

    if (!newKeyProjectId && session.project_ids.length > 1) {
      setStatus('Choose a project for this key.');
      return;
    }

    setSubmittingKey(true);
    try {
      const payload = {
        project_id: newKeyProjectId || undefined,
        name: newKeyName.trim() || undefined,
        scopes,
      };
      const result = await api.createApiKey(payload);
      setCreatedKey(result);
      await refreshKeys();
      setStatus(`Created key ${result.key_prefix}. Copy it now (shown only once).`);
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to create API key.';
      setStatus(message);
    } finally {
      setSubmittingKey(false);
    }
  };

  const handleRevokeKey = async (keyId: string) => {
    try {
      await api.revokeApiKey(keyId);
      await refreshKeys();
      setStatus(`Revoked key ${keyId}.`);
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to revoke API key.';
      setStatus(message);
    }
  };

  return (
    <main className="min-h-screen">
      <div className="border-b border-border bg-surface">
        <div className="max-w-5xl mx-auto px-4 sm:px-6 py-5 sm:py-6">
          <h1 className="text-2xl font-bold text-text-primary">Access Console</h1>
          <p className="text-sm text-text-secondary mt-1">Profile, API keys, and SDK settings in one place.</p>
        </div>
      </div>

      <div className="max-w-5xl mx-auto px-4 sm:px-6 py-5 sm:py-6 space-y-4">
        {status && (
          <div className="rounded-lg border border-border bg-surface px-4 py-3 text-sm text-text-secondary">{status}</div>
        )}

        <section id="profile" className="rounded-xl border border-border bg-surface p-4 sm:p-5 space-y-3">
          <div>
            <h2 className="text-lg font-semibold text-text-primary">Profile</h2>
            <p className="text-sm text-text-secondary mt-1">Session status and sign-out controls.</p>
          </div>

          <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 text-sm">
            <div className="rounded-lg border border-border bg-surface-secondary px-3 py-2">
              <p className="text-xs text-text-muted">Identity</p>
              <p className="mt-1 text-text-primary break-all">{describeIdentity(identity)}</p>
            </div>
            <div className="rounded-lg border border-border bg-surface-secondary px-3 py-2">
              <p className="text-xs text-text-muted">Session</p>
              <p className="mt-1 text-text-primary">
                {loadingSession
                  ? 'Checking...'
                  : session
                    ? `Authenticated (${session.project_ids.length} project${session.project_ids.length === 1 ? '' : 's'})`
                    : 'Not authenticated'}
              </p>
            </div>
          </div>

          <div className="flex flex-wrap gap-2">
            {!session && (
              <Link
                href="/login"
                className="inline-flex items-center justify-center px-4 py-2.5 rounded-lg bg-accent text-white text-sm font-medium hover:bg-accent-hover transition-colors"
              >
                Go to Login
              </Link>
            )}
            <button
              type="button"
              onClick={handleSessionLogout}
              disabled={!session}
              className="px-4 py-2.5 rounded-lg border border-border text-sm font-medium text-text-secondary hover:bg-surface-secondary hover:text-text-primary transition-colors disabled:opacity-60 disabled:cursor-not-allowed"
            >
              Log out
            </button>
            <button
              type="button"
              onClick={() => void refreshSession()}
              className="px-4 py-2.5 rounded-lg border border-border text-sm font-medium text-text-secondary hover:bg-surface-secondary hover:text-text-primary transition-colors"
            >
              Refresh Session
            </button>
          </div>
        </section>

        <section id="api-keys" className="rounded-xl border border-border bg-surface p-4 sm:p-5 space-y-4">
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
                disabled={!session || session.project_ids.length === 0}
              >
                <option value="">{session?.project_ids.length ? 'Auto / Select project' : 'No active session'}</option>
                {session?.project_ids.map((projectId) => (
                  <option key={projectId} value={projectId}>
                    {projectId}
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
              onClick={() => void refreshKeys()}
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
                  onClick={() => void copyToClipboard(createdKey.api_key, 'Copied new API key to clipboard.')}
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
                      <td className="px-3 py-2 text-text-secondary">{key.project_id || 'global'}</td>
                      <td className="px-3 py-2 text-text-secondary">{key.scopes.join(', ')}</td>
                      <td className="px-3 py-2 text-text-secondary">{key.is_revoked ? 'revoked' : 'active'}</td>
                      <td className="px-3 py-2 text-text-secondary">{formatTimestamp(key.last_used_at)}</td>
                      <td className="px-3 py-2">
                        <button
                          type="button"
                          disabled={key.is_revoked}
                          onClick={() => void handleRevokeKey(key.key_id)}
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

        <section id="settings" className="rounded-xl border border-border bg-surface p-4 sm:p-5 space-y-4">
          <div>
            <h2 className="text-lg font-semibold text-text-primary">Settings</h2>
            <p className="text-sm text-text-secondary mt-1">Use these commands on your training machine.</p>
          </div>

          <div className="rounded-lg border border-border bg-surface-secondary p-3">
            <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-2">
              <p className="text-xs text-text-muted">API Base URL</p>
              <button
                type="button"
                onClick={() => void copyToClipboard(apiBaseUrl, 'Copied API base URL to clipboard.')}
                className="px-3 py-1.5 rounded-md border border-border text-xs font-medium text-text-secondary hover:bg-surface"
              >
                Copy URL
              </button>
            </div>
            <code className="block mt-2 text-sm text-text-primary break-all">{apiBaseUrl}</code>
          </div>

          <div className="space-y-3">
            <div className="rounded-lg border border-border bg-surface-secondary p-3">
              <div className="flex items-center justify-between gap-2">
                <p className="text-xs text-text-muted">pip quickstart</p>
                <button
                  type="button"
                  onClick={() => void copyToClipboard(pipCommand, 'Copied pip quickstart command.')}
                  className="px-3 py-1.5 rounded-md border border-border text-xs font-medium text-text-secondary hover:bg-surface"
                >
                  Copy
                </button>
              </div>
              <pre className="mt-2 overflow-x-auto text-xs sm:text-sm text-text-primary">{pipCommand}</pre>
            </div>

            <div className="rounded-lg border border-border bg-surface-secondary p-3">
              <div className="flex items-center justify-between gap-2">
                <p className="text-xs text-text-muted">uv quickstart</p>
                <button
                  type="button"
                  onClick={() => void copyToClipboard(uvCommand, 'Copied uv quickstart command.')}
                  className="px-3 py-1.5 rounded-md border border-border text-xs font-medium text-text-secondary hover:bg-surface"
                >
                  Copy
                </button>
              </div>
              <pre className="mt-2 overflow-x-auto text-xs sm:text-sm text-text-primary">{uvCommand}</pre>
            </div>
          </div>
        </section>
      </div>
    </main>
  );
}
