/**
 * MLRunX API Client
 *
 * Provides type-safe API calls to the MLRunX backend.
 */

const DEFAULT_API_BASE_URL = process.env.NEXT_PUBLIC_API_URL || 'http://localhost:3001';
const SERVER_API_BASE_URL = process.env.MLRUNX_API_URL || DEFAULT_API_BASE_URL;

export const API_KEY_STORAGE_KEY = 'mlrunx_api_key';
export const API_URL_STORAGE_KEY = 'mlrunx_api_url';
export const SESSION_JWT_STORAGE_KEY = 'mlrunx_session_jwt';
export const DEFAULT_API_URL = DEFAULT_API_BASE_URL;

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
}

export interface RunDetail extends Run {
  metrics_summary: Array<{
    name: string;
    last_value: number;
    last_step: number;
  }>;
}

class ApiError extends Error {
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
  sessionJwt: string;
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
  const storedApiKey = readStorage(API_KEY_STORAGE_KEY);
  const storedSessionJwt = readStorage(SESSION_JWT_STORAGE_KEY);
  return {
    apiBaseUrl: normalizeApiBaseUrl(storedBaseUrl || DEFAULT_API_BASE_URL),
    apiKey: storedApiKey || '',
    sessionJwt: storedSessionJwt || '',
  };
}

export function saveStoredApiConfig(config: Partial<StoredApiConfig>) {
  if (config.apiBaseUrl !== undefined) {
    writeStorage(API_URL_STORAGE_KEY, normalizeApiBaseUrl(config.apiBaseUrl));
  }
  if (config.apiKey !== undefined) {
    const trimmedKey = config.apiKey.trim();
    if (trimmedKey) {
      writeStorage(API_KEY_STORAGE_KEY, trimmedKey);
    } else {
      removeStorage(API_KEY_STORAGE_KEY);
    }
  }
  if (config.sessionJwt !== undefined) {
    const trimmedToken = config.sessionJwt.trim();
    if (trimmedToken) {
      writeStorage(SESSION_JWT_STORAGE_KEY, trimmedToken);
    } else {
      removeStorage(SESSION_JWT_STORAGE_KEY);
    }
  }
}

export function clearStoredApiConfig() {
  removeStorage(API_KEY_STORAGE_KEY);
  removeStorage(API_URL_STORAGE_KEY);
  removeStorage(SESSION_JWT_STORAGE_KEY);
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
    return readStorage(API_KEY_STORAGE_KEY) || undefined;
  }
  return process.env.MLRUNX_API_KEY;
}

function getSessionJwt(): string | undefined {
  if (typeof window === 'undefined') return undefined;
  return readStorage(SESSION_JWT_STORAGE_KEY) || undefined;
}

async function fetchApi<T>(
  endpoint: string,
  options: RequestInit = {}
): Promise<T> {
  const url = `${getApiBaseUrl()}${endpoint}`;
  const headers: HeadersInit = {
    'Content-Type': 'application/json',
    ...(options.headers || {}),
  };

  // Prefer JWT/session token when configured; fallback to API key.
  const sessionJwt = getSessionJwt();
  const apiKey = getApiKey();

  if (sessionJwt) {
    (headers as Record<string, string>)['Authorization'] = `Bearer ${sessionJwt}`;
  } else if (apiKey) {
    (headers as Record<string, string>)['X-API-Key'] = apiKey;
  }

  const response = await fetch(url, {
    ...options,
    headers,
  });

  if (!response.ok) {
    const text = await response.text();
    throw new ApiError(response.status, text || response.statusText);
  }

  return response.json();
}

export const api = {
  /**
   * List runs with optional filtering and pagination.
   */
  async listRuns(params: ListRunsParams = {}): Promise<ListRunsResponse> {
    const searchParams = new URLSearchParams();
    if (params.project) searchParams.set('project', params.project);
    if (params.status) searchParams.set('status', params.status);
    if (params.query) searchParams.set('q', params.query);
    if (params.tags?.length) searchParams.set('tags', params.tags.join(','));
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
    maxPoints = 1000
  ): Promise<{
    runs: Array<{
      run_id: string;
      run_name: string | null;
      status: string;
      series: MetricSeries[];
    }>;
    common_metrics: string[];
    alignment: string;
  }> {
    return fetchApi('/api/v1/runs/compare', {
      method: 'POST',
      body: JSON.stringify({
        run_ids: runIds,
        metric_names: metricNames,
        max_points: maxPoints,
      }),
    });
  },
};
