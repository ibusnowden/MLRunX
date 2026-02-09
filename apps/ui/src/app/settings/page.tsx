'use client';

import { FormEvent, useEffect, useState } from 'react';
import {
  DEFAULT_API_URL,
  clearStoredApiConfig,
  getStoredApiConfig,
  saveStoredApiConfig,
} from '@/lib/api';

export default function SettingsPage() {
  const [apiBaseUrl, setApiBaseUrl] = useState(DEFAULT_API_URL);
  const [apiKey, setApiKey] = useState('');
  const [sessionJwt, setSessionJwt] = useState('');
  const [status, setStatus] = useState<string | null>(null);

  useEffect(() => {
    const config = getStoredApiConfig();
    setApiBaseUrl(config.apiBaseUrl);
    setApiKey(config.apiKey);
    setSessionJwt(config.sessionJwt);
  }, []);

  const handleSave = (event: FormEvent) => {
    event.preventDefault();
    saveStoredApiConfig({
      apiBaseUrl,
      apiKey,
      sessionJwt,
    });
    setStatus(`Saved at ${new Date().toLocaleTimeString()}`);
  };

  const handleReset = () => {
    clearStoredApiConfig();
    setApiBaseUrl(DEFAULT_API_URL);
    setApiKey('');
    setSessionJwt('');
    setStatus(`Reset to defaults at ${new Date().toLocaleTimeString()}`);
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
              Used when no JWT is configured. Sent as <code>X-API-Key</code>.
            </p>
          </div>

          <div>
            <label htmlFor="session-jwt" className="block text-sm font-medium text-text-primary mb-1.5">
              Session JWT (Optional)
            </label>
            <textarea
              id="session-jwt"
              value={sessionJwt}
              onChange={(event) => setSessionJwt(event.target.value)}
              placeholder="Paste UI session JWT token"
              autoComplete="off"
              rows={4}
              className="w-full rounded-lg border border-border bg-surface-secondary px-3 py-2.5 text-sm text-text-primary placeholder:text-text-muted focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
            />
            <p className="mt-1.5 text-xs text-text-muted">
              Feature-flagged path: when set, UI sends <code>Authorization: Bearer &lt;jwt&gt;</code> instead of API key.
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
