/**
 * MLRunX API Client
 *
 * Provides type-safe API calls to the MLRunX backend.
 */

const DEFAULT_API_BASE_URL = process.env.NEXT_PUBLIC_API_URL || 'http://localhost:3001';
const SERVER_API_BASE_URL = process.env.MLRUNX_API_URL || DEFAULT_API_BASE_URL;

export const API_KEY_STORAGE_KEY = 'mlrunx_api_key';
export const API_URL_STORAGE_KEY = 'mlrunx_api_url';
export const UI_CSRF_COOKIE_NAME = 'mlrunx_ui_csrf';
export const DEFAULT_API_URL = DEFAULT_API_BASE_URL;

function envFlagEnabled(value: string | undefined): boolean {
  if (!value) return false;
  return value === '1' || value.toLowerCase() === 'true' || value.toLowerCase() === 'yes';
}

function localApiKeyStorageEnabled(): boolean {
  return envFlagEnabled(process.env.NEXT_PUBLIC_MLRUNX_ALLOW_LOCAL_STORAGE);
}

export interface Run {
  run_id: string;
  project_id: string;
  name: string | null;
  status: 'running' | 'finished' | 'failed' | 'killed' | 'pending';
  metrics_count: number;
  params_count: number;
  tags: Record<string, string>;
  created_at: string;
  updated_at: string;
  duration_seconds: number | null;
}

export interface ListRunsResponse {
  runs: Run[];
  total: number;
  limit: number;
  offset: number;
}

export interface ListRunsParams {
  project?: string;
  status?: string;
  query?: string;
  tags?: string[];
  filter?: string;
  sortBy?: 'created_at' | 'updated_at' | 'name' | 'status' | 'duration_seconds' | 'metrics_count' | 'params_count';
  sortOrder?: 'asc' | 'desc';
  limit?: number;
  offset?: number;
}

export interface MetricPoint {
  step: number;
  mean: number;
  min: number;
  max: number;
  count: number;
}

export interface MetricSeries {
  name: string;
  points: MetricPoint[];
  total_points: number;
  downsampled: boolean;
}

export interface MetricsResponse {
  run_id: string;
  series: MetricSeries[];
  available_metrics: string[];
  metric_aliases?: Record<string, string>;
}

export interface RunDetail extends Run {
  metrics_summary: Array<{
    name: string;
    last_value: number;
    last_step: number;
  }>;
}

export interface RunEvent {
  id: number;
  run_id: string;
  level: 'debug' | 'info' | 'warn' | 'error' | string;
  source: string;
  message: string;
  step: number | null;
  timestamp: number | null;
  created_at: string;
}

export interface RunEventsResponse {
  run_id: string;
  events: RunEvent[];
  next_after_id: number | null;
  has_more: boolean;
}

export class ApiError extends Error {
  constructor(
    public status: number,
    message: string
  ) {
    super(message);
    this.name = 'ApiError';
  }
}

export interface StoredApiConfig {
  apiBaseUrl: string;
  apiKey: string;
}

export interface UiAuthLoginResult {
  status: string;
  user_id: string;
  expires_at: string;
  project_count: number;
}

export interface UiAuthSessionResult {
  authenticated: boolean;
  auth_mode: string;
  scopes: string[];
  project_ids: string[];
  key_prefix: string;
  is_dev_mode: boolean;
  is_platform_admin: boolean;
  ui_session_ttl_seconds: number;
  ui_key_max_ttl_seconds: number;
}

export interface CreateApiKeyRequest {
  project_id?: string;
  name?: string;
  scopes: string[];
  expires_in_seconds?: number;
}

export interface CreateApiKeyResponse {
  api_key: string;
  key_id: string;
  key_prefix: string;
  project_id: string | null;
  name: string | null;
  scopes: string[];
  expires_at: string | null;
}

export interface ApiKeyInfo {
  key_id: string;
  key_prefix: string;
  project_id: string | null;
  name: string | null;
  scopes: string[];
  created_at: string;
  last_used_at: string | null;
  is_revoked: boolean;
}

export interface ListApiKeysResponse {
  keys: ApiKeyInfo[];
}

export interface ProjectInfo {
  project_id: string;
  name: string;
  description: string | null;
  created_at: string;
  updated_at: string;
}

export interface ListProjectsResponse {
  projects: ProjectInfo[];
}

export interface MetricAlias {
  project_id: string;
  raw_name: string;
  // Backward-compatible field returned by older API payloads.
  metric_name: string;
  display_name: string;
  unit?: string | null;
  description?: string | null;
  is_active?: boolean;
  created_at: string;
  updated_at: string;
}

export interface ListMetricAliasesResponse {
  project_id: string;
  aliases: MetricAlias[];
}

export interface AdminUser {
  user_id: string;
  email: string | null;
  display_name: string | null;
  auth_provider: string;
  external_subject: string | null;
  is_service_account: boolean;
  disabled: boolean;
  active_project_count: number;
  active_session_count: number;
  created_at: string;
  updated_at: string;
}

export interface AdminListUsersResponse {
  users: AdminUser[];
}

export interface AdminUserMembership {
  project_id: string;
  project_name: string;
  role: string;
  granted_by_user_id: string | null;
  created_at: string;
  revoked_at: string | null;
}

export interface AdminListUserMembershipsResponse {
  user_id: string;
  memberships: AdminUserMembership[];
}

export interface AdminSession {
  session_id: string;
  user_id: string;
  created_at: string;
  last_seen_at: string | null;
  expires_at: string;
  revoked_at: string | null;
  client_ip: string | null;
  user_agent: string | null;
}

export interface AdminListSessionsResponse {
  sessions: AdminSession[];
}

export interface AdminAuditEvent {
  id: number;
  occurred_at: string;
  actor_user_id: string | null;
  actor_key_id: string | null;
  project_id: string | null;
  run_id: string | null;
  action: string;
  resource_type: string;
  resource_id: string | null;
  outcome: string;
  request_id: string | null;
  client_ip: string | null;
  user_agent: string | null;
  metadata: unknown;
}

export interface AdminListAuditEventsResponse {
  events: AdminAuditEvent[];
}

function normalizeApiBaseUrl(value: string): string {
  const trimmed = value.trim();
  if (!trimmed) return DEFAULT_API_BASE_URL;
  return trimmed.endsWith('/') ? trimmed.slice(0, -1) : trimmed;
}

function readStorage(key: string): string | null {
  if (typeof window === 'undefined') return null;
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}

function writeStorage(key: string, value: string) {
  if (typeof window === 'undefined') return;
  try {
    window.localStorage.setItem(key, value);
  } catch {
    // Ignore storage failures in restricted environments.
  }
}

function removeStorage(key: string) {
  if (typeof window === 'undefined') return;
  try {
    window.localStorage.removeItem(key);
  } catch {
    // Ignore storage failures in restricted environments.
  }
}

export function getStoredApiConfig(): StoredApiConfig {
  const storedBaseUrl = readStorage(API_URL_STORAGE_KEY);
  const storedApiKey = localApiKeyStorageEnabled() ? readStorage(API_KEY_STORAGE_KEY) : null;
  return {
    apiBaseUrl: normalizeApiBaseUrl(storedBaseUrl || DEFAULT_API_BASE_URL),
    apiKey: storedApiKey || '',
  };
}

export function saveStoredApiConfig(config: Partial<StoredApiConfig>) {
  if (config.apiBaseUrl !== undefined) {
    writeStorage(API_URL_STORAGE_KEY, normalizeApiBaseUrl(config.apiBaseUrl));
  }
  if (config.apiKey !== undefined) {
    if (!localApiKeyStorageEnabled()) {
      removeStorage(API_KEY_STORAGE_KEY);
      return;
    }
    const trimmedKey = config.apiKey.trim();
    if (trimmedKey) {
      writeStorage(API_KEY_STORAGE_KEY, trimmedKey);
    } else {
      removeStorage(API_KEY_STORAGE_KEY);
    }
  }
}

export function clearStoredApiConfig() {
  removeStorage(API_KEY_STORAGE_KEY);
  removeStorage(API_URL_STORAGE_KEY);
}

function getApiBaseUrl(): string {
  if (typeof window !== 'undefined') {
    const stored = readStorage(API_URL_STORAGE_KEY);
    return normalizeApiBaseUrl(stored || DEFAULT_API_BASE_URL);
  }
  return normalizeApiBaseUrl(SERVER_API_BASE_URL);
}

function getApiKey(): string | undefined {
  if (typeof window !== 'undefined') {
    if (!localApiKeyStorageEnabled()) {
      return undefined;
    }
    return readStorage(API_KEY_STORAGE_KEY) || undefined;
  }
  return process.env.MLRUNX_API_KEY;
}

function readCookie(name: string): string | undefined {
  if (typeof document === 'undefined') return undefined;
  const match = document.cookie
    .split(';')
    .map((part) => part.trim())
    .find((part) => part.startsWith(`${name}=`));
  if (!match) return undefined;
  return decodeURIComponent(match.slice(name.length + 1));
}

function isMutatingMethod(method: string | undefined): boolean {
  if (!method) return false;
  const normalized = method.toUpperCase();
  return normalized === 'POST' || normalized === 'PUT' || normalized === 'PATCH' || normalized === 'DELETE';
}

interface FetchApiOptions {
  skipApiKey?: boolean;
  preferUiSession?: boolean;
  forceApiKey?: boolean;
}

async function fetchApi<T>(
  endpoint: string,
  options: RequestInit = {},
  fetchOptions: FetchApiOptions = {}
): Promise<T> {
  const url = `${getApiBaseUrl()}${endpoint}`;
  const headers: HeadersInit = {
    'Content-Type': 'application/json',
    ...(options.headers || {}),
  };

  const apiKey = getApiKey();
  const csrfToken = readCookie(UI_CSRF_COOKIE_NAME);
  const hasSessionCookie = Boolean(csrfToken);
  const useSessionAuth = hasSessionCookie && !fetchOptions.forceApiKey;

  if (!fetchOptions.skipApiKey && !useSessionAuth && apiKey) {
    (headers as Record<string, string>)['X-API-Key'] = apiKey;
  }

  if (isMutatingMethod(options.method) && useSessionAuth && csrfToken) {
    (headers as Record<string, string>)['X-CSRF-Token'] = csrfToken;
  }

  const includeCredentials = fetchOptions.skipApiKey || !apiKey || useSessionAuth;

  const response = await fetch(url, {
    ...options,
    headers,
    credentials: includeCredentials ? 'include' : 'same-origin',
  });

  if (!response.ok) {
    const text = await response.text();
    throw new ApiError(response.status, text || response.statusText);
  }

  return response.json();
}

export const api = {
  async loginUiSession(jwt: string): Promise<UiAuthLoginResult> {
    return fetchApi<UiAuthLoginResult>(
      '/api/v1/ui-auth/login',
      {
        method: 'POST',
        body: JSON.stringify({ jwt }),
      },
      { skipApiKey: true }
    );
  },

  async loginUiSessionWithBearer(jwt: string): Promise<UiAuthLoginResult> {
    return fetchApi<UiAuthLoginResult>(
      '/api/v1/ui-auth/login',
      {
        method: 'POST',
        headers: {
          Authorization: `Bearer ${jwt}`,
        },
        body: '{}',
      },
      { skipApiKey: true }
    );
  },

  async getUiSession(): Promise<UiAuthSessionResult> {
    return fetchApi<UiAuthSessionResult>('/api/v1/ui-auth/session', {}, { skipApiKey: true });
  },

  async logoutUiSession(): Promise<{ status: string }> {
    return fetchApi<{ status: string }>(
      '/api/v1/ui-auth/logout',
      { method: 'POST' },
      { skipApiKey: true }
    );
  },

  async createApiKey(request: CreateApiKeyRequest): Promise<CreateApiKeyResponse> {
    return fetchApi<CreateApiKeyResponse>(
      '/api/v1/keys',
      {
        method: 'POST',
        body: JSON.stringify(request),
      },
      { skipApiKey: true }
    );
  },

  async listApiKeys(): Promise<ListApiKeysResponse> {
    return fetchApi<ListApiKeysResponse>('/api/v1/keys', {}, { skipApiKey: true });
  },

  async revokeApiKey(keyId: string): Promise<{ status: string; revoked: string }> {
    return fetchApi<{ status: string; revoked: string }>(
      `/api/v1/keys/${encodeURIComponent(keyId)}`,
      { method: 'DELETE' },
      { skipApiKey: true }
    );
  },

  async listProjects(): Promise<ListProjectsResponse> {
    return fetchApi<ListProjectsResponse>('/api/v1/projects', {}, { skipApiKey: true });
  },

  async createProject(request: {
    name: string;
    description?: string;
  }): Promise<ProjectInfo> {
    return fetchApi<ProjectInfo>(
      '/api/v1/projects',
      {
        method: 'POST',
        body: JSON.stringify(request),
      },
      { skipApiKey: true }
    );
  },

  async deleteProject(projectId: string): Promise<{ status: string }> {
    return fetchApi<{ status: string }>(
      `/api/v1/projects/${encodeURIComponent(projectId)}`,
      { method: 'DELETE' },
      { skipApiKey: true }
    );
  },

  async listMetricAliases(projectId: string): Promise<ListMetricAliasesResponse> {
    return fetchApi<ListMetricAliasesResponse>(
      `/api/v1/projects/${encodeURIComponent(projectId)}/metric-aliases`,
      {},
      { skipApiKey: true }
    );
  },

  async upsertMetricAlias(
    projectId: string,
    request: {
      raw_name?: string;
      metric_name?: string;
      display_name: string;
      unit?: string;
      description?: string;
      is_active?: boolean;
    }
  ): Promise<MetricAlias> {
    return fetchApi<MetricAlias>(
      `/api/v1/projects/${encodeURIComponent(projectId)}/metric-aliases`,
      {
        method: 'POST',
        body: JSON.stringify(request),
      },
      { skipApiKey: true }
    );
  },

  async deleteMetricAlias(projectId: string, metricName: string): Promise<{ status: string }> {
    return fetchApi<{ status: string }>(
      `/api/v1/projects/${encodeURIComponent(projectId)}/metric-aliases/${encodeURIComponent(metricName)}`,
      { method: 'DELETE' },
      { skipApiKey: true }
    );
  },

  async listAdminUsers(): Promise<AdminListUsersResponse> {
    return fetchApi<AdminListUsersResponse>('/api/v1/admin/users', {}, { preferUiSession: true });
  },

  async listAdminUserMemberships(
    userId: string,
    params: { includeRevoked?: boolean } = {}
  ): Promise<AdminListUserMembershipsResponse> {
    const searchParams = new URLSearchParams();
    if (params.includeRevoked) searchParams.set('include_revoked', 'true');
    const query = searchParams.toString();
    return fetchApi<AdminListUserMembershipsResponse>(
      `/api/v1/admin/users/${encodeURIComponent(userId)}/memberships${query ? `?${query}` : ''}`,
      {},
      { preferUiSession: true }
    );
  },

  async disableAdminUser(userId: string): Promise<AdminUser> {
    return fetchApi<AdminUser>(
      `/api/v1/admin/users/${encodeURIComponent(userId)}/disable`,
      {
        method: 'POST',
      },
      { preferUiSession: true }
    );
  },

  async enableAdminUser(userId: string): Promise<AdminUser> {
    return fetchApi<AdminUser>(
      `/api/v1/admin/users/${encodeURIComponent(userId)}/enable`,
      {
        method: 'POST',
      },
      { preferUiSession: true }
    );
  },

  async listAdminSessions(
    params: { userId?: string; includeRevoked?: boolean } = {}
  ): Promise<AdminListSessionsResponse> {
    const searchParams = new URLSearchParams();
    if (params.userId) searchParams.set('user_id', params.userId);
    if (params.includeRevoked) searchParams.set('include_revoked', 'true');
    const query = searchParams.toString();
    return fetchApi<AdminListSessionsResponse>(
      `/api/v1/admin/sessions${query ? `?${query}` : ''}`,
      {},
      { preferUiSession: true }
    );
  },

  async revokeAdminSession(sessionId: string): Promise<{ status: string }> {
    return fetchApi<{ status: string }>(
      `/api/v1/admin/sessions/${encodeURIComponent(sessionId)}/revoke`,
      { method: 'POST' },
      { preferUiSession: true }
    );
  },

  async listAdminAuditEvents(
    params: {
      projectId?: string;
      userId?: string;
      keyId?: string;
      action?: string;
      outcome?: string;
      limit?: number;
    } = {}
  ): Promise<AdminListAuditEventsResponse> {
    const searchParams = new URLSearchParams();
    if (params.projectId) searchParams.set('project_id', params.projectId);
    if (params.userId) searchParams.set('user_id', params.userId);
    if (params.keyId) searchParams.set('key_id', params.keyId);
    if (params.action) searchParams.set('action', params.action);
    if (params.outcome) searchParams.set('outcome', params.outcome);
    if (params.limit) searchParams.set('limit', String(params.limit));
    const query = searchParams.toString();
    return fetchApi<AdminListAuditEventsResponse>(
      `/api/v1/admin/audit-events${query ? `?${query}` : ''}`,
      {},
      { preferUiSession: true }
    );
  },

  /**
   * List runs with optional filtering and pagination.
   */
  async listRuns(params: ListRunsParams = {}): Promise<ListRunsResponse> {
    const searchParams = new URLSearchParams();
    if (params.project) searchParams.set('project', params.project);
    if (params.status) searchParams.set('status', params.status);
    if (params.query) searchParams.set('q', params.query);
    if (params.tags?.length) searchParams.set('tags', params.tags.join(','));
    if (params.filter) searchParams.set('filter', params.filter);
    if (params.sortBy) searchParams.set('sort_by', params.sortBy);
    if (params.sortOrder) searchParams.set('sort_order', params.sortOrder);
    if (params.limit) searchParams.set('limit', params.limit.toString());
    if (params.offset) searchParams.set('offset', params.offset.toString());

    const query = searchParams.toString();
    return fetchApi<ListRunsResponse>(`/api/v1/runs${query ? `?${query}` : ''}`);
  },

  /**
   * Get run details by ID.
   */
  async getRun(runId: string): Promise<RunDetail> {
    return fetchApi<RunDetail>(`/api/v1/runs/${runId}`);
  },

  /**
   * Get metrics for a run.
   */
  async getMetrics(
    runId: string,
    params: { names?: string[]; maxPoints?: number } = {}
  ): Promise<MetricsResponse> {
    const searchParams = new URLSearchParams();
    if (params.names?.length) searchParams.set('names', params.names.join(','));
    if (params.maxPoints) searchParams.set('max_points', params.maxPoints.toString());

    const query = searchParams.toString();
    return fetchApi<MetricsResponse>(
      `/api/v1/runs/${runId}/metrics${query ? `?${query}` : ''}`
    );
  },

  /**
   * Get structured run events for timeline/log display.
   */
  async getRunEvents(
    runId: string,
    params: { afterId?: number; limit?: number } = {}
  ): Promise<RunEventsResponse> {
    const searchParams = new URLSearchParams();
    if (params.afterId !== undefined) searchParams.set('after_id', String(params.afterId));
    if (params.limit !== undefined) searchParams.set('limit', String(params.limit));
    const query = searchParams.toString();
    return fetchApi<RunEventsResponse>(
      `/api/v1/runs/${runId}/events${query ? `?${query}` : ''}`
    );
  },

  /**
   * Delete a run and all its associated data.
   */
  async deleteRun(runId: string): Promise<{ status: string }> {
    return fetchApi(`/api/v1/runs/${runId}`, {
      method: 'DELETE',
    });
  },

  /**
   * Compare multiple runs.
   */
  async compareRuns(
    runIds: string[],
    metricNames: string[] = [],
    maxPoints = 1000,
    paging: { limit?: number; offset?: number } = {}
  ): Promise<{
    runs: Array<{
      run_id: string;
      run_name: string | null;
      status: string;
      series: MetricSeries[];
      metric_aliases?: Record<string, string>;
    }>;
    common_metrics: string[];
    alignment: string;
    total?: number;
    limit?: number;
    offset?: number;
  }> {
    const payload = {
      run_ids: runIds,
      metric_names: metricNames,
      max_points: maxPoints,
      limit: paging.limit,
      offset: paging.offset,
    };

    try {
      // Prefer UI-session auth to avoid stale local API keys overriding browser sessions.
      return await fetchApi(
        '/api/v1/runs/compare',
        {
          method: 'POST',
          body: JSON.stringify(payload),
        },
        { skipApiKey: true }
      );
    } catch (error) {
      // Fallback for API-key-only deployments.
      if (error instanceof ApiError && (error.status === 401 || error.status === 403)) {
        return fetchApi(
          '/api/v1/runs/compare',
          {
            method: 'POST',
            body: JSON.stringify(payload),
          },
          { forceApiKey: true }
        );
      }
      throw error;
    }
  },
};
