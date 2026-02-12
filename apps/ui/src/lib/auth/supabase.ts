import { createClient, type Provider, type Session, type SupabaseClient } from '@supabase/supabase-js';

let browserClient: SupabaseClient | null = null;

function getSupabaseConfig() {
  const url = process.env.NEXT_PUBLIC_SUPABASE_URL?.trim() || '';
  const anonKey = process.env.NEXT_PUBLIC_SUPABASE_ANON_KEY?.trim() || '';
  return { url, anonKey };
}

export function isSupabaseAuthConfigured(): boolean {
  const { url, anonKey } = getSupabaseConfig();
  return Boolean(url && anonKey);
}

function requireSupabaseClient(): SupabaseClient {
  if (typeof window === 'undefined') {
    throw new Error('Supabase auth is only available in the browser.');
  }

  const { url, anonKey } = getSupabaseConfig();
  if (!url || !anonKey) {
    throw new Error(
      'Supabase auth is not configured. Set NEXT_PUBLIC_SUPABASE_URL and NEXT_PUBLIC_SUPABASE_ANON_KEY.'
    );
  }

  if (!browserClient) {
    browserClient = createClient(url, anonKey, {
      auth: {
        persistSession: true,
        autoRefreshToken: true,
        detectSessionInUrl: true,
      },
    });
  }

  return browserClient;
}

function throwIfSupabaseError(error: { message: string } | null): void {
  if (error) {
    throw new Error(error.message || 'Authentication request failed.');
  }
}

export type OAuthProvider = Extract<Provider, 'github' | 'google' | 'azure'>;

export async function signInWithEmailPassword(email: string, password: string): Promise<Session> {
  const { data, error } = await requireSupabaseClient().auth.signInWithPassword({
    email,
    password,
  });
  throwIfSupabaseError(error);
  if (!data.session) {
    throw new Error('No auth session returned. Check account confirmation requirements.');
  }
  return data.session;
}

export async function signUpWithEmailPassword(
  email: string,
  password: string,
  displayName?: string
): Promise<{ session: Session | null; emailConfirmationRequired: boolean }> {
  const { data, error } = await requireSupabaseClient().auth.signUp({
    email,
    password,
    options: {
      data: displayName?.trim() ? { display_name: displayName.trim() } : undefined,
    },
  });
  throwIfSupabaseError(error);
  return {
    session: data.session,
    emailConfirmationRequired: !data.session,
  };
}

export async function signInWithOAuth(
  provider: OAuthProvider,
  redirectTo: string
): Promise<void> {
  const { error } = await requireSupabaseClient().auth.signInWithOAuth({
    provider,
    options: {
      redirectTo,
    },
  });
  throwIfSupabaseError(error);
}

export async function exchangeOAuthCodeForSessionIfPresent(searchParams: URLSearchParams): Promise<void> {
  const code = searchParams.get('code');
  if (!code) return;
  const { error } = await requireSupabaseClient().auth.exchangeCodeForSession(code);
  throwIfSupabaseError(error);
}

export async function getSupabaseAccessToken(): Promise<string | null> {
  const { data, error } = await requireSupabaseClient().auth.getSession();
  throwIfSupabaseError(error);
  return data.session?.access_token ?? null;
}

export async function signOutSupabase(): Promise<void> {
  const { error } = await requireSupabaseClient().auth.signOut();
  throwIfSupabaseError(error);
}
