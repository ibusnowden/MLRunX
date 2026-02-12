'use client';

import Link from 'next/link';
import { FormEvent, useEffect, useMemo, useState } from 'react';
import {
  ApiKeyInfo,
  CreateApiKeyResponse,
  DEFAULT_API_URL,
  UiAuthSessionResult,
  api,
  clearStoredApiConfig,
  getStoredApiConfig,
  saveStoredApiConfig,
} from '@/lib/api';
import { signOutSupabase } from '@/lib/auth/supabase';

function formatTimestamp(epochSeconds: string | null): string {
  if (!epochSeconds) return 'Never';
  const parsed = Number(epochSeconds);
  if (!Number.isFinite(parsed)) return epochSeconds;
  return new Date(parsed * 1000).toLocaleString();
}

export default function SettingsPage() {
  const [apiBaseUrl, setApiBaseUrl] = useState(DEFAULT_API_URL);
  const [apiKey, setApiKey] = useState('');
  const [session, setSession] = useState<UiAuthSessionResult | null>(null);
  const [keys, setKeys] = useState<ApiKeyInfo[]>([]);
  const [newKeyName, setNewKeyName] = useState('');
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
      const result = await api.getUiSession();
      setSession(result);
      setStatus(null);
    } catch {
      setSession(null);
      setKeys([]);
    } finally {
      setLoadingSession(false);
    }
  };

  useEffect(() => {
    const config = getStoredApiConfig();
    setApiBaseUrl(config.apiBaseUrl);
    setApiKey(config.apiKey);
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

  const sdkApiKey = useMemo(() => {
    if (createdKey?.api_key) return createdKey.api_key;
    if (apiKey.trim()) return apiKey.trim();
    return '<paste-api-key>';
  }, [createdKey, apiKey]);

  const handleSave = (event: FormEvent) => {
    event.preventDefault();
    saveStoredApiConfig({
      apiBaseUrl,
      apiKey,
    });
    setStatus(`Saved at ${new Date().toLocaleTimeString()}`);
  };

  const handleReset = () => {
    clearStoredApiConfig();
    setApiBaseUrl(DEFAULT_API_URL);
    setApiKey('');
    setCreatedKey(null);
    setStatus(`Reset to defaults at ${new Date().toLocaleTimeString()}`);
  };

  const handleSessionLogout = async () => {
    try {
      await api.logoutUiSession();
      await signOutSupabase();
      await refreshSession();
      setCreatedKey(null);
      setStatus('UI session ended.');
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to end UI session.';
      setStatus(message);
    }
  };

  const handleCreateKey = async () => {
    if (!session) {
      setStatus('Sign in to a UI session before creating API keys.');
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
      setApiKey(result.api_key);
      saveStoredApiConfig({ apiKey: result.api_key });
      await refreshKeys();
      setStatus(`Created key ${result.key_prefix} (shown once below).`);
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
        <div className="max-w-4xl mx-auto px-4 sm:px-6 py-4 sm:py-6">
          <h1 className="text-xl sm:text-2xl font-bold text-text-primary">Access Console</h1>
          <p className="text-xs sm:text-sm text-text-secondary mt-1">
            Sign in, mint project-scoped API keys, and copy SDK setup commands.
          </p>
        </div>
      </div>

      <div className="max-w-4xl mx-auto px-4 sm:px-6 py-4 sm:py-6">
        <form onSubmit={handleSave} className="bg-surface rounded-xl border border-border p-4 sm:p-6 space-y-6">
          <div>
            <label htmlFor="api-url" className="block text-sm font-medium text-text-primary mb-1.5">
              API Base URL (SDK + UI)
            </label>
            <input
              id="api-url"
              type="url"
              value={apiBaseUrl}
              onChange={(event) => setApiBaseUrl(event.target.value)}
              placeholder="https://mlrunx-api.example.com"
              className="w-full rounded-lg border border-border bg-surface-secondary px-3 py-2.5 text-sm text-text-primary placeholder:text-text-muted focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
            />
            <p className="mt-1.5 text-xs text-text-muted">
              Use the public root URL, for example: <code>https://mlrunx.ibra-niang.com</code>.
            </p>
          </div>

          <div>
            <label htmlFor="api-key" className="block text-sm font-medium text-text-primary mb-1.5">
              API Key
            </label>
            <input
              id="api-key"
              type="password"
              value={apiKey}
              onChange={(event) => setApiKey(event.target.value)}
              placeholder="Paste read/write key"
              autoComplete="off"
              className="w-full rounded-lg border border-border bg-surface-secondary px-3 py-2.5 text-sm text-text-primary placeholder:text-text-muted focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
            />
            <p className="mt-1.5 text-xs text-text-muted">
              Stored in browser local storage and sent as <code>X-API-Key</code> for API-key auth.
            </p>
          </div>

          <div>
            <label className="block text-sm font-medium text-text-primary mb-1.5">
              UI Session
            </label>
            <p className="mt-1.5 text-xs text-text-muted">
              Use <Link href="/login" className="text-accent hover:underline">/login</Link> or{' '}
              <Link href="/signup" className="text-accent hover:underline">/signup</Link> for sign in. This page now manages only session status and API key self-service.
            </p>
            <div className="mt-3 flex flex-col sm:flex-row gap-2">
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
                End UI Session
              </button>
              <button
                type="button"
                onClick={() => void refreshSession()}
                className="px-4 py-2.5 rounded-lg border border-border text-sm font-medium text-text-secondary hover:bg-surface-secondary hover:text-text-primary transition-colors"
              >
                Refresh Session Status
              </button>
            </div>
            <p className="mt-2 text-xs text-text-muted">
              {loadingSession
                ? 'Checking session...'
                : session
                  ? `Authenticated (${session.auth_mode}) with scopes [${session.scopes.join(', ')}] across ${session.project_ids.length} project(s).`
                  : 'No active UI session cookie. Sign in at /login.'}
            </p>
          </div>

          <div className="border-t border-border pt-5 space-y-4">
            <div>
              <h2 className="text-base font-semibold text-text-primary">API Keys</h2>
              <p className="text-xs text-text-muted mt-1">
                Create project-scoped read/write keys for SDK usage. UI-session key creation does not allow admin scope.
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
                  <option value="">
                    {session?.project_ids.length ? 'Auto / Select project' : 'No active session'}
                  </option>
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
              <div className="rounded-lg border border-border bg-surface-secondary p-3">
                <p className="text-xs text-text-muted">New key (shown once):</p>
                <code className="block mt-1 text-xs sm:text-sm break-all text-text-primary">{createdKey.api_key}</code>
              </div>
            )}

            <div className="overflow-x-auto rounded-lg border border-border">
              <table className="w-full text-left text-sm">
                <thead className="bg-surface-secondary text-text-secondary">
                  <tr>
                    <th className="px-3 py-2 font-medium">Prefix</th>
                    <th className="px-3 py-2 font-medium">Project</th>
                    <th className="px-3 py-2 font-medium">Scopes</th>
                    <th className="px-3 py-2 font-medium">Last Used</th>
                    <th className="px-3 py-2 font-medium">Status</th>
                    <th className="px-3 py-2 font-medium">Action</th>
                  </tr>
                </thead>
                <tbody>
                  {keys.length === 0 ? (
                    <tr>
                      <td colSpan={6} className="px-3 py-4 text-text-muted">
                        {session ? 'No API keys visible for this UI session.' : 'Sign in to view keys.'}
                      </td>
                    </tr>
                  ) : (
                    keys.map((key) => (
                      <tr key={key.key_id} className="border-t border-border">
                        <td className="px-3 py-2 text-text-primary">{key.key_prefix}</td>
                        <td className="px-3 py-2 text-text-secondary">{key.project_id || 'global'}</td>
                        <td className="px-3 py-2 text-text-secondary">{key.scopes.join(', ')}</td>
                        <td className="px-3 py-2 text-text-secondary">{formatTimestamp(key.last_used_at)}</td>
                        <td className="px-3 py-2 text-text-secondary">{key.is_revoked ? 'revoked' : 'active'}</td>
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
          </div>

          <div className="border-t border-border pt-5 space-y-2">
            <h2 className="text-base font-semibold text-text-primary">SDK Quickstart</h2>
            <p className="text-xs text-text-muted">
              Point the SDK at your public MLRunX URL (root host, not <code>/api</code>).
            </p>
            <pre className="overflow-x-auto rounded-lg border border-border bg-surface-secondary p-3 text-xs sm:text-sm text-text-primary">
{`pip install mlrunx
export MLRUNX_SERVER_URL=${apiBaseUrl}
export MLRUNX_API_KEY=${sdkApiKey}`}
            </pre>
            <pre className="overflow-x-auto rounded-lg border border-border bg-surface-secondary p-3 text-xs sm:text-sm text-text-primary">
{`uv pip install mlrunx
MLRUNX_SERVER_URL=${apiBaseUrl} MLRUNX_API_KEY=${sdkApiKey} python train.py`}
            </pre>
          </div>

          <div className="flex flex-col sm:flex-row gap-2 sm:items-center">
            <button
              type="submit"
              className="px-4 py-2.5 rounded-lg bg-accent text-white text-sm font-medium hover:bg-accent-hover transition-colors"
            >
              Save Local Config
            </button>
            <button
              type="button"
              onClick={handleReset}
              className="px-4 py-2.5 rounded-lg border border-border text-sm font-medium text-text-secondary hover:bg-surface-secondary hover:text-text-primary transition-colors"
            >
              Reset Defaults
            </button>
            {status && (
              <span className="text-xs text-text-muted sm:ml-2">{status}</span>
            )}
          </div>
        </form>
      </div>
    </main>
  );
}
