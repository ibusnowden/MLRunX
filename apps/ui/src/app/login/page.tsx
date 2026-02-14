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
} from '@/lib/auth/supabase';

function LoginContent() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const nextPath = useMemo(() => sanitizeNextPath(searchParams.get('next'), '/onboarding'), [searchParams]);

  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [rememberMe, setRememberMe] = useState(true);
  const [loading, setLoading] = useState(false);
  const [resetLoading, setResetLoading] = useState(false);
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
                disabled={resetLoading || loading}
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
              disabled={loading}
              className="w-full rounded-lg bg-gradient-to-br from-accent to-accent-hover py-2.5 text-sm font-semibold text-white transition-opacity hover:opacity-95 disabled:cursor-not-allowed disabled:opacity-60"
            >
              {loading ? 'Signing in...' : 'Sign In'}
            </button>
          </form>
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
