'use client';

import Link from 'next/link';
import { use, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useRouter } from 'next/navigation';
import { api, MetricSeries, RunDetail, RunEvent } from '@/lib/api';
import { formatDuration, formatFixed } from '@/lib/format';
import { UPlotChart, type ChartSeries } from '@/components/charts/UPlotChart';
import { useTheme } from '@/components/ThemeProvider';
import { useAutoRefresh } from '@/lib/useAutoRefresh';

type ChartData = {
  xData: number[];
  series: ChartSeries[];
};

type ProgressState = 'done' | 'active' | 'pending';

type ProgressStep = {
  label: string;
  detail: string;
  state: ProgressState;
};

type MetricStats = {
  lastValue?: number;
  lastStep?: number;
  bestValue?: number;
  bestStep?: number;
  maxStep?: number;
};

const METRIC_FETCH_MAX_POINTS = 1200;
const RUN_EVENTS_FETCH_LIMIT = 200;

const TRAIN_LOSS_CANDIDATES = ['train/loss', 'loss', 'train.loss', 'training/loss'];
const VAL_LOSS_CANDIDATES = ['val/loss', 'val_loss', 'validation/loss', 'eval/loss'];
const THROUGHPUT_CANDIDATES = [
  'tokens/sec',
  'tokens_per_sec',
  'throughput/tokens_per_sec',
  'train/tokens_per_sec',
  'tps',
];

const STATUS_BADGE_STYLES: Record<string, string> = {
  running: 'border-[color:rgba(96,165,250,0.30)] bg-[rgba(96,165,250,0.12)] text-[var(--badge-running-text)]',
  finished: 'border-[color:rgba(52,211,153,0.30)] bg-[rgba(52,211,153,0.12)] text-[var(--badge-finished-text)]',
  failed: 'border-[color:rgba(248,113,113,0.30)] bg-[rgba(248,113,113,0.12)] text-[var(--badge-failed-text)]',
  killed: 'border-[color:rgba(251,191,36,0.30)] bg-[rgba(251,191,36,0.12)] text-[var(--badge-killed-text)]',
  pending: 'border-border bg-surface-secondary text-text-secondary',
};

const STATUS_DOT_STYLES: Record<string, string> = {
  running: 'bg-[var(--badge-running-text)]',
  finished: 'bg-[var(--badge-finished-text)]',
  failed: 'bg-[var(--badge-failed-text)]',
  killed: 'bg-[var(--badge-killed-text)]',
  pending: 'bg-text-muted',
};

const BackIcon = () => (
  <svg className="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15.75 19.5L8.25 12l7.5-7.5" />
  </svg>
);

const MoonIcon = () => (
  <svg className="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M21.752 15.002A9.718 9.718 0 0118 15.75c-5.385 0-9.75-4.365-9.75-9.75 0-1.33.266-2.597.748-3.752A9.753 9.753 0 003 11.25C3 16.635 7.365 21 12.75 21a9.753 9.753 0 009.002-5.998z" />
  </svg>
);

const SunIcon = () => (
  <svg className="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M12 3v2.25m6.364.386l-1.591 1.591M21 12h-2.25m-.386 6.364l-1.591-1.591M12 18.75V21m-4.773-4.227l-1.591 1.591M5.25 12H3m4.227-4.773L5.636 5.636M15.75 12a3.75 3.75 0 11-7.5 0 3.75 3.75 0 017.5 0z" />
  </svg>
);

const CompareIcon = () => (
  <svg className="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M7.5 21L3 16.5m0 0L7.5 12M3 16.5h13.5m0-13.5L21 7.5m0 0L16.5 12M21 7.5H7.5" />
  </svg>
);

const TrashIcon = () => (
  <svg className="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M14.74 9l-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 01-2.244 2.077H8.084a2.25 2.25 0 01-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 00-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 013.478-.397m7.5 0v-.916c0-1.18-.91-2.164-2.09-2.201a51.964 51.964 0 00-3.32 0c-1.18.037-2.09 1.022-2.09 2.201v.916m7.5 0a48.667 48.667 0 00-7.5 0" />
  </svg>
);

const ClockIcon = () => (
  <svg className="h-3.5 w-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M12 6v6h4.5m4.5 0a9 9 0 11-18 0 9 9 0 0118 0z" />
  </svg>
);

const CalendarIcon = () => (
  <svg className="h-3.5 w-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M6.75 3v2.25M17.25 3v2.25M3 18.75V7.5a2.25 2.25 0 012.25-2.25h13.5A2.25 2.25 0 0121 7.5v11.25m-18 0A2.25 2.25 0 005.25 21h13.5A2.25 2.25 0 0021 18.75m-18 0v-7.5A2.25 2.25 0 015.25 9h13.5A2.25 2.25 0 0121 11.25v7.5" />
  </svg>
);

const IdIcon = () => (
  <svg className="h-3.5 w-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M7.864 4.243A17.237 17.237 0 0111.76 3.07M12 3.07c1.344.07 2.654.26 3.896.585m0 0L19.5 7.125M4.5 7.125l3.604-3.47m0 0a48.108 48.108 0 017.792 0" />
  </svg>
);

const LoadingSpinner = () => (
  <div className="flex items-center justify-center gap-2 text-sm text-text-muted">
    <svg className="h-5 w-5 animate-spin" viewBox="0 0 24 24">
      <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" fill="none" />
      <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
    </svg>
    Loading run data...
  </div>
);

function normalizeKey(value: string): string {
  return value.toLowerCase().replace(/[^a-z0-9]/g, '_');
}

function parseApiDate(value: string): Date | null {
  const trimmed = value.trim();
  if (!trimmed) return null;
  const normalized = /^\d{4}-\d{2}-\d{2} /.test(trimmed)
    ? `${trimmed.replace(' ', 'T')}Z`
    : trimmed;
  const parsed = new Date(normalized);
  if (Number.isNaN(parsed.getTime())) return null;
  return parsed;
}

function formatDateTime(value: string): string {
  const parsed = parseApiDate(value);
  if (!parsed) return value;
  return parsed.toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    year: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  });
}

function formatTimeOnly(value: string): string {
  const parsed = parseApiDate(value);
  if (!parsed) return '--:--:--';
  return parsed.toLocaleTimeString(undefined, {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    hour12: false,
  });
}

function formatRelativeTime(value: string): string {
  const parsed = parseApiDate(value);
  if (!parsed) return '';
  const deltaSeconds = Math.round((Date.now() - parsed.getTime()) / 1000);
  if (deltaSeconds < 20) return 'just now';
  if (deltaSeconds < 60) return `${deltaSeconds}s ago`;
  if (deltaSeconds < 3600) return `${Math.floor(deltaSeconds / 60)} min ago`;
  if (deltaSeconds < 86400) return `${Math.floor(deltaSeconds / 3600)} hr ago`;
  return `${Math.floor(deltaSeconds / 86400)} day ago`;
}

function formatStatusLabel(status: string): string {
  if (!status) return 'Pending';
  return status.charAt(0).toUpperCase() + status.slice(1);
}

function shortenRunId(runId: string): string {
  return runId.length <= 12 ? runId : runId.slice(0, 12);
}

function humanizeKey(raw: string): string {
  return raw
    .replace(/[_/.-]+/g, ' ')
    .replace(/\b\w/g, (char) => char.toUpperCase());
}

function buildTagIndex(tags: Record<string, string>): Map<string, string> {
  const index = new Map<string, string>();
  Object.entries(tags).forEach(([key, value]) => {
    index.set(normalizeKey(key), value);
  });
  return index;
}

function pickTagValue(tags: Record<string, string>, candidates: string[]): string | undefined {
  const index = buildTagIndex(tags);
  for (const candidate of candidates) {
    const value = index.get(normalizeKey(candidate));
    if (value) {
      return value;
    }
  }
  return undefined;
}

function parseNumeric(value: string | undefined): number | undefined {
  if (!value) return undefined;
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) return undefined;
  return parsed;
}

function pickSeriesByCandidates(
  allMetrics: MetricSeries[],
  exactCandidates: string[],
  containsCandidates: string[] = []
): MetricSeries | undefined {
  const exactLower = exactCandidates.map((value) => value.toLowerCase());
  const containsLower = containsCandidates.map((value) => value.toLowerCase());

  for (const metric of allMetrics) {
    if (exactLower.includes(metric.name.toLowerCase())) {
      return metric;
    }
  }
  for (const metric of allMetrics) {
    const lowered = metric.name.toLowerCase();
    if (containsLower.some((needle) => lowered.includes(needle))) {
      return metric;
    }
  }
  return undefined;
}

function getMetricStats(series: MetricSeries | undefined): MetricStats {
  if (!series || series.points.length === 0) return {};

  let bestValue = Number.POSITIVE_INFINITY;
  let bestStep = 0;
  let lastValue = 0;
  let lastStep = 0;

  series.points.forEach((point) => {
    if (point.mean < bestValue) {
      bestValue = point.mean;
      bestStep = point.step;
    }
    if (point.step >= lastStep) {
      lastStep = point.step;
      lastValue = point.mean;
    }
  });

  return {
    lastValue,
    lastStep,
    bestValue: Number.isFinite(bestValue) ? bestValue : undefined,
    bestStep,
    maxStep: lastStep,
  };
}

function selectFeaturedMetrics(names: string[]): string[] {
  if (names.length === 0) return [];

  const preferred: string[] = [];
  const addIfFound = (predicate: (name: string) => boolean) => {
    const found = names.find(predicate);
    if (found && !preferred.includes(found)) {
      preferred.push(found);
    }
  };

  addIfFound((name) => TRAIN_LOSS_CANDIDATES.includes(name.toLowerCase()));
  addIfFound((name) => VAL_LOSS_CANDIDATES.includes(name.toLowerCase()));
  addIfFound((name) => name.toLowerCase().includes('loss'));
  addIfFound((name) => name.toLowerCase().includes('lr') || name.toLowerCase().includes('learning_rate'));
  addIfFound((name) => name.toLowerCase().includes('seq') || name.toLowerCase().includes('length'));

  for (const name of names) {
    if (preferred.length >= 3) break;
    if (!preferred.includes(name)) {
      preferred.push(name);
    }
  }

  return preferred.slice(0, 3);
}

function alignSeriesForChart(seriesList: MetricSeries[]): ChartData {
  if (seriesList.length === 0) {
    return { xData: [], series: [] };
  }

  const stepSet = new Set<number>();
  seriesList.forEach((series) => {
    series.points.forEach((point) => {
      stepSet.add(point.step);
    });
  });

  const xData = Array.from(stepSet).sort((a, b) => a - b);
  const stepIndex = new Map<number, number>(xData.map((step, index) => [step, index]));

  const alignedSeries: ChartSeries[] = seriesList.map((series) => {
    const data = new Array<number>(xData.length).fill(Number.NaN);
    series.points.forEach((point) => {
      const index = stepIndex.get(point.step);
      if (index !== undefined) {
        data[index] = point.mean;
      }
    });
    return {
      label: series.name,
      data,
    };
  });

  return {
    xData,
    series: alignedSeries,
  };
}

function formatLargeNumber(value: number): string {
  if (Math.abs(value) >= 1000) {
    return `${(value / 1000).toFixed(1)}k`;
  }
  return value.toFixed(1);
}

function getTotalSteps(run: RunDetail, allMetrics: MetricSeries[]): number {
  let maxStep = 0;

  run.metrics_summary.forEach((metric) => {
    maxStep = Math.max(maxStep, metric.last_step);
  });

  allMetrics.forEach((series) => {
    if (series.points.length > 0) {
      maxStep = Math.max(maxStep, series.points[series.points.length - 1].step);
    }
  });

  if (maxStep > 0) return maxStep;
  return run.metrics_count;
}

function buildProgressSteps(
  run: RunDetail,
  tags: Record<string, string>,
  totalSteps: number
): ProgressStep[] {
  const modelName = pickTagValue(tags, ['model', 'model_name']) || 'model metadata';
  const datasetName = pickTagValue(tags, ['dataset', 'data', 'dataset_name']) || 'dataset metadata';
  const durationLabel = formatDuration(run.duration_seconds);
  const trainingDetail = totalSteps > 0
    ? `${totalSteps.toLocaleString()} logged steps`
    : `${run.metrics_count.toLocaleString()} metric points`;

  const steps: Array<{ label: string; detail: string }> = [
    { label: 'Initializing', detail: 'Run context prepared' },
    { label: 'Loading Model', detail: modelName },
    { label: 'Loading Dataset', detail: datasetName },
    { label: 'Training', detail: trainingDetail },
    { label: 'Saving Model', detail: run.status === 'finished' ? 'Artifacts finalized' : 'Waiting for terminal state' },
    {
      label: 'Completed',
      detail:
        run.status === 'finished'
          ? `Finished in ${durationLabel}`
          : run.status === 'running'
            ? 'In progress'
            : run.status === 'failed'
              ? 'Exited with failure'
              : run.status === 'killed'
                ? 'Stopped by user/system'
                : 'Queued',
    },
  ];

  const stateByStatus: Record<string, ProgressState[]> = {
    pending: ['active', 'pending', 'pending', 'pending', 'pending', 'pending'],
    running: ['done', 'done', 'done', 'active', 'pending', 'pending'],
    finished: ['done', 'done', 'done', 'done', 'done', 'done'],
    failed: ['done', 'done', 'done', 'done', 'pending', 'active'],
    killed: ['done', 'done', 'done', 'done', 'pending', 'active'],
  };

  const states = stateByStatus[run.status] || stateByStatus.pending;
  return steps.map((step, index) => ({
    ...step,
    state: states[index] || 'pending',
  }));
}

function normalizeEventLevel(level: string): 'debug' | 'info' | 'warn' | 'error' {
  const normalized = level.trim().toLowerCase();
  if (normalized === 'error') return 'error';
  if (normalized === 'warn' || normalized === 'warning') return 'warn';
  if (normalized === 'debug') return 'debug';
  return 'info';
}

function formatEventTime(event: RunEvent): string {
  if (event.timestamp && Number.isFinite(event.timestamp)) {
    return new Date(event.timestamp * 1000).toLocaleTimeString(undefined, {
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
      hour12: false,
    });
  }
  return formatTimeOnly(event.created_at);
}

function mergeRunEvents(existing: RunEvent[], incoming: RunEvent[]): RunEvent[] {
  if (incoming.length === 0) return existing;
  const merged = new Map<number, RunEvent>();
  existing.forEach((event) => merged.set(event.id, event));
  incoming.forEach((event) => merged.set(event.id, event));
  return Array.from(merged.values()).sort((a, b) => a.id - b.id);
}

function getThroughputValue(run: RunDetail, allMetrics: MetricSeries[]): number | undefined {
  const throughputSeries = pickSeriesByCandidates(
    allMetrics,
    THROUGHPUT_CANDIDATES,
    ['token', 'throughput', 'tps']
  );

  if (throughputSeries && throughputSeries.points.length > 0) {
    return throughputSeries.points[throughputSeries.points.length - 1].mean;
  }

  const fromTags = parseNumeric(
    pickTagValue(run.tags, [
      'tokens/sec',
      'tokens_per_sec',
      'throughput',
      'throughput_tokens_per_sec',
      'tps',
    ])
  );
  return fromTags;
}

function extractHyperparameterTags(
  tags: Record<string, string>,
  exclude: string[]
): Array<{ key: string; value: string }> {
  const excluded = new Set(exclude.map((key) => normalizeKey(key)));
  const preferredPattern = /(lr|learning|batch|epoch|rank|warmup|weight|token|step|seq|accum|dropout|layer|head|dim)/i;

  const primary = Object.entries(tags)
    .filter(([key]) => !excluded.has(normalizeKey(key)))
    .filter(([key]) => preferredPattern.test(key))
    .slice(0, 8)
    .map(([key, value]) => ({ key: humanizeKey(key), value }));

  if (primary.length > 0) return primary;

  return Object.entries(tags)
    .filter(([key]) => !excluded.has(normalizeKey(key)))
    .slice(0, 8)
    .map(([key, value]) => ({ key: humanizeKey(key), value }));
}

function SummaryCard({
  label,
  value,
  subtext,
  emphasize = false,
}: {
  label: string;
  value: string;
  subtext?: string;
  emphasize?: boolean;
}) {
  return (
    <div className="border-r border-border px-5 py-4 last:border-r-0">
      <div className="text-[11px] uppercase tracking-[0.08em] text-text-muted">{label}</div>
      <div className={`mt-2 font-mono text-3xl font-semibold tracking-[-0.03em] ${emphasize ? 'text-[var(--badge-finished-text)]' : 'text-text-primary'}`}>
        {value}
      </div>
      {subtext && <div className="mt-1 text-xs text-text-muted">{subtext}</div>}
    </div>
  );
}

function MetaChip({
  icon,
  children,
}: {
  icon: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <span className="inline-flex items-center gap-1.5 text-sm text-text-secondary">
      <span className="text-text-muted">{icon}</span>
      {children}
    </span>
  );
}

function KeyValueGrid({
  items,
}: {
  items: Array<{ key: string; value: string }>;
}) {
  if (items.length === 0) {
    return (
      <div className="px-5 py-6 text-sm text-text-muted">
        No hyperparameters were logged for this run.
      </div>
    );
  }

  return (
    <div className="grid grid-cols-1 divide-y divide-border sm:grid-cols-2 sm:divide-x sm:divide-y-0">
      {items.map((item) => (
        <div key={item.key} className="flex items-center justify-between px-5 py-3 text-sm">
          <span className="text-text-muted">{item.key}</span>
          <span className="font-mono font-medium text-text-primary">{item.value}</span>
        </div>
      ))}
    </div>
  );
}

export default function RunDetailPage({ params }: { params: Promise<{ run_id: string }> }) {
  const { run_id: runId } = use(params);
  const router = useRouter();
  const { isDark, toggleTheme } = useTheme();

  const [run, setRun] = useState<RunDetail | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [metrics, setMetrics] = useState<MetricSeries[]>([]);
  const [availableMetrics, setAvailableMetrics] = useState<string[]>([]);
  const [metricsLoading, setMetricsLoading] = useState(true);
  const [metricsError, setMetricsError] = useState<string | null>(null);
  const [runEvents, setRunEvents] = useState<RunEvent[]>([]);
  const [eventsLoading, setEventsLoading] = useState(true);
  const [eventsError, setEventsError] = useState<string | null>(null);
  const [showDeleteConfirm, setShowDeleteConfirm] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const eventsCursorRef = useRef<number | null>(null);

  const setTheme = (target: 'dark' | 'light') => {
    if ((target === 'dark') !== isDark) {
      toggleTheme();
    }
  };

  const fetchRun = useCallback(async ({ silent = false }: { silent?: boolean } = {}) => {
    if (!silent) {
      setLoading(true);
    }
    setError(null);
    try {
      const data = await api.getRun(runId);
      setRun(data);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load run');
    } finally {
      if (!silent) {
        setLoading(false);
      }
    }
  }, [runId]);

  const fetchMetrics = useCallback(async ({ silent = false }: { silent?: boolean } = {}) => {
    if (!silent) {
      setMetricsLoading(true);
    }
    setMetricsError(null);
    try {
      const response = await api.getMetrics(runId, { maxPoints: METRIC_FETCH_MAX_POINTS });
      setAvailableMetrics(response.available_metrics);
      setMetrics(response.series);
    } catch (err) {
      setMetricsError(err instanceof Error ? err.message : 'Failed to load metrics');
    } finally {
      if (!silent) {
        setMetricsLoading(false);
      }
    }
  }, [runId]);

  const fetchRunEvents = useCallback(
    async ({
      silent = false,
      reset = false,
    }: { silent?: boolean; reset?: boolean } = {}) => {
      if (!silent) {
        setEventsLoading(true);
      }
      if (reset) {
        eventsCursorRef.current = null;
        setRunEvents([]);
      }
      setEventsError(null);
      try {
        const response = await api.getRunEvents(runId, {
          afterId: reset ? undefined : (eventsCursorRef.current ?? undefined),
          limit: RUN_EVENTS_FETCH_LIMIT,
        });
        setRunEvents((previous) => (reset ? response.events : mergeRunEvents(previous, response.events)));
        const nextAfterId = response.next_after_id ?? eventsCursorRef.current;
        if (nextAfterId !== null && nextAfterId !== undefined) {
          eventsCursorRef.current = nextAfterId;
        }
      } catch (err) {
        setEventsError(err instanceof Error ? err.message : 'Failed to load run events');
      } finally {
        if (!silent) {
          setEventsLoading(false);
        }
      }
    },
    [runId]
  );

  useEffect(() => {
    void fetchRun();
    void fetchMetrics();
    void fetchRunEvents({ reset: true });
  }, [fetchRun, fetchMetrics, fetchRunEvents]);

  const shouldAutoRefresh = run?.status === 'running'
    || (!!run && run.metrics_count > 0 && availableMetrics.length === 0);

  useAutoRefresh(
    async () => {
      await Promise.all([
        fetchRun({ silent: true }),
        fetchMetrics({ silent: true }),
      ]);
    },
    { intervalMs: 15000, enabled: shouldAutoRefresh, runOnMount: false }
  );

  useAutoRefresh(
    async () => {
      await fetchRunEvents({ silent: true });
    },
    { intervalMs: 4000, enabled: run?.status === 'running', runOnMount: false }
  );

  const featuredMetricNames = useMemo(
    () => selectFeaturedMetrics(availableMetrics),
    [availableMetrics]
  );

  const featuredMetricSeries = useMemo(() => {
    return featuredMetricNames
      .map((name) => metrics.find((series) => series.name === name))
      .filter((series): series is MetricSeries => Boolean(series));
  }, [featuredMetricNames, metrics]);

  const chartData = useMemo<ChartData>(() => {
    return alignSeriesForChart(featuredMetricSeries);
  }, [featuredMetricSeries]);

  const totalSteps = useMemo(() => {
    if (!run) return 0;
    return getTotalSteps(run, metrics);
  }, [run, metrics]);

  const trainLossSeries = useMemo(
    () => pickSeriesByCandidates(metrics, TRAIN_LOSS_CANDIDATES, ['loss']),
    [metrics]
  );
  const valLossSeries = useMemo(
    () => pickSeriesByCandidates(metrics, VAL_LOSS_CANDIDATES, ['val', 'eval']),
    [metrics]
  );

  const trainLossStats = useMemo(() => getMetricStats(trainLossSeries), [trainLossSeries]);
  const valLossStats = useMemo(() => getMetricStats(valLossSeries), [valLossSeries]);
  const throughputValue = useMemo(() => {
    if (!run) return undefined;
    return getThroughputValue(run, metrics);
  }, [run, metrics]);

  const runTags = useMemo(() => run?.tags ?? {}, [run]);

  const configValues = useMemo(() => ({
    gpu: pickTagValue(runTags, ['gpu', 'gpu_name', 'hardware/gpu', 'device', 'cuda_device']) || '-',
    quantization: pickTagValue(runTags, ['quantization', 'quant', 'precision', 'bits']) || '-',
    framework: pickTagValue(runTags, ['framework', 'trainer', 'library']) || '-',
  }), [runTags]);

  const hyperparams = useMemo(
    () => extractHyperparameterTags(runTags, [
      'model',
      'dataset',
      'framework',
      'gpu',
      'gpu_name',
      'hardware/gpu',
      'device',
      'cuda_device',
      'quantization',
      'quant',
      'precision',
      'bits',
    ]),
    [runTags]
  );

  const progressSteps = useMemo(() => {
    if (!run) return [];
    return buildProgressSteps(run, runTags, totalSteps);
  }, [run, runTags, totalSteps]);

  const lastEventRelative = useMemo(() => {
    if (!run) return '';
    if (runEvents.length === 0) return formatRelativeTime(run.updated_at);
    return formatRelativeTime(runEvents[runEvents.length - 1].created_at);
  }, [run, runEvents]);

  const handleDeleteRun = useCallback(async () => {
    setDeleting(true);
    try {
      await api.deleteRun(runId);
      router.push('/');
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to delete run');
      setDeleting(false);
    }
  }, [runId, router]);

  if (loading) {
    return (
      <main className="min-h-screen bg-background px-5 py-8 sm:px-8">
        <div className="mx-auto max-w-[1150px]">
          <LoadingSpinner />
        </div>
      </main>
    );
  }

  if (error || !run) {
    return (
      <main className="min-h-screen bg-background px-5 py-8 sm:px-8">
        <div className="mx-auto max-w-[1150px]">
          <div className="rounded-xl border border-[color:rgba(248,113,113,0.25)] bg-danger-subtle px-4 py-3 text-sm text-danger">
            {error || 'Run not found'}
          </div>
          <button
            type="button"
            onClick={() => router.push('/')}
            className="mt-4 inline-flex items-center gap-2 rounded-lg border border-border bg-surface px-3 py-2 text-sm text-text-secondary hover:bg-surface-hover hover:text-text-primary"
          >
            <BackIcon />
            Back to Runs
          </button>
        </div>
      </main>
    );
  }

  return (
    <main className="min-h-screen bg-[var(--page-gradient)] px-5 py-8 sm:px-8">
      <div className="mx-auto max-w-[1150px]">
        <div className="mb-6 flex flex-wrap items-center justify-between gap-3">
          <Link
            href="/"
            className="inline-flex items-center gap-1.5 text-sm font-medium text-text-muted transition-colors hover:text-accent"
          >
            <BackIcon />
            Back to Runs
          </Link>

          <div className="flex flex-wrap items-center gap-2">
            <div className="inline-flex rounded-lg border border-border bg-surface p-0.5">
              <button
                type="button"
                onClick={() => setTheme('dark')}
                className={`inline-flex h-8 w-9 items-center justify-center rounded-md transition-colors ${
                  isDark ? 'bg-accent-subtle text-accent' : 'text-text-muted hover:text-text-secondary'
                }`}
                title="Dark mode"
              >
                <MoonIcon />
              </button>
              <button
                type="button"
                onClick={() => setTheme('light')}
                className={`inline-flex h-8 w-9 items-center justify-center rounded-md transition-colors ${
                  !isDark ? 'bg-accent-subtle text-accent' : 'text-text-muted hover:text-text-secondary'
                }`}
                title="Light mode"
              >
                <SunIcon />
              </button>
            </div>

            <button
              type="button"
              onClick={() => router.push(`/compare?runs=${encodeURIComponent(run.run_id)}`)}
              className="inline-flex h-9 items-center gap-1.5 rounded-lg border border-border bg-surface px-3 text-sm font-medium text-text-secondary transition-all duration-200 hover:-translate-y-0.5 hover:bg-surface-hover hover:text-text-primary"
            >
              <CompareIcon />
              Compare
            </button>

            {showDeleteConfirm ? (
              <div className="inline-flex items-center gap-1 rounded-lg border border-[color:rgba(248,113,113,0.25)] bg-[rgba(248,113,113,0.08)] px-1.5 py-1">
                <button
                  type="button"
                  onClick={() => void handleDeleteRun()}
                  disabled={deleting}
                  className="rounded-md bg-danger px-2 py-1 text-xs font-semibold text-white disabled:opacity-60"
                >
                  {deleting ? 'Deleting...' : 'Confirm'}
                </button>
                <button
                  type="button"
                  onClick={() => setShowDeleteConfirm(false)}
                  className="rounded-md px-2 py-1 text-xs font-medium text-text-secondary hover:text-text-primary"
                >
                  Cancel
                </button>
              </div>
            ) : (
              <button
                type="button"
                onClick={() => setShowDeleteConfirm(true)}
                className="inline-flex h-9 items-center gap-1.5 rounded-lg border border-[color:rgba(248,113,113,0.25)] bg-[rgba(248,113,113,0.08)] px-3 text-sm font-medium text-danger transition-all duration-200 hover:-translate-y-0.5 hover:bg-[rgba(248,113,113,0.14)]"
              >
                <TrashIcon />
                Delete
              </button>
            )}
          </div>
        </div>

        <div className="mb-5">
          <div className="mb-2 flex flex-wrap items-center gap-3">
            <h1 className="text-4xl font-semibold tracking-[-0.03em] text-text-primary">
              {run.name || 'Unnamed Run'}
            </h1>
            <span
              className={`inline-flex items-center gap-1.5 rounded-full border px-3 py-1 text-sm font-semibold ${
                STATUS_BADGE_STYLES[run.status] || STATUS_BADGE_STYLES.pending
              }`}
            >
              <span className={`h-1.5 w-1.5 rounded-full ${STATUS_DOT_STYLES[run.status] || STATUS_DOT_STYLES.pending}`} />
              {formatStatusLabel(run.status)}
            </span>
          </div>

          <div className="flex flex-wrap items-center gap-x-4 gap-y-2 text-sm text-text-secondary">
            <MetaChip icon={<IdIcon />}>
              ID: <span className="rounded-md border border-border bg-background px-2 py-0.5 font-mono text-xs text-text-muted">{shortenRunId(run.run_id)}</span>
            </MetaChip>
            <MetaChip icon={<ClockIcon />}>{formatDuration(run.duration_seconds)}</MetaChip>
            <MetaChip icon={<CalendarIcon />}>{formatDateTime(run.created_at)}</MetaChip>
            {pickTagValue(runTags, ['model', 'model_name']) && (
              <MetaChip icon={<CompareIcon />}>
                Model: <span className="rounded-md border border-border bg-background px-2 py-0.5 font-mono text-xs text-text-muted">{pickTagValue(runTags, ['model', 'model_name'])}</span>
              </MetaChip>
            )}
            {pickTagValue(runTags, ['dataset', 'data', 'dataset_name']) && (
              <MetaChip icon={<IdIcon />}>
                Dataset: <span className="rounded-md border border-border bg-background px-2 py-0.5 font-mono text-xs text-text-muted">{pickTagValue(runTags, ['dataset', 'data', 'dataset_name'])}</span>
              </MetaChip>
            )}
          </div>
        </div>

        <div className="mb-4 overflow-hidden rounded-2xl border border-border bg-surface">
          <div className="grid grid-cols-1 divide-y divide-border sm:grid-cols-2 sm:divide-x sm:divide-y-0 lg:grid-cols-4">
            <SummaryCard
              label="Total Steps"
              value={totalSteps.toLocaleString()}
              subtext={`${run.metrics_count.toLocaleString()} metric points`}
            />
            <SummaryCard
              label="Final Loss"
              value={trainLossStats.lastValue !== undefined ? formatFixed(trainLossStats.lastValue, 3) : '-'}
              subtext={
                trainLossStats.bestValue !== undefined
                  ? `best ${formatFixed(trainLossStats.bestValue, 3)} @ step ${trainLossStats.bestStep?.toLocaleString()}`
                  : undefined
              }
              emphasize={trainLossStats.lastValue !== undefined}
            />
            <SummaryCard
              label="Val Loss"
              value={valLossStats.lastValue !== undefined ? formatFixed(valLossStats.lastValue, 3) : '-'}
              subtext={
                valLossStats.bestValue !== undefined
                  ? `best ${formatFixed(valLossStats.bestValue, 3)} @ step ${valLossStats.bestStep?.toLocaleString()}`
                  : undefined
              }
            />
            <SummaryCard
              label="Tokens / Sec"
              value={throughputValue !== undefined ? formatLargeNumber(throughputValue) : '-'}
              subtext={throughputValue !== undefined ? 'latest throughput' : `${run.metrics_count.toLocaleString()} points logged`}
            />
          </div>
        </div>

        <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
          <section className="rounded-2xl border border-border bg-surface shadow-[0_0_0_1px_rgba(255,255,255,0.01)]">
            <div className="flex items-center justify-between border-b border-border px-5 py-4">
              <div className="flex items-center gap-2.5">
                <div className="inline-flex h-8 w-8 items-center justify-center rounded-lg bg-accent-subtle text-accent">
                  <CompareIcon />
                </div>
                <div>
                  <h2 className="text-xl font-semibold text-text-primary">Training Loss</h2>
                  <p className="text-xs text-text-muted">
                    {featuredMetricSeries.length > 0 ? `${featuredMetricSeries.length} metrics` : 'No metric series'}
                  </p>
                </div>
              </div>
            </div>
            <div className="p-4">
              {metricsError && (
                <div className="mb-3 rounded-lg border border-[color:rgba(248,113,113,0.25)] bg-danger-subtle px-3 py-2 text-sm text-danger">
                  {metricsError}
                </div>
              )}
              {metricsLoading ? (
                <div className="h-[430px] rounded-xl border border-border bg-background">
                  <LoadingSpinner />
                </div>
              ) : chartData.xData.length === 0 || chartData.series.length === 0 ? (
                <div className="flex h-[430px] items-center justify-center rounded-xl border border-border bg-background text-sm text-text-muted">
                  {run.metrics_count > 0
                    ? 'Metrics are still syncing for this run.'
                    : 'No metrics recorded for this run.'}
                </div>
              ) : (
                <div className="overflow-hidden rounded-xl border border-border">
                  <UPlotChart
                    xData={chartData.xData}
                    series={chartData.series}
                    xLabel="Step"
                    yLabel="Loss"
                    height={430}
                    interactive={true}
                    darkTheme={isDark}
                    showLegend={true}
                    smoothing={0.06}
                    areaFill={true}
                  />
                </div>
              )}
            </div>
          </section>

          <div className="flex flex-col gap-4">
            <section className="rounded-2xl border border-border bg-surface shadow-[0_0_0_1px_rgba(255,255,255,0.01)]">
              <div className="border-b border-border px-5 py-4">
                <h2 className="text-xl font-semibold text-text-primary">Training Configuration</h2>
              </div>
              <div className="grid grid-cols-1 sm:grid-cols-3">
                <div className="border-b border-border px-5 py-4 sm:border-b-0 sm:border-r">
                  <div className="text-[11px] uppercase tracking-[0.08em] text-text-muted">GPU</div>
                  <div className="mt-1.5 text-xl font-semibold text-text-primary">{configValues.gpu}</div>
                </div>
                <div className="border-b border-border px-5 py-4 sm:border-b-0 sm:border-r">
                  <div className="text-[11px] uppercase tracking-[0.08em] text-text-muted">Quantization</div>
                  <div className="mt-1.5 text-xl font-semibold text-text-primary">{configValues.quantization}</div>
                </div>
                <div className="px-5 py-4">
                  <div className="text-[11px] uppercase tracking-[0.08em] text-text-muted">Framework</div>
                  <div className="mt-1.5 text-xl font-semibold text-text-primary">{configValues.framework}</div>
                </div>
              </div>
            </section>

            <section className="rounded-2xl border border-border bg-surface shadow-[0_0_0_1px_rgba(255,255,255,0.01)]">
              <div className="border-b border-border px-5 py-4">
                <h2 className="text-xl font-semibold text-text-primary">Hyperparameters</h2>
              </div>
              <KeyValueGrid items={hyperparams} />
            </section>
          </div>

          <section className="rounded-2xl border border-border bg-surface shadow-[0_0_0_1px_rgba(255,255,255,0.01)]">
            <div className="border-b border-border px-5 py-4">
              <h2 className="text-xl font-semibold text-text-primary">Training Progress</h2>
            </div>
            <div className="px-5 py-4">
              <ol className="space-y-1">
                {progressSteps.map((step, index) => (
                  <li key={step.label} className="flex gap-3 py-1">
                    <div className="flex w-5 flex-col items-center">
                      <span
                        className={`h-2.5 w-2.5 rounded-full border-2 ${
                          step.state === 'done'
                            ? 'border-[var(--badge-finished-text)] bg-[var(--badge-finished-text)]'
                            : step.state === 'active'
                              ? 'border-[var(--badge-running-text)] bg-[var(--badge-running-text)] shadow-[0_0_0_4px_rgba(96,165,250,0.15)]'
                              : 'border-border bg-surface-secondary'
                        }`}
                      />
                      {index < progressSteps.length - 1 && (
                        <span
                          className={`mt-1 h-7 w-[2px] ${
                            step.state === 'done' ? 'bg-[var(--badge-finished-text)]' : 'bg-border'
                          }`}
                        />
                      )}
                    </div>
                    <div>
                      <div
                        className={`text-lg ${
                          step.state === 'active'
                            ? 'font-semibold text-accent'
                            : step.state === 'done'
                              ? 'font-medium text-text-primary'
                              : 'font-medium text-text-muted'
                        }`}
                      >
                        {step.label}
                      </div>
                      <div className="font-mono text-xs text-text-muted">{step.detail}</div>
                    </div>
                  </li>
                ))}
              </ol>
            </div>
          </section>

          <section className="rounded-2xl border border-border bg-surface shadow-[0_0_0_1px_rgba(255,255,255,0.01)]">
            <div className="border-b border-border px-5 py-4">
              <h2 className="text-xl font-semibold text-text-primary">Logs</h2>
              <p className="mt-0.5 text-xs text-text-muted">Live run events from backend</p>
            </div>
            <div className="p-4">
              <div className="max-h-[260px] overflow-y-auto rounded-xl border border-border bg-background px-4 py-3 font-mono text-xs">
                {eventsLoading ? (
                  <div className="py-6 text-center text-text-muted">Loading run events...</div>
                ) : eventsError ? (
                  <div className="py-6 text-center text-danger">{eventsError}</div>
                ) : runEvents.length === 0 ? (
                  <div className="py-6 text-center text-text-muted">
                    No events recorded yet. Use{' '}
                    <code className="font-semibold text-text-secondary">run.log_event(...)</code>{' '}
                    to stream training logs.
                  </div>
                ) : (
                  runEvents.map((event) => {
                    const level = normalizeEventLevel(event.level);
                    const levelClass =
                      level === 'error'
                        ? 'font-semibold text-danger'
                        : level === 'warn'
                          ? 'font-semibold text-warning'
                          : level === 'debug'
                            ? 'font-semibold text-text-secondary'
                            : 'font-semibold text-accent';
                    const stepSuffix =
                      event.step !== null && event.step !== undefined
                        ? ` (step ${event.step.toLocaleString()})`
                        : '';
                    const sourcePrefix = event.source ? `[${event.source}] ` : '';

                    return (
                      <div key={event.id} className="mb-1 flex gap-3 leading-6 last:mb-0">
                        <span className="text-text-muted">{formatEventTime(event)}</span>
                        <span className={levelClass}>{level.toUpperCase()}</span>
                        <span className="text-text-secondary">{`${sourcePrefix}${event.message}${stepSuffix}`}</span>
                      </div>
                    );
                  })
                )}
              </div>
              <div className="mt-2 text-xs text-text-muted">
                Last event: {lastEventRelative || 'n/a'}
              </div>
            </div>
          </section>
        </div>
      </div>
    </main>
  );
}
