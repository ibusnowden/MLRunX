'use client';

import { useEffect } from 'react';

export default function Error({
  error,
  reset,
}: {
  error: Error & { digest?: string };
  reset: () => void;
}) {
  useEffect(() => {
    console.error('UI route error boundary caught:', error);
  }, [error]);

  return (
    <main className="min-h-screen flex items-center justify-center p-6">
      <div className="max-w-lg w-full rounded-xl border border-border bg-surface p-6">
        <h1 className="text-xl font-semibold text-text-primary">Something went wrong</h1>
        <p className="mt-2 text-sm text-text-secondary">
          The page failed to render. Try again or refresh the browser.
        </p>
        <div className="mt-4 flex items-center gap-3">
          <button
            type="button"
            onClick={reset}
            className="px-4 py-2 rounded-lg bg-accent text-white text-sm font-medium hover:bg-accent-hover transition-colors"
          >
            Retry
          </button>
          <a
            href="/"
            className="px-4 py-2 rounded-lg border border-border text-sm font-medium text-text-secondary hover:bg-surface-secondary hover:text-text-primary transition-colors"
          >
            Go Home
          </a>
        </div>
      </div>
    </main>
  );
}
