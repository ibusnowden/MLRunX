import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { API_KEY_STORAGE_KEY, API_URL_STORAGE_KEY, UI_CSRF_COOKIE_NAME, api } from './api';

function jsonResponse(payload: unknown, status = 200): Response {
  return new Response(JSON.stringify(payload), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });
}

function headersToRecord(headers: HeadersInit | undefined): Record<string, string> {
  if (!headers) return {};
  if (headers instanceof Headers) {
    return Object.fromEntries(headers.entries());
  }
  if (Array.isArray(headers)) {
    return Object.fromEntries(headers);
  }
  return headers;
}

describe('api key management client', () => {
  beforeEach(() => {
    window.localStorage.clear();
    document.cookie = `${UI_CSRF_COOKIE_NAME}=; Max-Age=0; Path=/`;
    vi.restoreAllMocks();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('lists API keys via UI-session auth without sending X-API-Key', async () => {
    window.localStorage.setItem(API_URL_STORAGE_KEY, 'https://mlrunx.ibra-niang.com');
    window.localStorage.setItem(API_KEY_STORAGE_KEY, 'mlrunx_local_key');

    const fetchMock = vi
      .spyOn(globalThis, 'fetch')
      .mockResolvedValue(jsonResponse({ keys: [] }));

    const result = await api.listApiKeys();
    expect(result.keys).toEqual([]);

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, options] = fetchMock.mock.calls[0] as [string, RequestInit];
    const headers = headersToRecord(options.headers);

    expect(url).toBe('https://mlrunx.ibra-niang.com/api/v1/keys');
    expect(options.credentials).toBe('include');
    expect(headers['Content-Type']).toBe('application/json');
    expect(headers['X-API-Key']).toBeUndefined();
  });

  it('creates API key with csrf token for mutating requests', async () => {
    window.localStorage.setItem(API_URL_STORAGE_KEY, 'https://mlrunx.ibra-niang.com');
    window.localStorage.setItem(API_KEY_STORAGE_KEY, 'mlrunx_local_key');
    document.cookie = `${UI_CSRF_COOKIE_NAME}=csrf-token-123; Path=/`;

    const fetchMock = vi.spyOn(globalThis, 'fetch').mockResolvedValue(
      jsonResponse({
        api_key: 'mlrunx_abc123',
        key_id: 'k-1',
        key_prefix: 'mlrunx_a',
        project_id: 'project-a',
        name: 'sdk-write',
        scopes: ['read', 'write'],
      })
    );

    const created = await api.createApiKey({
      project_id: 'project-a',
      name: 'sdk-write',
      scopes: ['read', 'write'],
    });

    expect(created.key_id).toBe('k-1');
    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, options] = fetchMock.mock.calls[0] as [string, RequestInit];
    const headers = headersToRecord(options.headers);

    expect(url).toBe('https://mlrunx.ibra-niang.com/api/v1/keys');
    expect(options.method).toBe('POST');
    expect(options.credentials).toBe('include');
    expect(headers['X-API-Key']).toBeUndefined();
    expect(headers['X-CSRF-Token']).toBe('csrf-token-123');
    expect(options.body).toBe(
      JSON.stringify({
        project_id: 'project-a',
        name: 'sdk-write',
        scopes: ['read', 'write'],
      })
    );
  });

  it('revokes API key using encoded key id path', async () => {
    window.localStorage.setItem(API_URL_STORAGE_KEY, 'https://mlrunx.ibra-niang.com');

    const fetchMock = vi
      .spyOn(globalThis, 'fetch')
      .mockResolvedValue(jsonResponse({ status: 'ok', revoked: 'abc/123' }));

    await api.revokeApiKey('abc/123');

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, options] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(url).toBe('https://mlrunx.ibra-niang.com/api/v1/keys/abc%2F123');
    expect(options.method).toBe('DELETE');
    expect(options.credentials).toBe('include');
  });
});
