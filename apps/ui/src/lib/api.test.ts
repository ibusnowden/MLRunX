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

function ensureLocalStorageMock(): void {
  const localStorageValue = window.localStorage as Partial<Storage>;
  if (
    typeof localStorageValue.getItem === 'function' &&
    typeof localStorageValue.setItem === 'function' &&
    typeof localStorageValue.removeItem === 'function' &&
    typeof localStorageValue.clear === 'function'
  ) {
    return;
  }

  const store = new Map<string, string>();
  const mockStorage: Storage = {
    get length() {
      return store.size;
    },
    clear() {
      store.clear();
    },
    getItem(key: string) {
      return store.get(key) ?? null;
    },
    key(index: number) {
      return Array.from(store.keys())[index] ?? null;
    },
    removeItem(key: string) {
      store.delete(key);
    },
    setItem(key: string, value: string) {
      store.set(key, value);
    },
  };

  Object.defineProperty(window, 'localStorage', {
    value: mockStorage,
    configurable: true,
  });
}

describe('api key management client', () => {
  beforeEach(() => {
    ensureLocalStorageMock();
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

  it('logs in UI session with bearer token without API key header', async () => {
    window.localStorage.setItem(API_URL_STORAGE_KEY, 'https://mlrunx.ibra-niang.com');
    window.localStorage.setItem(API_KEY_STORAGE_KEY, 'mlrunx_local_key');

    const fetchMock = vi.spyOn(globalThis, 'fetch').mockResolvedValue(
      jsonResponse({
        status: 'ok',
        user_id: 'user-1',
        expires_at: '2026-02-12 00:00:00',
        project_count: 1,
      })
    );

    const result = await api.loginUiSessionWithBearer('provider-jwt-token');
    expect(result.status).toBe('ok');

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, options] = fetchMock.mock.calls[0] as [string, RequestInit];
    const headers = headersToRecord(options.headers);

    expect(url).toBe('https://mlrunx.ibra-niang.com/api/v1/ui-auth/login');
    expect(options.method).toBe('POST');
    expect(options.credentials).toBe('include');
    expect(headers['Authorization']).toBe('Bearer provider-jwt-token');
    expect(headers['X-API-Key']).toBeUndefined();
  });

  it('lists admin audit events with encoded query filters', async () => {
    window.localStorage.setItem(API_URL_STORAGE_KEY, 'https://mlrunx.ibra-niang.com');
    window.localStorage.setItem(API_KEY_STORAGE_KEY, 'mlrunx_platform_admin_key');

    const fetchMock = vi
      .spyOn(globalThis, 'fetch')
      .mockResolvedValue(jsonResponse({ events: [] }));

    await api.listAdminAuditEvents({
      action: 'admin.users.list',
      userId: 'user/123',
      limit: 50,
    });

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, options] = fetchMock.mock.calls[0] as [string, RequestInit];
    const headers = headersToRecord(options.headers);

    expect(url).toBe(
      'https://mlrunx.ibra-niang.com/api/v1/admin/audit-events?user_id=user%2F123&action=admin.users.list&limit=50'
    );
    expect(headers['X-API-Key']).toBe('mlrunx_platform_admin_key');
    expect(options.credentials).toBe('same-origin');
  });

  it('prefers UI-session auth for admin endpoints when csrf cookie exists', async () => {
    window.localStorage.setItem(API_URL_STORAGE_KEY, 'https://mlrunx.ibra-niang.com');
    window.localStorage.setItem(API_KEY_STORAGE_KEY, 'mlrunx_platform_admin_key');
    document.cookie = `${UI_CSRF_COOKIE_NAME}=csrf-admin; Path=/`;

    const fetchMock = vi
      .spyOn(globalThis, 'fetch')
      .mockResolvedValue(jsonResponse({ events: [] }));

    await api.listAdminAuditEvents({ action: 'admin.users.list' });

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, options] = fetchMock.mock.calls[0] as [string, RequestInit];
    const headers = headersToRecord(options.headers);

    expect(url).toBe(
      'https://mlrunx.ibra-niang.com/api/v1/admin/audit-events?action=admin.users.list'
    );
    expect(options.credentials).toBe('include');
    expect(headers['X-API-Key']).toBeUndefined();
  });

  it('lists projects using UI-session auth', async () => {
    window.localStorage.setItem(API_URL_STORAGE_KEY, 'https://mlrunx.ibra-niang.com');
    window.localStorage.setItem(API_KEY_STORAGE_KEY, 'mlrunx_local_key');

    const fetchMock = vi
      .spyOn(globalThis, 'fetch')
      .mockResolvedValue(jsonResponse({ projects: [] }));

    const result = await api.listProjects();
    expect(result.projects).toEqual([]);

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, options] = fetchMock.mock.calls[0] as [string, RequestInit];
    const headers = headersToRecord(options.headers);

    expect(url).toBe('https://mlrunx.ibra-niang.com/api/v1/projects');
    expect(options.credentials).toBe('include');
    expect(headers['X-API-Key']).toBeUndefined();
  });

  it('creates and deletes projects using UI-session auth with csrf token', async () => {
    window.localStorage.setItem(API_URL_STORAGE_KEY, 'https://mlrunx.ibra-niang.com');
    window.localStorage.setItem(API_KEY_STORAGE_KEY, 'mlrunx_local_key');
    document.cookie = `${UI_CSRF_COOKIE_NAME}=csrf-token-xyz; Path=/`;

    const fetchMock = vi
      .spyOn(globalThis, 'fetch')
      .mockResolvedValueOnce(
        jsonResponse({
          project_id: 'project-123',
          name: 'workspace',
          description: null,
          created_at: '2026-02-13 00:00:00',
          updated_at: '2026-02-13 00:00:00',
        })
      )
      .mockResolvedValueOnce(jsonResponse({ status: 'ok' }));

    const created = await api.createProject({ name: 'workspace' });
    expect(created.project_id).toBe('project-123');

    await api.deleteProject('project/123');

    expect(fetchMock).toHaveBeenCalledTimes(2);
    const [createUrl, createOptions] = fetchMock.mock.calls[0] as [string, RequestInit];
    const [deleteUrl, deleteOptions] = fetchMock.mock.calls[1] as [string, RequestInit];
    const createHeaders = headersToRecord(createOptions.headers);
    const deleteHeaders = headersToRecord(deleteOptions.headers);

    expect(createUrl).toBe('https://mlrunx.ibra-niang.com/api/v1/projects');
    expect(createOptions.method).toBe('POST');
    expect(createOptions.credentials).toBe('include');
    expect(createHeaders['X-CSRF-Token']).toBe('csrf-token-xyz');
    expect(createHeaders['X-API-Key']).toBeUndefined();

    expect(deleteUrl).toBe('https://mlrunx.ibra-niang.com/api/v1/projects/project%2F123');
    expect(deleteOptions.method).toBe('DELETE');
    expect(deleteOptions.credentials).toBe('include');
    expect(deleteHeaders['X-CSRF-Token']).toBe('csrf-token-xyz');
    expect(deleteHeaders['X-API-Key']).toBeUndefined();
  });

  it('queries run events with incremental cursor parameters', async () => {
    window.localStorage.setItem(API_URL_STORAGE_KEY, 'https://mlrunx.ibra-niang.com');
    window.localStorage.setItem(API_KEY_STORAGE_KEY, 'mlrunx_local_key');

    const fetchMock = vi
      .spyOn(globalThis, 'fetch')
      .mockResolvedValue(
        jsonResponse({ run_id: 'run-1', events: [], next_after_id: null, has_more: false })
      );

    await api.getRunEvents('run-1', { afterId: 42, limit: 100 });

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, options] = fetchMock.mock.calls[0] as [string, RequestInit];
    const headers = headersToRecord(options.headers);

    expect(url).toBe('https://mlrunx.ibra-niang.com/api/v1/runs/run-1/events?after_id=42&limit=100');
    expect(options.credentials).toBe('same-origin');
    expect(headers['X-API-Key']).toBe('mlrunx_local_key');
  });
});
