'use client';

import Link from 'next/link';
import { FormEvent, Suspense, useEffect, useState } from 'react';
import { useRouter, useSearchParams } from 'next/navigation';
import { AuthShell } from '@/components/auth/AuthShell';
import {
  exchangeOAuthCodeForSessionIfPresent,
  getSupabaseAccessToken,
  signOutSupabase,
  updatePassword,
} from '@/lib/auth/supabase';

function ResetPasswordContent() {
  const router = useRouter();
  const searchParams = useSearchParams();

  const [password, setPassword] = useState('');
  const [confirmPassword, setConfirmPassword] = useState('');
  const [loading, setLoading] = useState(false);
  const [initializing, setInitializing] = useState(true);
  const [linkValid, setLinkValid] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;

    const initialize = async () => {
      setInitializing(true);
      setError(null);
      try {
        await exchangeOAuthCodeForSessionIfPresent(searchParams);
        const accessToken = await getSupabaseAccessToken();
        if (!accessToken) {
          throw new Error('Invalid or expired reset link. Request a new password reset email.');
        }
        if (!cancelled) {
          setLinkValid(true);
        }
      } catch (err) {
        if (!cancelled) {
          setLinkValid(false);
          setError(err instanceof Error ? err.message : 'Invalid reset link.');
        }
      } finally {
        if (!cancelled) {
          setInitializing(false);
        }
      }
    };

    void initialize();
    return () => {
      cancelled = true;
    };
  }, [searchParams]);

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();

    if (password.length < 8) {
      setError('Password must be at least 8 characters.');
      return;
    }
    if (password !== confirmPassword) {
      setError('Passwords do not match.');
      return;
    }

    setLoading(true);
    setError(null);
    setNotice(null);
    try {
      await updatePassword(password);
      await signOutSupabase();
      router.replace('/login?reset=success');
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Unable to update password.');
    } finally {
      setLoading(false);
    }
  };

  return (
    <AuthShell mode="login">
      <div className="space-y-4">
        <h1 className="text-xl font-semibold text-text-primary">Reset password</h1>
        <p className="text-sm text-text-secondary">
          Enter a new password for your account.
        </p>

        {initializing && (
          <div className="rounded-lg border border-border bg-surface-secondary px-3 py-2 text-sm text-text-secondary">
            Verifying reset link...
          </div>
        )}

        {!initializing && error && (
          <div className="rounded-lg border border-danger/30 bg-danger-subtle px-3 py-2 text-sm text-danger">
            {error}
          </div>
        )}

        {!initializing && linkValid && (
          <form onSubmit={handleSubmit} className="space-y-4">
            <div>
              <label htmlFor="password" className="mb-1.5 block text-sm font-medium text-text-secondary">
                New password
              </label>
              <input
                id="password"
                type="password"
                value={password}
                onChange={(event) => setPassword(event.target.value)}
                autoComplete="new-password"
                className="w-full rounded-lg border border-border bg-surface-secondary px-3.5 py-2.5 text-sm text-text-primary placeholder:text-text-muted focus:border-accent focus:outline-none focus:ring-2 focus:ring-accent/20"
                placeholder="At least 8 characters"
              />
            </div>

            <div>
              <label htmlFor="confirm-password" className="mb-1.5 block text-sm font-medium text-text-secondary">
                Confirm password
              </label>
              <input
                id="confirm-password"
                type="password"
                value={confirmPassword}
                onChange={(event) => setConfirmPassword(event.target.value)}
                autoComplete="new-password"
                className="w-full rounded-lg border border-border bg-surface-secondary px-3.5 py-2.5 text-sm text-text-primary placeholder:text-text-muted focus:border-accent focus:outline-none focus:ring-2 focus:ring-accent/20"
                placeholder="Re-enter password"
              />
            </div>

            {notice && (
              <div className="rounded-lg border border-success/30 bg-success/10 px-3 py-2 text-sm text-success">
                {notice}
              </div>
            )}

            <button
              type="submit"
              disabled={loading}
              className="w-full rounded-lg bg-gradient-to-br from-accent to-accent-hover py-2.5 text-sm font-semibold text-white transition-opacity hover:opacity-95 disabled:cursor-not-allowed disabled:opacity-60"
            >
              {loading ? 'Updating...' : 'Update password'}
            </button>
          </form>
        )}

        {!initializing && !linkValid && (
          <p className="text-sm text-text-secondary">
            Return to{' '}
            <Link href="/login" className="font-medium text-accent hover:underline">
              sign in
            </Link>{' '}
            and request a new reset email.
          </p>
        )}
      </div>
    </AuthShell>
  );
}

export default function ResetPasswordPage() {
  return (
    <Suspense
      fallback={
        <main className="min-h-screen bg-background flex items-center justify-center px-4 py-10">
          <div className="rounded-lg border border-border bg-surface px-4 py-3 text-sm text-text-secondary">
            Loading reset flow...
          </div>
        </main>
      }
    >
      <ResetPasswordContent />
    </Suspense>
  );
}
