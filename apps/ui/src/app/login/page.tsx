'use client';

import Link from 'next/link';
import { FormEvent, Suspense, useMemo, useState } from 'react';
import { useRouter, useSearchParams } from 'next/navigation';
import { sanitizeNextPath } from '@/lib/auth/routes';
import { exchangeProviderTokenForUiSession } from '@/lib/auth/session';
import {
  isSupabaseAuthConfigured,
  signInWithEmailPassword,
  signInWithOAuth,
  type OAuthProvider,
} from '@/lib/auth/supabase';

const OAUTH_PROVIDERS: Array<{ provider: OAuthProvider; label: string }> = [
  { provider: 'github', label: 'GitHub' },
  { provider: 'google', label: 'Google' },
  { provider: 'azure', label: 'Microsoft' },
];

function LoginContent() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const nextPath = useMemo(() => sanitizeNextPath(searchParams.get('next'), '/onboarding'), [searchParams]);

  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [loading, setLoading] = useState(false);
  const [oauthLoading, setOauthLoading] = useState<OAuthProvider | null>(null);
  const [error, setError] = useState<string | null>(null);

  const configured = isSupabaseAuthConfigured();

  const handleEmailLogin = async (event: FormEvent) => {
    event.preventDefault();
    if (!email.trim() || !password.trim()) {
      setError('Email and password are required.');
      return;
    }

    setLoading(true);
    setError(null);
    try {
      await signInWithEmailPassword(email.trim(), password);
      await exchangeProviderTokenForUiSession();
      router.replace(nextPath);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Login failed.');
    } finally {
      setLoading(false);
    }
  };

  const handleOAuthLogin = async (provider: OAuthProvider) => {
    setOauthLoading(provider);
    setError(null);
    try {
      const redirectTo = `${window.location.origin}/auth/callback?next=${encodeURIComponent(nextPath)}`;
      await signInWithOAuth(provider, redirectTo);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'OAuth login failed.');
      setOauthLoading(null);
    }
  };

  return (
    <main className="min-h-screen bg-background flex items-center justify-center px-4 py-10">
      <div className="w-full max-w-lg rounded-xl border border-border bg-surface p-6 sm:p-8">
        <h1 className="text-2xl font-bold text-text-primary">Log in to your account</h1>
        <p className="mt-2 text-sm text-text-secondary">
          Sign in with email/password or OAuth, then mint API keys from Access Console.
        </p>

        {!configured ? (
          <div className="mt-6 rounded-lg border border-warning/30 bg-warning/10 p-4 text-sm text-warning">
            Supabase auth is not configured. Set <code>NEXT_PUBLIC_SUPABASE_URL</code> and{' '}
            <code>NEXT_PUBLIC_SUPABASE_ANON_KEY</code> for login/signup.
          </div>
        ) : (
          <>
            <div className="mt-6 grid grid-cols-1 sm:grid-cols-3 gap-2">
              {OAUTH_PROVIDERS.map(({ provider, label }) => (
                <button
                  key={provider}
                  type="button"
                  onClick={() => void handleOAuthLogin(provider)}
                  disabled={Boolean(oauthLoading) || loading}
                  className="rounded-lg border border-border bg-surface-secondary px-3 py-2 text-sm font-medium text-text-primary hover:bg-surface-hover disabled:opacity-60 disabled:cursor-not-allowed"
                >
                  {oauthLoading === provider ? `Redirecting...` : label}
                </button>
              ))}
            </div>

            <div className="my-6 flex items-center gap-3 text-xs text-text-muted">
              <div className="h-px flex-1 bg-border" />
              <span>OR USE EMAIL</span>
              <div className="h-px flex-1 bg-border" />
            </div>

            <form onSubmit={handleEmailLogin} className="space-y-4">
              <div>
                <label htmlFor="email" className="block text-sm font-medium text-text-primary mb-1.5">
                  Email
                </label>
                <input
                  id="email"
                  type="email"
                  value={email}
                  onChange={(event) => setEmail(event.target.value)}
                  autoComplete="email"
                  className="w-full rounded-lg border border-border bg-surface-secondary px-3 py-2.5 text-sm text-text-primary placeholder:text-text-muted focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
                  placeholder="you@company.com"
                />
              </div>

              <div>
                <label htmlFor="password" className="block text-sm font-medium text-text-primary mb-1.5">
                  Password
                </label>
                <input
                  id="password"
                  type="password"
                  value={password}
                  onChange={(event) => setPassword(event.target.value)}
                  autoComplete="current-password"
                  className="w-full rounded-lg border border-border bg-surface-secondary px-3 py-2.5 text-sm text-text-primary placeholder:text-text-muted focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
                  placeholder="Enter password"
                />
              </div>

              {error && (
                <div className="rounded-lg border border-danger/30 bg-danger-subtle px-3 py-2 text-sm text-danger">
                  {error}
                </div>
              )}

              <button
                type="submit"
                disabled={loading || Boolean(oauthLoading)}
                className="w-full rounded-lg bg-accent text-white py-2.5 text-sm font-semibold hover:bg-accent-hover disabled:opacity-60 disabled:cursor-not-allowed"
              >
                {loading ? 'Signing in...' : 'Log me in'}
              </button>
            </form>
          </>
        )}

        <p className="mt-6 text-sm text-text-secondary">
          Don&apos;t have an account?{' '}
          <Link href="/signup" className="text-accent font-medium hover:underline">
            Sign up
          </Link>
        </p>
      </div>
    </main>
  );
}

export default function LoginPage() {
  return (
    <Suspense
      fallback={
        <main className="min-h-screen bg-background flex items-center justify-center px-4 py-10">
          <div className="rounded-lg border border-border bg-surface px-4 py-3 text-sm text-text-secondary">
            Loading login...
          </div>
        </main>
      }
    >
      <LoginContent />
    </Suspense>
  );
}
