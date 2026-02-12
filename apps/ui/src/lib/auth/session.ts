import { api } from '@/lib/api';
import { getSupabaseAccessToken } from './supabase';

export async function exchangeProviderTokenForUiSession(): Promise<void> {
  const accessToken = await getSupabaseAccessToken();
  if (!accessToken) {
    throw new Error('No external auth session found. Please sign in again.');
  }
  await api.loginUiSessionWithBearer(accessToken);
}
