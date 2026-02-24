'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useRouter, useSearchParams } from 'next/navigation';
import { api, ListRunsResponse, Run, RunDetail } from '@/lib/api';
import { formatDuration, formatFixed } from '@/lib/format';
import { useAutoRefresh } from '@/lib/useAutoRefresh';
import {
  deleteRunFilterPreset,
  loadRunFilterPresets,
  saveRunFilterPresets,
  type RunFilterPreset,
  upsertRunFilterPreset,
} from '@/lib/presets';

interface RunsTableProps {
  initialData?: ListRunsResponse;
  onRunClick?: (run: Run) => void;
  onStatsChange?: (stats: RunsTableStats) => void;
}

export interface RunsTableStats {
  totalRuns: number;
  pageRuns: number;
  averageDurationSeconds: number | null;
}

type SearchToken = {
  key: string;
  value: string;
  raw: string;
};

type LossInfo = {
  value?: number;
  metricName?: string;
};

const STATUS_STYLES: Record<string, string> = {
  running: 'border-[color:rgba(96,165,250,0.30)] bg-[rgba(96,165,250,0.14)] text-[var(--badge-running-text)]',
  finished: 'border-[color:rgba(52,211,153,0.3)] bg-[rgba(52,211,153,0.14)] text-[var(--badge-finished-text)]',
  failed: 'border-[color:rgba(248,113,113,0.3)] bg-[rgba(248,113,113,0.14)] text-[var(--badge-failed-text)]',
  killed: 'border-[color:rgba(251,191,36,0.3)] bg-[rgba(251,191,36,0.14)] text-[var(--badge-killed-text)]',
  pending: 'border-border bg-surface-secondary text-text-secondary',
};

const STATUS_DOT: Record<string, string> = {
  running: 'bg-[var(--badge-running-text)]',
  finished: 'bg-[var(--badge-finished-text)]',
  failed: 'bg-[var(--badge-failed-text)]',
  killed: 'bg-[var(--badge-killed-text)]',
  pending: 'bg-text-muted',
};

const STATUS_OPTIONS: Array<{ value: string; label: string }> = [
  { value: '', label: 'All status' },
  { value: 'running', label: 'Running' },
  { value: 'finished', label: 'Finished' },
  { value: 'failed', label: 'Failed' },
  { value: 'killed', label: 'Killed' },
  { value: 'pending', label: 'Pending' },
];

const TOKEN_REGEX = /(\w+):("[^"]+"|'[^']+'|\S+)/g;
const TOKEN_KEYS = new Set(['project', 'status', 'tag', 'name', 'id']);
const LOSS_METRIC_PREFERENCE = ['val_loss', 'train/loss', 'train.loss', 'loss'];

function stripTokenQuotes(value: string): string {
  if (
    (value.startsWith('"') && value.endsWith('"')) ||
    (value.startsWith("'") && value.endsWith("'"))
  ) {
    return value.slice(1, -1);
  }
  return value;
}

function parseSearchQuery(input: string): { tokens: SearchToken[]; text: string } {
  const tokens: SearchToken[] = [];
  const textParts: string[] = [];
  let cursor = 0;

  for (const match of input.matchAll(TOKEN_REGEX)) {
    const raw = match[0];
    const start = match.index ?? 0;
    const end = start + raw.length;
    const key = match[1].toLowerCase();
    if (!TOKEN_KEYS.has(key)) continue;

    if (start > cursor) {
      textParts.push(input.slice(cursor, start));
    }

    const value = stripTokenQuotes(match[2]);
    tokens.push({ key, value, raw });
    cursor = end;
  }

  textParts.push(input.slice(cursor));
  const text = textParts.join(' ').replace(/\s+/g, ' ').trim();
  return { tokens, text };
}

function formatToken(key: string, value: string): string {
  const trimmed = value.trim();
  if (!trimmed) return '';
  const needsQuotes = /\s/.test(trimmed);
  return `${key}:${needsQuotes ? `"${trimmed}"` : trimmed}`;
}

function buildSearchQuery(text: string, tokens: SearchToken[]): string {
  const tokenStrings = tokens
    .map((token) => formatToken(token.key, token.value))
    .filter(Boolean);
  const parts = [...tokenStrings, text.trim()].filter(Boolean);
  return parts.join(' ').trim();
}

function normalizePresetQuery(value: string): string {
  return value.trim().replace(/\s+/g, ' ');
}

function updateTokenValue(query: string, key: string, value: string): string {
  const parsed = parseSearchQuery(query);
  const nextTokens = parsed.tokens.filter((token) => token.key !== key);
  const trimmed = value.trim();
  if (trimmed) {
    nextTokens.push({ key, value: trimmed, raw: formatToken(key, trimmed) });
  }
  return buildSearchQuery(parsed.text, nextTokens);
}

function parseTagFilter(value: string): { key: string; value?: string } | null {
  const trimmed = value.trim();
  if (!trimmed) return null;
  const [key, tagValue] = trimmed.split('=', 2);
  if (!key) return null;
  return tagValue ? { key, value: tagValue } : { key };
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

function formatCreatedAt(value: string): string {
  const parsed = parseApiDate(value);
  if (!parsed) return value;
  return parsed.toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

function formatRelativeTime(value: string): string {
  const parsed = parseApiDate(value);
  if (!parsed) return '';
  const deltaSeconds = Math.round((Date.now() - parsed.getTime()) / 1000);
  if (deltaSeconds < 15) return 'just now';
  if (deltaSeconds < 60) return `${deltaSeconds}s ago`;
  if (deltaSeconds < 3600) return `${Math.floor(deltaSeconds / 60)} min ago`;
  if (deltaSeconds < 86400) return `${Math.floor(deltaSeconds / 3600)} hr ago`;
  return `${Math.floor(deltaSeconds / 86400)} day ago`;
}

function formatStatusLabel(status: string): string {
  if (!status) return 'Pending';
  return status.charAt(0).toUpperCase() + status.slice(1);
}

function formatProjectRef(projectId: string): string {
  if (projectId.length <= 16) return projectId;
  return `${projectId.slice(0, 12)}...`;
}

function parseNumericTag(value: string | undefined): number | undefined {
  if (!value) return undefined;
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) return undefined;
  return parsed;
}

function getLossMetricFromSummary(detail: RunDetail | undefined): {
  value?: number;
  metricName?: string;
} {
  const summary = detail?.metrics_summary ?? [];
  if (summary.length === 0) return {};

  const lowered = summary.map((item) => ({
    ...item,
    loweredName: item.name.toLowerCase(),
  }));

  for (const preferredName of LOSS_METRIC_PREFERENCE) {
    const found = lowered.find((entry) => entry.loweredName === preferredName);
    if (found && Number.isFinite(found.last_value)) {
      return {
        value: found.last_value,
        metricName: found.name,
      };
    }
  }

  const fallback = lowered.find((entry) => entry.loweredName.includes('loss'));
  if (fallback && Number.isFinite(fallback.last_value)) {
    return {
      value: fallback.last_value,
      metricName: fallback.name,
    };
  }

  return {};
}

function getLossInfo(run: Run, detail: RunDetail | undefined): LossInfo {
  const fromSummary = getLossMetricFromSummary(detail);
  if (fromSummary.value !== undefined) return fromSummary;

  const tags = run.tags || {};
  const preferredTagKeys = ['val_loss', 'train/loss', 'train.loss', 'loss'];
  for (const key of preferredTagKeys) {
    const value = parseNumericTag(tags[key]);
    if (value !== undefined) {
      return { value, metricName: key };
    }
  }

  for (const [key, value] of Object.entries(tags)) {
    if (!key.toLowerCase().includes('loss')) continue;
    const parsed = parseNumericTag(value);
    if (parsed !== undefined) {
      return { value: parsed, metricName: key };
    }
  }

  return {};
}

function getStepCount(run: Run, detail: RunDetail | undefined): number {
  const summary = detail?.metrics_summary ?? [];
  if (summary.length > 0) {
    const maxStep = summary.reduce((max, metric) => Math.max(max, metric.last_step), 0);
    if (maxStep > 0) return maxStep;
  }
  return run.metrics_count;
}

const SearchIcon = () => (
  <svg className="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M21 21l-5.197-5.197m0 0A7.5 7.5 0 105.196 5.196a7.5 7.5 0 0010.607 10.607z" />
  </svg>
);

const FilterIcon = () => (
  <svg className="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M12 3c2.755 0 5.455.232 8.083.678.533.09.917.556.917 1.096v1.044a2.25 2.25 0 01-.659 1.591l-5.432 5.432a2.25 2.25 0 00-.659 1.591v2.927a2.25 2.25 0 01-1.244 2.013L9.75 21v-6.568a2.25 2.25 0 00-.659-1.591L3.659 7.409A2.25 2.25 0 013 5.818V4.774c0-.54.384-1.006.917-1.096A48.32 48.32 0 0112 3z" />
  </svg>
);

const RefreshIcon = () => (
  <svg className="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M16.023 9.348h4.992v-.001M2.985 19.644v-4.992m0 0h4.992m-4.993 0l3.181 3.183a8.25 8.25 0 0013.803-3.7M4.031 9.865a8.25 8.25 0 0113.803-3.7l3.181 3.182" />
  </svg>
);

const EyeIcon = () => (
  <svg className="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M2.036 12.322a1.012 1.012 0 010-.639C3.423 7.51 7.36 4.5 12 4.5c4.638 0 8.573 3.007 9.963 7.178.07.207.07.431 0 .639C20.577 16.49 16.64 19.5 12 19.5c-4.638 0-8.573-3.007-9.963-7.178z" />
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
  </svg>
);

const CompareIcon = () => (
  <svg className="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M7.5 21L3 16.5m0 0L7.5 12M3 16.5h13.5m0-13.5L21 7.5m0 0L16.5 12M21 7.5H7.5" />
  </svg>
);

const TrashIcon = () => (
  <svg className="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
  </svg>
);

const ChevronLeftIcon = () => (
  <svg className="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15 19l-7-7 7-7" />
  </svg>
);

const ChevronRightIcon = () => (
  <svg className="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 5l7 7-7 7" />
  </svg>
);

export function RunsTable({ initialData, onRunClick, onStatsChange }: RunsTableProps) {
  const router = useRouter();
  const searchParams = useSearchParams();

  const initialQuery = searchParams.get('q') ?? '';
  const initialPageParam = parseInt(searchParams.get('page') ?? '1', 10);
  const initialPage =
    Number.isFinite(initialPageParam) && initialPageParam > 0
      ? initialPageParam - 1
      : 0;

  const [runs, setRuns] = useState<Run[]>(initialData?.runs || []);
  const [total, setTotal] = useState(initialData?.total || 0);
  const [loading, setLoading] = useState(!initialData);
  const [error, setError] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState(initialQuery);
  const [debouncedQuery, setDebouncedQuery] = useState(initialQuery);
  const [page, setPage] = useState(initialPage);
  const [deletingRunId, setDeletingRunId] = useState<string | null>(null);
  const [deleteConfirmId, setDeleteConfirmId] = useState<string | null>(null);
  const [statusMenuOpen, setStatusMenuOpen] = useState(false);
  const [runDetailsById, setRunDetailsById] = useState<Record<string, RunDetail>>({});
  const [savedFilterPresets, setSavedFilterPresets] = useState<RunFilterPreset[]>([]);
  const [activeFilterPresetId, setActiveFilterPresetId] = useState<string>('');

  const statusMenuRef = useRef<HTMLDivElement | null>(null);
  const pageSize = 20;

  const parsedSearch = useMemo(() => parseSearchQuery(searchQuery), [searchQuery]);
  const parsedDebounced = useMemo(() => parseSearchQuery(debouncedQuery), [debouncedQuery]);
  const statusToken = parsedSearch.tokens.find((token) => token.key === 'status');
  const activeStatus = statusToken?.value?.toLowerCase() ?? '';

  const effectiveFilters = useMemo(() => {
    const status = parsedDebounced.tokens.find((token) => token.key === 'status')?.value;
    const project = parsedDebounced.tokens.find((token) => token.key === 'project')?.value;
    const nameTokens = parsedDebounced.tokens
      .filter((token) => token.key === 'name')
      .map((token) => token.value);
    const idTokens = parsedDebounced.tokens
      .filter((token) => token.key === 'id')
      .map((token) => token.value);
    const tagTokens = parsedDebounced.tokens
      .filter((token) => token.key === 'tag')
      .map((token) => token.value)
      .filter(Boolean);

    const tagFilters = tagTokens
      .map(parseTagFilter)
      .filter((tag): tag is { key: string; value?: string } => tag !== null)
      .map((tag) => (tag.value ? `${tag.key}=${tag.value}` : tag.key));

    const queryParts = [parsedDebounced.text, ...nameTokens, ...idTokens]
      .map((part) => part.trim())
      .filter(Boolean);

    const query = queryParts.join(' ').trim();

    return {
      status: status?.toLowerCase(),
      project: project?.trim(),
      query: query || undefined,
      tags: tagFilters.length > 0 ? tagFilters : undefined,
    };
  }, [parsedDebounced]);

  useEffect(() => {
    setSavedFilterPresets(loadRunFilterPresets());
  }, []);

  useEffect(() => {
    const normalized = normalizePresetQuery(searchQuery);
    if (!normalized) {
      setActiveFilterPresetId('');
      return;
    }
    const matchedPreset = savedFilterPresets.find(
      (preset) => normalizePresetQuery(preset.query) === normalized
    );
    setActiveFilterPresetId(matchedPreset?.id || '');
  }, [savedFilterPresets, searchQuery]);

  useEffect(() => {
    const timer = setTimeout(() => {
      setDebouncedQuery(searchQuery);
    }, 250);
    return () => clearTimeout(timer);
  }, [searchQuery]);

  useEffect(() => {
    const params = new URLSearchParams();
    if (debouncedQuery.trim()) params.set('q', debouncedQuery.trim());
    if (page > 0) params.set('page', String(page + 1));
    const queryString = params.toString();
    if (queryString !== searchParams.toString()) {
      router.replace(queryString ? `/?${queryString}` : '/', { scroll: false });
    }
  }, [debouncedQuery, page, router, searchParams]);

  useEffect(() => {
    const nextQuery = searchParams.get('q') ?? '';
    const nextPageParam = parseInt(searchParams.get('page') ?? '1', 10);
    const nextPage =
      Number.isFinite(nextPageParam) && nextPageParam > 0
        ? nextPageParam - 1
        : 0;

    setSearchQuery((current) => (current === nextQuery ? current : nextQuery));
    setDebouncedQuery((current) => (current === nextQuery ? current : nextQuery));
    setPage((current) => (current === nextPage ? current : nextPage));
  }, [searchParams]);

  const fetchRuns = useCallback(
    async ({ silent = false }: { silent?: boolean } = {}) => {
      if (!silent) setLoading(true);
      setError(null);
      try {
        const response = await api.listRuns({
          project: effectiveFilters.project,
          status: effectiveFilters.status || undefined,
          query: effectiveFilters.query,
          tags: effectiveFilters.tags,
          limit: pageSize,
          offset: page * pageSize,
        });
        setRuns(response.runs);
        setTotal(response.total);
      } catch (err) {
        setError(err instanceof Error ? err.message : 'Failed to fetch runs');
      } finally {
        if (!silent) setLoading(false);
      }
    },
    [effectiveFilters, page]
  );

  useEffect(() => {
    void fetchRuns();
  }, [fetchRuns]);

  useAutoRefresh(
    () => fetchRuns({ silent: true }),
    { intervalMs: 15000, enabled: true, runOnMount: false }
  );

  useEffect(() => {
    if (onStatsChange) {
      const durations = runs
        .map((run) => run.duration_seconds)
        .filter((value): value is number => Number.isFinite(value));
      const averageDurationSeconds =
        durations.length > 0
          ? durations.reduce((sum, value) => sum + value, 0) / durations.length
          : null;
      onStatsChange({
        totalRuns: total,
        pageRuns: runs.length,
        averageDurationSeconds,
      });
    }
  }, [onStatsChange, runs, total]);

  useEffect(() => {
    const visibleRunIds = new Set(runs.map((run) => run.run_id));
    setRunDetailsById((current) => {
      const filteredEntries = Object.entries(current).filter(([runId]) => visibleRunIds.has(runId));
      if (filteredEntries.length === Object.keys(current).length) {
        return current;
      }
      return Object.fromEntries(filteredEntries);
    });
  }, [runs]);

  useEffect(() => {
    const missingRunIds = runs
      .map((run) => run.run_id)
      .filter((runId) => !runDetailsById[runId]);

    if (missingRunIds.length === 0) return;

    let active = true;

    const loadMissingDetails = async () => {
      const responses = await Promise.allSettled(
        missingRunIds.map((runId) => api.getRun(runId))
      );

      if (!active) return;

      setRunDetailsById((current) => {
        const next = { ...current };
        responses.forEach((result, index) => {
          if (result.status === 'fulfilled') {
            next[missingRunIds[index]] = result.value;
          }
        });
        return next;
      });
    };

    void loadMissingDetails();

    return () => {
      active = false;
    };
  }, [runDetailsById, runs]);

  useEffect(() => {
    if (!statusMenuOpen) return undefined;

    const handleDocumentClick = (event: MouseEvent) => {
      const target = event.target;
      if (!(target instanceof Node)) return;
      if (!statusMenuRef.current?.contains(target)) {
        setStatusMenuOpen(false);
      }
    };

    document.addEventListener('mousedown', handleDocumentClick);
    return () => {
      document.removeEventListener('mousedown', handleDocumentClick);
    };
  }, [statusMenuOpen]);

  const handleDeleteRun = useCallback(async (runId: string) => {
    setDeletingRunId(runId);
    try {
      await api.deleteRun(runId);
      setRuns((current) => current.filter((run) => run.run_id !== runId));
      setTotal((current) => Math.max(0, current - 1));
      setDeleteConfirmId(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to delete run');
    } finally {
      setDeletingRunId(null);
    }
  }, []);

  const handleSaveFilterPreset = useCallback(() => {
    const normalizedQuery = normalizePresetQuery(searchQuery);
    if (!normalizedQuery) return;

    const defaultName = normalizedQuery.slice(0, 36);
    const requestedName = window.prompt('Save filter preset as:', defaultName);
    if (requestedName === null) return;

    const name = requestedName.trim();
    if (!name) return;

    try {
      const result = upsertRunFilterPreset(savedFilterPresets, name, normalizedQuery);
      setSavedFilterPresets(result.presets);
      setActiveFilterPresetId(result.saved.id);
      saveRunFilterPresets(result.presets);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to save filter preset');
    }
  }, [savedFilterPresets, searchQuery]);

  const handleApplyFilterPreset = useCallback(
    (presetId: string) => {
      if (!presetId) {
        setActiveFilterPresetId('');
        return;
      }

      const preset = savedFilterPresets.find((entry) => entry.id === presetId);
      if (!preset) return;
      setActiveFilterPresetId(preset.id);
      setSearchQuery(preset.query);
      setPage(0);
    },
    [savedFilterPresets]
  );

  const handleDeleteFilterPreset = useCallback(() => {
    if (!activeFilterPresetId) return;
    const nextPresets = deleteRunFilterPreset(savedFilterPresets, activeFilterPresetId);
    setSavedFilterPresets(nextPresets);
    setActiveFilterPresetId('');
    saveRunFilterPresets(nextPresets);
  }, [activeFilterPresetId, savedFilterPresets]);

  const totalPages = Math.ceil(total / pageSize);
  const isSearching = searchQuery.trim().length > 0;
  const runCountLabel = isSearching
    ? `${total} matching runs`
    : `${total} runs`;

  const selectedStatusLabel = STATUS_OPTIONS.find(
    (option) => option.value === activeStatus
  )?.label;

  return (
    <div className="mlrx-fade-up mlrx-fade-up-delay-2 rounded-2xl border border-border bg-surface shadow-[0_0_0_1px_rgba(255,255,255,0.01)]">
      <div className="flex flex-col gap-4 border-b border-border px-5 py-4 sm:px-6">
        <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
          <div className="flex items-center gap-3">
            <h2 className="text-[22px] font-semibold tracking-[-0.02em] text-text-primary">
              Training Runs
            </h2>
            <span className="rounded-full bg-background px-2.5 py-1 text-xs font-medium text-text-muted">
              {runCountLabel}
            </span>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <div className="relative min-w-[220px] flex-1 md:w-[360px]">
              <div className="flex h-11 items-center gap-2 rounded-xl border border-border bg-background px-3 text-text-secondary transition-all duration-200 focus-within:border-accent focus-within:shadow-[0_0_0_3px_rgba(59,125,255,0.12)]">
                <SearchIcon />
                <input
                  type="text"
                  placeholder='Search runs... (project:demo status:finished)'
                  value={searchQuery}
                  onChange={(event) => {
                    setSearchQuery(event.target.value);
                    setPage(0);
                  }}
                  className="w-full bg-transparent text-sm text-text-primary outline-none placeholder:text-text-muted"
                />
              </div>
            </div>

            <div className="relative" ref={statusMenuRef}>
              <button
                type="button"
                onClick={() => setStatusMenuOpen((open) => !open)}
                className="inline-flex h-11 items-center gap-2 rounded-xl border border-border bg-background px-3.5 text-sm font-medium text-text-secondary transition-all duration-200 hover:-translate-y-0.5 hover:border-[color:rgba(255,255,255,0.12)] hover:text-text-primary"
              >
                <FilterIcon />
                {selectedStatusLabel || 'Filter'}
              </button>
              {statusMenuOpen && (
                <div className="absolute right-0 top-12 z-20 min-w-[180px] rounded-xl border border-border bg-surface p-1 shadow-xl">
                  {STATUS_OPTIONS.map((option) => (
                    <button
                      key={option.value || 'all'}
                      type="button"
                      onClick={() => {
                        const nextQuery = updateTokenValue(searchQuery, 'status', option.value);
                        setSearchQuery(nextQuery);
                        setPage(0);
                        setStatusMenuOpen(false);
                      }}
                      className={`block w-full rounded-lg px-2.5 py-2 text-left text-sm ${
                        activeStatus === option.value
                          ? 'bg-sidebar-active text-accent'
                          : 'text-text-secondary hover:bg-surface-hover hover:text-text-primary'
                      }`}
                    >
                      {option.label}
                    </button>
                  ))}
                </div>
              )}
            </div>

            <select
              value={activeFilterPresetId}
              onChange={(event) => handleApplyFilterPreset(event.target.value)}
              className="h-11 rounded-xl border border-border bg-background px-3.5 text-sm font-medium text-text-secondary transition-all duration-200 hover:border-[color:rgba(255,255,255,0.12)] hover:text-text-primary focus:outline-none focus:ring-2 focus:ring-accent/40"
            >
              <option value="">Saved filters</option>
              {savedFilterPresets.map((preset) => (
                <option key={preset.id} value={preset.id}>
                  {preset.name}
                </option>
              ))}
            </select>

            <button
              type="button"
              onClick={handleSaveFilterPreset}
              disabled={!normalizePresetQuery(searchQuery)}
              className="inline-flex h-11 items-center gap-2 rounded-xl border border-border bg-background px-3.5 text-sm font-medium text-text-secondary transition-all duration-200 hover:-translate-y-0.5 hover:border-[color:rgba(255,255,255,0.12)] hover:text-text-primary disabled:cursor-not-allowed disabled:opacity-50 disabled:hover:translate-y-0"
            >
              Save Filter
            </button>

            {activeFilterPresetId && (
              <button
                type="button"
                onClick={handleDeleteFilterPreset}
                className="inline-flex h-11 items-center gap-2 rounded-xl border border-danger/30 bg-danger/10 px-3.5 text-sm font-medium text-danger transition-all duration-200 hover:-translate-y-0.5 hover:bg-danger/15"
              >
                Delete Preset
              </button>
            )}

            <button
              type="button"
              onClick={() => {
                void fetchRuns();
              }}
              className="inline-flex h-11 items-center gap-2 rounded-xl border border-border bg-background px-3.5 text-sm font-medium text-text-secondary transition-all duration-200 hover:-translate-y-0.5 hover:border-[color:rgba(255,255,255,0.12)] hover:text-text-primary"
            >
              <RefreshIcon />
              Refresh
            </button>
          </div>
        </div>

        {isSearching && (
          <button
            type="button"
            onClick={() => {
              setSearchQuery('');
              setPage(0);
            }}
            className="self-start text-xs text-text-muted hover:text-text-secondary"
          >
            Clear filters
          </button>
        )}
      </div>

      {error && (
        <div className="mx-5 mt-4 rounded-lg border border-[color:rgba(248,113,113,0.25)] bg-danger-subtle px-4 py-3 text-sm text-danger sm:mx-6">
          {error}
        </div>
      )}

      <div className="hidden overflow-x-auto md:block">
        <table className="w-full min-w-[980px] border-collapse">
          <thead>
            <tr className="border-y border-border bg-[rgba(0,0,0,0.14)]">
              <th className="px-6 py-3 text-left text-xs font-semibold uppercase tracking-[0.09em] text-text-muted">Name</th>
              <th className="px-4 py-3 text-left text-xs font-semibold uppercase tracking-[0.09em] text-text-muted">Status</th>
              <th className="px-4 py-3 text-left text-xs font-semibold uppercase tracking-[0.09em] text-text-muted">Project</th>
              <th className="px-4 py-3 text-left text-xs font-semibold uppercase tracking-[0.09em] text-text-muted">Steps</th>
              <th className="px-4 py-3 text-left text-xs font-semibold uppercase tracking-[0.09em] text-text-muted">Loss</th>
              <th className="px-4 py-3 text-left text-xs font-semibold uppercase tracking-[0.09em] text-text-muted">Duration</th>
              <th className="px-4 py-3 text-left text-xs font-semibold uppercase tracking-[0.09em] text-text-muted">Created</th>
              <th className="px-4 py-3 text-right text-xs font-semibold uppercase tracking-[0.09em] text-text-muted" />
            </tr>
          </thead>
          <tbody>
            {loading ? (
              <tr>
                <td colSpan={8} className="px-6 py-10 text-center text-sm text-text-muted">
                  Loading runs...
                </td>
              </tr>
            ) : runs.length === 0 ? (
              <tr>
                <td colSpan={8} className="px-6 py-10 text-center text-sm text-text-muted">
                  No runs found for the current filters.
                </td>
              </tr>
            ) : (
              runs.map((run, index) => {
                const details = runDetailsById[run.run_id];
                const lossInfo = getLossInfo(run, details);
                const stepCount = getStepCount(run, details);
                const relativeCreatedAt = formatRelativeTime(run.created_at);
                return (
                  <tr
                    key={run.run_id}
                    onClick={() => onRunClick?.(run)}
                    className="mlrx-row-enter group border-b border-border-subtle text-sm transition-all duration-200 odd:bg-[rgba(255,255,255,0.01)] hover:bg-surface-hover"
                    style={{ animationDelay: `${Math.min(index * 35, 210)}ms` }}
                  >
                    <td className="px-6 py-4">
                      <div className="text-[15px] font-semibold tracking-[-0.01em] text-text-primary">
                        {run.name || run.run_id.slice(0, 8)}
                      </div>
                      <div className="font-mono text-xs text-text-muted">{run.run_id.slice(0, 12)}</div>
                    </td>
                    <td className="px-4 py-4">
                      <span
                        className={`inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs font-semibold ${
                          STATUS_STYLES[run.status] || STATUS_STYLES.pending
                        }`}
                      >
                        <span className={`h-1.5 w-1.5 rounded-full ${STATUS_DOT[run.status] || STATUS_DOT.pending}`} />
                        {formatStatusLabel(run.status)}
                      </span>
                    </td>
                    <td className="px-4 py-4">
                      <span className="inline-flex max-w-[128px] truncate rounded-md border border-border bg-background px-2.5 py-1 font-mono text-xs text-text-secondary" title={run.project_id}>
                        {formatProjectRef(run.project_id)}
                      </span>
                    </td>
                    <td className="px-4 py-4 font-mono text-[15px] text-text-primary">{stepCount.toLocaleString()}</td>
                    <td className="px-4 py-4">
                      <div className="font-mono text-[15px] text-text-primary">
                        {lossInfo.value !== undefined ? formatFixed(lossInfo.value, 3) : '-'}
                      </div>
                      <div className="text-xs text-text-muted">{lossInfo.metricName || 'loss'}</div>
                    </td>
                    <td className="px-4 py-4 font-mono text-[15px] text-text-secondary">
                      {formatDuration(run.duration_seconds)}
                    </td>
                    <td className="px-4 py-4">
                      <div className="text-[15px] text-text-secondary">{formatCreatedAt(run.created_at)}</div>
                      <div className="text-xs text-text-muted">{relativeCreatedAt}</div>
                    </td>
                    <td className="px-4 py-4">
                      <div className="flex justify-end gap-1 opacity-0 transition-opacity duration-200 group-hover:opacity-100">
                        {deleteConfirmId === run.run_id ? (
                          <>
                            <button
                              type="button"
                              onClick={(event) => {
                                event.stopPropagation();
                                void handleDeleteRun(run.run_id);
                              }}
                              disabled={deletingRunId === run.run_id}
                              className="rounded-md bg-danger px-2.5 py-1 text-xs font-semibold text-white disabled:opacity-60"
                            >
                              {deletingRunId === run.run_id ? '...' : 'Confirm'}
                            </button>
                            <button
                              type="button"
                              onClick={(event) => {
                                event.stopPropagation();
                                setDeleteConfirmId(null);
                              }}
                              className="rounded-md border border-border bg-background px-2.5 py-1 text-xs font-medium text-text-secondary hover:text-text-primary"
                            >
                              Cancel
                            </button>
                          </>
                        ) : (
                          <>
                            <button
                              type="button"
                              title="View run"
                              onClick={(event) => {
                                event.stopPropagation();
                                onRunClick?.(run);
                              }}
                              className="inline-flex h-8 w-8 items-center justify-center rounded-md border border-transparent text-text-muted hover:border-border hover:bg-surface-secondary hover:text-text-primary"
                            >
                              <EyeIcon />
                            </button>
                            <button
                              type="button"
                              title="Compare run"
                              onClick={(event) => {
                                event.stopPropagation();
                                router.push(`/compare?runs=${encodeURIComponent(run.run_id)}`);
                              }}
                              className="inline-flex h-8 w-8 items-center justify-center rounded-md border border-transparent text-text-muted hover:border-border hover:bg-surface-secondary hover:text-text-primary"
                            >
                              <CompareIcon />
                            </button>
                            <button
                              type="button"
                              title="Delete run"
                              onClick={(event) => {
                                event.stopPropagation();
                                setDeleteConfirmId(run.run_id);
                              }}
                              className="inline-flex h-8 w-8 items-center justify-center rounded-md border border-transparent text-text-muted hover:border-danger/40 hover:bg-danger/10 hover:text-danger"
                            >
                              <TrashIcon />
                            </button>
                          </>
                        )}
                      </div>
                    </td>
                  </tr>
                );
              })
            )}
          </tbody>
        </table>
      </div>

      <div className="space-y-3 p-4 md:hidden">
        {loading ? (
          <div className="rounded-xl border border-border bg-background px-4 py-6 text-center text-sm text-text-muted">
            Loading runs...
          </div>
        ) : runs.length === 0 ? (
          <div className="rounded-xl border border-border bg-background px-4 py-6 text-center text-sm text-text-muted">
            No runs found.
          </div>
        ) : (
          runs.map((run) => {
            const details = runDetailsById[run.run_id];
            const lossInfo = getLossInfo(run, details);
            const stepCount = getStepCount(run, details);
            return (
              <div key={run.run_id} className="rounded-xl border border-border bg-background p-4 transition-all duration-200 hover:border-[color:rgba(91,148,255,0.32)]">
                <div className="flex items-start justify-between gap-3">
                  <button
                    type="button"
                    onClick={() => onRunClick?.(run)}
                    className="min-w-0 flex-1 text-left"
                  >
                    <div className="truncate text-base font-semibold text-text-primary">
                      {run.name || run.run_id.slice(0, 8)}
                    </div>
                    <div className="truncate font-mono text-xs text-text-muted">{run.run_id}</div>
                  </button>
                  <span
                    className={`inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs font-semibold ${
                      STATUS_STYLES[run.status] || STATUS_STYLES.pending
                    }`}
                  >
                    <span className={`h-1.5 w-1.5 rounded-full ${STATUS_DOT[run.status] || STATUS_DOT.pending}`} />
                    {formatStatusLabel(run.status)}
                  </span>
                </div>
                <div className="mt-3 grid grid-cols-2 gap-2 text-xs">
                  <div className="rounded-md border border-border bg-surface-secondary px-2.5 py-2">
                    <div className="text-text-muted">Project</div>
                    <div className="mt-0.5 truncate font-mono text-text-secondary" title={run.project_id}>
                      {formatProjectRef(run.project_id)}
                    </div>
                  </div>
                  <div className="rounded-md border border-border bg-surface-secondary px-2.5 py-2">
                    <div className="text-text-muted">Steps</div>
                    <div className="mt-0.5 font-mono text-text-primary">{stepCount.toLocaleString()}</div>
                  </div>
                  <div className="rounded-md border border-border bg-surface-secondary px-2.5 py-2">
                    <div className="text-text-muted">Loss</div>
                    <div className="mt-0.5 font-mono text-text-primary">
                      {lossInfo.value !== undefined ? formatFixed(lossInfo.value, 3) : '-'}
                    </div>
                  </div>
                  <div className="rounded-md border border-border bg-surface-secondary px-2.5 py-2">
                    <div className="text-text-muted">Duration</div>
                    <div className="mt-0.5 font-mono text-text-primary">{formatDuration(run.duration_seconds)}</div>
                  </div>
                </div>
                <div className="mt-3 flex items-center justify-between">
                  <div>
                    <div className="text-xs text-text-secondary">{formatCreatedAt(run.created_at)}</div>
                    <div className="text-[11px] text-text-muted">{formatRelativeTime(run.created_at)}</div>
                  </div>
                  <div className="flex gap-1">
                    <button
                      type="button"
                      onClick={() => onRunClick?.(run)}
                      className="inline-flex h-8 w-8 items-center justify-center rounded-md border border-border text-text-secondary"
                    >
                      <EyeIcon />
                    </button>
                    <button
                      type="button"
                      onClick={() => router.push(`/compare?runs=${encodeURIComponent(run.run_id)}`)}
                      className="inline-flex h-8 w-8 items-center justify-center rounded-md border border-border text-text-secondary"
                    >
                      <CompareIcon />
                    </button>
                    <button
                      type="button"
                      onClick={() => setDeleteConfirmId(run.run_id)}
                      className="inline-flex h-8 w-8 items-center justify-center rounded-md border border-danger/30 text-danger"
                    >
                      <TrashIcon />
                    </button>
                  </div>
                </div>
                {deleteConfirmId === run.run_id && (
                  <div className="mt-3 flex items-center justify-end gap-2">
                    <button
                      type="button"
                      onClick={() => {
                        void handleDeleteRun(run.run_id);
                      }}
                      disabled={deletingRunId === run.run_id}
                      className="rounded-md bg-danger px-2.5 py-1 text-xs font-semibold text-white disabled:opacity-60"
                    >
                      {deletingRunId === run.run_id ? 'Deleting...' : 'Confirm delete'}
                    </button>
                    <button
                      type="button"
                      onClick={() => setDeleteConfirmId(null)}
                      className="rounded-md border border-border px-2.5 py-1 text-xs text-text-secondary"
                    >
                      Cancel
                    </button>
                  </div>
                )}
              </div>
            );
          })
        )}
      </div>

      {totalPages > 1 && (
        <div className="flex flex-col gap-2 border-t border-border px-5 py-3 sm:flex-row sm:items-center sm:justify-between sm:px-6">
          <div className="text-xs text-text-muted">
            {page * pageSize + 1} - {Math.min((page + 1) * pageSize, total)} of {total}
          </div>
          <div className="flex items-center gap-1">
            <button
              type="button"
              onClick={() => setPage((current) => Math.max(0, current - 1))}
              disabled={page === 0}
              className="inline-flex h-8 w-8 items-center justify-center rounded-md border border-border text-text-secondary disabled:opacity-40"
            >
              <ChevronLeftIcon />
            </button>
            <div className="px-2 text-xs text-text-secondary">
              {page + 1} / {totalPages}
            </div>
            <button
              type="button"
              onClick={() => setPage((current) => Math.min(totalPages - 1, current + 1))}
              disabled={page >= totalPages - 1}
              className="inline-flex h-8 w-8 items-center justify-center rounded-md border border-border text-text-secondary disabled:opacity-40"
            >
              <ChevronRightIcon />
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
