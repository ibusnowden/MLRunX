'use client';

import { FormEvent, useEffect, useState } from 'react';
import {
  DEFAULT_API_URL,
  UiAuthSessionResult,
  api,
  clearStoredApiConfig,
  getStoredApiConfig,
  saveStoredApiConfig,
} from '@/lib/api';

export default function SettingsPage() {
  const [apiBaseUrl, setApiBaseUrl] = useState(DEFAULT_API_URL);
  const [apiKey, setApiKey] = useState('');
  const [jwtInput, setJwtInput] = useState('');
  const [session, setSession] = useState<UiAuthSessionResult | null>(null);
  const [loadingSession, setLoadingSession] = useState(false);
  const [status, setStatus] = useState<string | null>(null);

  const refreshSession = async () => {
    setLoadingSession(true);
    try {
      const result = await api.getUiSession();
      setSession(result);
    } catch {
      setSession(null);
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
    setJwtInput('');
    setStatus(`Reset to defaults at ${new Date().toLocaleTimeString()}`);
  };

  const handleSessionLogin = async () => {
    if (!jwtInput.trim()) {
      setStatus('JWT is required to sign in.');
      return;
    }

    try {
      const result = await api.loginUiSession(jwtInput.trim());
      setJwtInput('');
      await refreshSession();
      setStatus(`UI session started (expires ${result.expires_at}).`);
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to start UI session.';
      setStatus(message);
    }
  };

  const handleSessionLogout = async () => {
    try {
      await api.logoutUiSession();
      await refreshSession();
      setStatus('UI session ended.');
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to end UI session.';
      setStatus(message);
    }
  };

  return (
    <main className="min-h-screen">
      <div className="border-b border-border bg-surface">
        <div className="max-w-4xl mx-auto px-4 sm:px-6 py-4 sm:py-6">
          <h1 className="text-xl sm:text-2xl font-bold text-text-primary">Settings</h1>
          <p className="text-xs sm:text-sm text-text-secondary mt-1">
            Configure API access for this browser session.
          </p>
        </div>
      </div>

      <div className="max-w-4xl mx-auto px-4 sm:px-6 py-4 sm:py-6">
        <form onSubmit={handleSave} className="bg-surface rounded-xl border border-border p-4 sm:p-6 space-y-5">
          <div>
            <label htmlFor="api-url" className="block text-sm font-medium text-text-primary mb-1.5">
              API Base URL
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
              Default: {DEFAULT_API_URL}
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
              Used for SDK/service API-key flow. Sent as <code>X-API-Key</code>.
            </p>
          </div>

          <div>
            <label htmlFor="session-jwt" className="block text-sm font-medium text-text-primary mb-1.5">
              Sign In JWT (Not Stored)
            </label>
            <textarea
              id="session-jwt"
              value={jwtInput}
              onChange={(event) => setJwtInput(event.target.value)}
              placeholder="Paste JWT to exchange for secure HttpOnly session cookie"
              autoComplete="off"
              rows={4}
              className="w-full rounded-lg border border-border bg-surface-secondary px-3 py-2.5 text-sm text-text-primary placeholder:text-text-muted focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
            />
            <p className="mt-1.5 text-xs text-text-muted">
              JWT is sent once to <code>/api/v1/ui-auth/login</code> and exchanged for HttpOnly + CSRF cookies.
            </p>
            <div className="mt-3 flex flex-col sm:flex-row gap-2">
              <button
                type="button"
                onClick={handleSessionLogin}
                className="px-4 py-2.5 rounded-lg bg-accent text-white text-sm font-medium hover:bg-accent-hover transition-colors"
              >
                Sign In UI Session
              </button>
              <button
                type="button"
                onClick={handleSessionLogout}
                className="px-4 py-2.5 rounded-lg border border-border text-sm font-medium text-text-secondary hover:bg-surface-secondary hover:text-text-primary transition-colors"
              >
                Sign Out UI Session
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
                  ? `Authenticated (${session.auth_mode}) with ${session.project_ids.length} project memberships.`
                  : 'No active UI session cookie.'}
            </p>
          </div>

          <div className="flex flex-col sm:flex-row gap-2 sm:items-center">
            <button
              type="submit"
              className="px-4 py-2.5 rounded-lg bg-accent text-white text-sm font-medium hover:bg-accent-hover transition-colors"
            >
              Save Settings
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
