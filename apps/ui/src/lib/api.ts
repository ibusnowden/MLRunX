/**
 * MLRunX API Client
 *
 * Provides type-safe API calls to the MLRunX backend.
 */

const API_BASE = process.env.NEXT_PUBLIC_API_URL || 'http://localhost:3001';

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

async function fetchApi<T>(
  endpoint: string,
  options: RequestInit = {}
): Promise<T> {
  const url = `${API_BASE}${endpoint}`;
  const headers: HeadersInit = {
    'Content-Type': 'application/json',
    ...(options.headers || {}),
  };

  // Add API key if available
  const apiKey = typeof window !== 'undefined'
    ? localStorage.getItem('mlrunx_api_key')
    : process.env.MLRUNX_API_KEY;

  if (apiKey) {
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
