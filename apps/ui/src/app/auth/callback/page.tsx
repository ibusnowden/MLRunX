'use client';

import { Suspense, useEffect, useMemo, useState } from 'react';
import { useRouter, useSearchParams } from 'next/navigation';
import { sanitizeNextPath } from '@/lib/auth/routes';
import { exchangeProviderTokenForUiSession } from '@/lib/auth/session';
import { exchangeOAuthCodeForSessionIfPresent } from '@/lib/auth/supabase';

function AuthCallbackContent() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const nextPath = useMemo(() => sanitizeNextPath(searchParams.get('next'), '/settings'), [searchParams]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;

    const finalize = async () => {
      try {
        await exchangeOAuthCodeForSessionIfPresent(searchParams);
        await exchangeProviderTokenForUiSession();
        if (!cancelled) {
          router.replace(nextPath);
        }
      } catch (err) {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : 'Auth callback failed.');
        }
      }
    };

    void finalize();

    return () => {
      cancelled = true;
    };
  }, [nextPath, router, searchParams]);

  return (
    <main className="min-h-screen bg-background flex items-center justify-center px-4 py-10">
      <div className="w-full max-w-md rounded-xl border border-border bg-surface p-6 text-center">
        <h1 className="text-xl font-semibold text-text-primary">Completing sign-in</h1>
        {!error ? (
          <p className="mt-2 text-sm text-text-secondary">Please wait while we create your UI session.</p>
        ) : (
          <p className="mt-3 text-sm text-danger">{error}</p>
        )}
      </div>
    </main>
  );
}

export default function AuthCallbackPage() {
  return (
    <Suspense
      fallback={
        <main className="min-h-screen bg-background flex items-center justify-center px-4 py-10">
          <div className="rounded-lg border border-border bg-surface px-4 py-3 text-sm text-text-secondary">
            Finalizing sign-in...
          </div>
        </main>
      }
    >
      <AuthCallbackContent />
    </Suspense>
  );
}
