'use client';

import Link from 'next/link';
import { FormEvent, Suspense, useMemo, useState } from 'react';
import { useRouter, useSearchParams } from 'next/navigation';
import { AuthShell } from '@/components/auth/AuthShell';
import { sanitizeNextPath } from '@/lib/auth/routes';
import { exchangeProviderTokenForUiSession } from '@/lib/auth/session';
import {
  isSupabaseAuthConfigured,
  sendPasswordResetEmail,
  signInWithEmailPassword,
  signInWithOAuth,
  type OAuthProvider,
} from '@/lib/auth/supabase';

const OAUTH_PROVIDERS: Array<{ provider: OAuthProvider; label: string }> = [
  { provider: 'google', label: 'Google' },
];

const GoogleIcon = () => (
  <svg viewBox="0 0 24 24" className="h-4 w-4" aria-hidden="true">
    <path fill="#4285F4" d="M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92a5.06 5.06 0 01-2.2 3.32v2.77h3.57c2.08-1.92 3.28-4.74 3.28-8.1z" />
    <path fill="#34A853" d="M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.77c-.98.66-2.23 1.06-3.71 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84C3.99 20.53 7.7 23 12 23z" />
    <path fill="#FBBC05" d="M5.84 14.09c-.22-.66-.35-1.36-.35-2.09s.13-1.43.35-2.09V7.07H2.18C1.43 8.55 1 10.22 1 12s.43 3.45 1.18 4.93l2.85-2.22.81-.62z" />
    <path fill="#EA4335" d="M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1 7.7 1 3.99 3.47 2.18 7.07l3.66 2.84c.87-2.6 3.3-4.53 6.16-4.53z" />
  </svg>
);

function LoginContent() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const nextPath = useMemo(() => sanitizeNextPath(searchParams.get('next'), '/onboarding'), [searchParams]);

  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [rememberMe, setRememberMe] = useState(true);
  const [loading, setLoading] = useState(false);
  const [resetLoading, setResetLoading] = useState(false);
  const [oauthLoading, setOauthLoading] = useState<OAuthProvider | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const resetStatusNotice = useMemo(() => {
    const status = searchParams.get('reset');
    if (status === 'success') {
      return 'Password updated successfully. Sign in with your new password.';
    }
    return null;
  }, [searchParams]);

  const configured = isSupabaseAuthConfigured();

  const handleEmailLogin = async (event: FormEvent) => {
    event.preventDefault();
    if (!email.trim() || !password.trim()) {
      setError('Email and password are required.');
      return;
    }

    setLoading(true);
    setError(null);
    setNotice(null);
    try {
      await signInWithEmailPassword(email.trim(), password);
      await exchangeProviderTokenForUiSession();
      if (rememberMe) {
        // Supabase client already persists session; this toggle keeps UX explicit.
      }
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
    setNotice(null);
    try {
      const redirectTo = `${window.location.origin}/auth/callback?next=${encodeURIComponent(nextPath)}`;
      await signInWithOAuth(provider, redirectTo);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'OAuth login failed.');
      setOauthLoading(null);
    }
  };

  const handleForgotPassword = async () => {
    if (!email.trim()) {
      setError('Enter your email first, then click "Forgot password?".');
      return;
    }

    setResetLoading(true);
    setError(null);
    setNotice(null);
    try {
      const redirectTo = `${window.location.origin}/auth/reset`;
      await sendPasswordResetEmail(email.trim(), redirectTo);
      setNotice('Password reset email sent. Check your inbox and open the reset link.');
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to send password reset email.');
    } finally {
      setResetLoading(false);
    }
  };

  return (
    <AuthShell mode="login">
      {!configured ? (
        <div className="rounded-lg border border-warning/30 bg-warning/10 p-4 text-sm text-warning">
          Supabase auth is not configured. Set <code>NEXT_PUBLIC_SUPABASE_URL</code> and{' '}
          <code>NEXT_PUBLIC_SUPABASE_ANON_KEY</code> for login/signup.
        </div>
      ) : (
        <>
          <form onSubmit={handleEmailLogin} className="space-y-4">
            <div>
              <label htmlFor="email" className="mb-1.5 block text-sm font-medium text-text-secondary">
                Email
              </label>
              <input
                id="email"
                type="email"
                value={email}
                onChange={(event) => setEmail(event.target.value)}
                autoComplete="email"
                className="w-full rounded-lg border border-border bg-surface-secondary px-3.5 py-2.5 text-sm text-text-primary placeholder:text-text-muted focus:border-accent focus:outline-none focus:ring-2 focus:ring-accent/20"
                placeholder="you@company.com"
              />
            </div>

            <div>
              <label htmlFor="password" className="mb-1.5 block text-sm font-medium text-text-secondary">
                Password
              </label>
              <input
                id="password"
                type="password"
                value={password}
                onChange={(event) => setPassword(event.target.value)}
                autoComplete="current-password"
                className="w-full rounded-lg border border-border bg-surface-secondary px-3.5 py-2.5 text-sm text-text-primary placeholder:text-text-muted focus:border-accent focus:outline-none focus:ring-2 focus:ring-accent/20"
                placeholder="••••••••"
              />
            </div>

            <div className="flex items-center justify-between text-sm">
              <label className="inline-flex items-center gap-2 text-text-secondary">
                <input
                  type="checkbox"
                  checked={rememberMe}
                  onChange={(event) => setRememberMe(event.target.checked)}
                  className="h-4 w-4 rounded border-border bg-surface-secondary accent-[var(--accent)]"
                />
                Remember me
              </label>
              <button
                type="button"
                onClick={() => void handleForgotPassword()}
                disabled={resetLoading || loading || Boolean(oauthLoading)}
                className="text-text-muted transition-colors hover:text-accent disabled:cursor-not-allowed disabled:opacity-60"
              >
                {resetLoading ? 'Sending...' : 'Forgot password?'}
              </button>
            </div>

            {error && (
              <div className="rounded-lg border border-danger/30 bg-danger-subtle px-3 py-2 text-sm text-danger">
                {error}
              </div>
            )}

            {notice && (
              <div className="rounded-lg border border-warning/30 bg-warning/10 px-3 py-2 text-sm text-warning">
                {notice}
              </div>
            )}

            {!notice && resetStatusNotice && (
              <div className="rounded-lg border border-success/30 bg-success/10 px-3 py-2 text-sm text-success">
                {resetStatusNotice}
              </div>
            )}

            <button
              type="submit"
              disabled={loading || Boolean(oauthLoading)}
              className="w-full rounded-lg bg-gradient-to-br from-accent to-accent-hover py-2.5 text-sm font-semibold text-white transition-opacity hover:opacity-95 disabled:cursor-not-allowed disabled:opacity-60"
            >
              {loading ? 'Signing in...' : 'Sign In'}
            </button>
          </form>

          <div className="my-6 flex items-center gap-3 text-xs text-text-muted">
            <div className="h-px flex-1 bg-border" />
            <span>or continue with</span>
            <div className="h-px flex-1 bg-border" />
          </div>

          <div className="grid grid-cols-1 gap-2">
            {OAUTH_PROVIDERS.map(({ provider, label }) => (
              <button
                key={provider}
                type="button"
                onClick={() => void handleOAuthLogin(provider)}
                disabled={Boolean(oauthLoading) || loading}
                className="inline-flex items-center justify-center gap-2 rounded-lg border border-border bg-surface-secondary px-3 py-2.5 text-sm font-medium text-text-primary transition-colors hover:bg-surface-hover disabled:cursor-not-allowed disabled:opacity-60"
              >
                <GoogleIcon />
                {oauthLoading === provider ? 'Redirecting...' : label}
              </button>
            ))}
          </div>
        </>
      )}

      <p className="mt-6 text-sm text-text-secondary">
        Don&apos;t have an account?{' '}
        <Link href="/signup" className="font-medium text-accent hover:underline">
          Create one
        </Link>
      </p>
    </AuthShell>
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
