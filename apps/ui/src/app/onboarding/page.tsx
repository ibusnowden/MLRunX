'use client';

import Link from 'next/link';
import { useEffect, useMemo, useState } from 'react';
import { DEFAULT_API_URL, UiAuthSessionResult, api, getStoredApiConfig } from '@/lib/api';

export default function OnboardingPage() {
  const [session, setSession] = useState<UiAuthSessionResult | null>(null);
  const [apiBaseUrl, setApiBaseUrl] = useState(DEFAULT_API_URL);
  const [apiKey, setApiKey] = useState('');
  const [loadingSession, setLoadingSession] = useState(false);
  const [status, setStatus] = useState<string | null>(null);

  const refreshSession = async () => {
    setLoadingSession(true);
    try {
      const result = await api.getUiSession();
      setSession(result);
      setStatus(null);
    } catch {
      setSession(null);
      setStatus('No active UI session. Log in again to continue onboarding.');
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

  const effectiveApiKey = useMemo(() => {
    if (apiKey.trim()) return apiKey.trim();
    return '<paste-api-key>';
  }, [apiKey]);

  const projectHint = useMemo(() => {
    if (!session) return '<your-project-id>';
    if (session.project_ids.length === 1) return session.project_ids[0];
    return '<choose-project-id-from-access-console>';
  }, [session]);

  return (
    <main className="min-h-screen">
      <div className="border-b border-border bg-surface">
        <div className="max-w-5xl mx-auto px-4 sm:px-6 py-5 sm:py-7">
          <h1 className="text-2xl font-bold text-text-primary">Getting Started</h1>
          <p className="text-sm text-text-secondary mt-1">
            Create your key, configure the SDK, send a first run, and return here to monitor plots.
          </p>
        </div>
      </div>

      <div className="max-w-5xl mx-auto px-4 sm:px-6 py-5 sm:py-7 space-y-5">
        <section className="rounded-xl border border-border bg-surface p-4 sm:p-6">
          <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3">
            <div>
              <h2 className="text-base font-semibold text-text-primary">1. Verify your UI session</h2>
              <p className="text-xs text-text-muted mt-1">
                Session auth is required for dashboard access and API key self-service.
              </p>
            </div>
            <button
              type="button"
              onClick={() => void refreshSession()}
              className="px-4 py-2.5 rounded-lg border border-border text-sm font-medium text-text-secondary hover:bg-surface-secondary hover:text-text-primary transition-colors"
            >
              {loadingSession ? 'Checking...' : 'Refresh Session'}
            </button>
          </div>

          <p className="mt-3 text-sm text-text-secondary">
            {session
              ? `Authenticated with scopes [${session.scopes.join(', ')}] across ${session.project_ids.length} project(s).`
              : status || 'No active session.'}
          </p>
        </section>

        <section className="rounded-xl border border-border bg-surface p-4 sm:p-6">
          <h2 className="text-base font-semibold text-text-primary">2. Create your API key</h2>
          <p className="text-xs text-text-muted mt-1">
            Keys are project-scoped. Create a read/write key in Access Console and keep it private.
          </p>
          <div className="mt-4 flex flex-wrap gap-2">
            <Link
              href="/settings"
              className="inline-flex items-center justify-center px-4 py-2.5 rounded-lg bg-accent text-white text-sm font-medium hover:bg-accent-hover transition-colors"
            >
              Open Access Console
            </Link>
            <Link
              href="/"
              className="inline-flex items-center justify-center px-4 py-2.5 rounded-lg border border-border text-sm font-medium text-text-secondary hover:bg-surface-secondary hover:text-text-primary transition-colors"
            >
              Open Dashboard
            </Link>
          </div>
        </section>

        <section className="rounded-xl border border-border bg-surface p-4 sm:p-6 space-y-3">
          <h2 className="text-base font-semibold text-text-primary">3. Configure SDK</h2>
          <p className="text-xs text-text-muted">
            Use your public host URL (root, not <code>/api</code>) and your newly created API key.
          </p>
          <pre className="overflow-x-auto rounded-lg border border-border bg-surface-secondary p-3 text-xs sm:text-sm text-text-primary">
{`pip install --upgrade mlrunx
export MLRUNX_SERVER_URL=${apiBaseUrl}
export MLRUNX_API_KEY=${effectiveApiKey}
export MLRUNX_PROJECT_ID=${projectHint}`}
          </pre>
          <pre className="overflow-x-auto rounded-lg border border-border bg-surface-secondary p-3 text-xs sm:text-sm text-text-primary">
{`uv pip install --upgrade mlrunx
MLRUNX_SERVER_URL=${apiBaseUrl} MLRUNX_API_KEY=${effectiveApiKey} MLRUNX_PROJECT_ID=${projectHint} python train.py`}
          </pre>
        </section>

        <section className="rounded-xl border border-border bg-surface p-4 sm:p-6 space-y-3">
          <h2 className="text-base font-semibold text-text-primary">4. Quick verification run</h2>
          <p className="text-xs text-text-muted">
            Run this once from your local machine, then refresh Home to confirm your run appears.
          </p>
          <pre className="overflow-x-auto rounded-lg border border-border bg-surface-secondary p-3 text-xs sm:text-sm text-text-primary">
{`python - <<'PY'
import inspect
import mlrunx

if "project_id" not in inspect.signature(mlrunx.init).parameters:
    raise SystemExit(
        "Incompatible mlrunx build detected. "
        "Run: pip install --upgrade "
        "'mlrunx @ git+https://github.com/ibusnowden/MLRunX.git#subdirectory=sdks/python'"
    )

run = mlrunx.init(project_id="${projectHint}", name="onboarding-smoke")
mlrunx.log({"loss": 0.42, "accuracy": 0.91}, step=1)
mlrunx.finish()
print("run_id:", run.run_id)
PY`}
          </pre>
        </section>
      </div>
    </main>
  );
}
