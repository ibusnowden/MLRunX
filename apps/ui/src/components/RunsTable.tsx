'use client';

import { useState, useEffect, useCallback, useMemo } from 'react';
import { useRouter, useSearchParams } from 'next/navigation';
import { api, Run, ListRunsResponse } from '@/lib/api';

interface RunsTableProps {
  initialData?: ListRunsResponse;
  onRunClick?: (run: Run) => void;
}

const STATUS_STYLES: Record<string, string> = {
  running: 'bg-[var(--badge-running-bg)] text-[var(--badge-running-text)]',
  finished: 'bg-[var(--badge-finished-bg)] text-[var(--badge-finished-text)]',
  failed: 'bg-[var(--badge-failed-bg)] text-[var(--badge-failed-text)]',
  killed: 'bg-[var(--badge-killed-bg)] text-[var(--badge-killed-text)]',
  pending: 'bg-[var(--badge-pending-bg)] text-[var(--badge-pending-text)]',
};

const STATUS_DOT: Record<string, string> = {
  running: 'bg-accent animate-pulse',
  finished: 'bg-success',
  failed: 'bg-danger',
  killed: 'bg-warning',
  pending: 'bg-text-muted',
};

const TOKEN_REGEX = /(\w+):("[^"]+"|'[^']+'|\S+)/g;
const TOKEN_KEYS = new Set(['project', 'status', 'tag', 'name', 'id']);

type SearchToken = {
  key: string;
  value: string;
  raw: string;
};

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

    if (!TOKEN_KEYS.has(key)) {
      continue;
    }

    if (start > cursor) {
      textParts.push(input.slice(cursor, start));
    }

    const rawValue = match[2];
    const value = stripTokenQuotes(rawValue);
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
  const tokenStrings = tokens.map((token) => formatToken(token.key, token.value)).filter(Boolean);
  const parts = [...tokenStrings, text.trim()].filter(Boolean);
  return parts.join(' ').trim();
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

function formatDuration(seconds: number | null): string {
  if (seconds === null) return '-';
  if (seconds < 60) return `${seconds.toFixed(1)}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${Math.floor(seconds % 60)}s`;
  return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`;
}

function formatDate(dateStr: string): string {
  const match = dateStr.match(/tv_sec:\s*(\d+)/);
  if (match) {
    const timestamp = parseInt(match[1], 10) * 1000;
    return new Date(timestamp).toLocaleString();
  }
  return dateStr;
}

// ── Icons ──
const SearchIcon = () => (
  <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
  </svg>
);

const RefreshIcon = () => (
  <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
  </svg>
);

const ChevronLeftIcon = () => (
  <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15 19l-7-7 7-7" />
  </svg>
);

const ChevronRightIcon = () => (
  <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 5l7 7-7 7" />
  </svg>
);

const TrashIcon = () => (
  <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
  </svg>
);

export function RunsTable({ initialData, onRunClick }: RunsTableProps) {
  const router = useRouter();
  const searchParams = useSearchParams();
  const initialQuery = searchParams.get('q') ?? '';
  const initialPageParam = parseInt(searchParams.get('page') ?? '1', 10);
  const initialPage = Number.isFinite(initialPageParam) && initialPageParam > 0
    ? initialPageParam - 1
    : 0;

  const [runs, setRuns] = useState<Run[]>(initialData?.runs || []);
  const [total, setTotal] = useState(initialData?.total || 0);
  const [loading, setLoading] = useState(!initialData);
  const [error, setError] = useState<string | null>(null);

  // Delete state
  const [deletingRunId, setDeletingRunId] = useState<string | null>(null);
  const [deleteConfirmId, setDeleteConfirmId] = useState<string | null>(null);

  // Filters
  const [searchQuery, setSearchQuery] = useState(initialQuery);
  const [debouncedQuery, setDebouncedQuery] = useState(initialQuery);

  // Pagination
  const [page, setPage] = useState(initialPage);
  const pageSize = 20;

  const parsedSearch = useMemo(() => parseSearchQuery(searchQuery), [searchQuery]);
  const parsedDebounced = useMemo(() => parseSearchQuery(debouncedQuery), [debouncedQuery]);

  const statusToken = parsedSearch.tokens.find((token) => token.key === 'status');

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
    const debounce = setTimeout(() => {
      setDebouncedQuery(searchQuery);
    }, 250);

    return () => clearTimeout(debounce);
  }, [searchQuery]);

  useEffect(() => {
    const params = new URLSearchParams();
    if (searchQuery.trim()) params.set('q', searchQuery.trim());
    if (page > 0) params.set('page', String(page + 1));
    const queryString = params.toString();
    const currentQuery = searchParams.toString();
    if (queryString !== currentQuery) {
      router.replace(queryString ? `/?${queryString}` : '/', { scroll: false });
    }
  }, [searchQuery, page, router, searchParams]);

  useEffect(() => {
    const nextQuery = searchParams.get('q') ?? '';
    const nextPageParam = parseInt(searchParams.get('page') ?? '1', 10);
    const nextPage = Number.isFinite(nextPageParam) && nextPageParam > 0
      ? nextPageParam - 1
      : 0;

    if (nextQuery !== searchQuery) {
      setSearchQuery(nextQuery);
    }
    if (nextPage !== page) {
      setPage(nextPage);
    }
  }, [searchParams]);

  const fetchRuns = useCallback(async () => {
    setLoading(true);
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
      setLoading(false);
    }
  }, [effectiveFilters, page]);

  useEffect(() => {
    fetchRuns();
  }, [fetchRuns]);

  const handleDeleteRun = useCallback(async (runId: string) => {
    setDeletingRunId(runId);
    try {
      await api.deleteRun(runId);
      // Remove from local state immediately
      setRuns((prev) => prev.filter((r) => r.run_id !== runId));
      setTotal((prev) => Math.max(0, prev - 1));
      setDeleteConfirmId(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to delete run');
    } finally {
      setDeletingRunId(null);
    }
  }, []);

  const totalPages = Math.ceil(total / pageSize);
  const isSearching = searchQuery.trim().length > 0;
  const resultsLabel = isSearching
    ? `${total} matching runs`
    : `${total} total runs`;

  return (
    <div className="w-full">
      {/* Toolbar */}
      <div className="flex flex-wrap gap-3 items-center mb-4">
        {/* Search */}
        <div className="relative flex-1 min-w-[280px]">
          <div className="absolute inset-y-0 left-0 flex items-center pl-3 pointer-events-none text-text-muted">
            <SearchIcon />
          </div>
          <input
            type="text"
            placeholder="Search runs... (e.g. project:demo status:finished tag:model=toy)"
            value={searchQuery}
            onChange={(e) => {
              setSearchQuery(e.target.value);
              setPage(0);
            }}
            className="w-full pl-10 pr-4 py-2.5 text-sm rounded-lg border border-border bg-surface-secondary text-text-primary placeholder:text-text-muted focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
          />
        </div>

        {/* Status filter */}
        <select
          value={statusToken?.value ?? ''}
          onChange={(e) => {
            const nextQuery = updateTokenValue(searchQuery, 'status', e.target.value);
            setSearchQuery(nextQuery);
            setPage(0);
          }}
          className="px-3 py-2.5 text-sm rounded-lg border border-border bg-surface-secondary text-text-primary focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
        >
          <option value="">All Status</option>
          <option value="running">Running</option>
          <option value="finished">Finished</option>
          <option value="failed">Failed</option>
          <option value="killed">Killed</option>
        </select>

        {/* Clear */}
        {isSearching && (
          <button
            onClick={() => {
              setSearchQuery('');
              setPage(0);
            }}
            className="px-3 py-2.5 text-sm rounded-lg border border-border text-text-secondary hover:bg-surface-hover hover:text-text-primary transition-colors"
          >
            Clear
          </button>
        )}

        {/* Refresh */}
        <button
          onClick={fetchRuns}
          className="flex items-center gap-2 px-4 py-2.5 text-sm rounded-lg bg-accent text-white hover:bg-accent-hover transition-colors font-medium"
        >
          <RefreshIcon />
          Refresh
        </button>
      </div>

      {/* Results count */}
      <div className="mb-3 text-xs text-text-muted">{resultsLabel}</div>

      {/* Error */}
      {error && (
        <div className="mb-4 p-4 rounded-lg bg-danger-subtle border border-danger/20 text-danger">
          {error}
        </div>
      )}

      {/* Table */}
      <div className="overflow-x-auto rounded-lg border border-border">
        <table className="w-full">
          <thead>
            <tr className="bg-surface-secondary">
              <th className="px-4 py-3 text-left text-xs font-semibold text-text-secondary uppercase tracking-wider">Name</th>
              <th className="px-4 py-3 text-left text-xs font-semibold text-text-secondary uppercase tracking-wider">Status</th>
              <th className="px-4 py-3 text-left text-xs font-semibold text-text-secondary uppercase tracking-wider">Project</th>
              <th className="px-4 py-3 text-left text-xs font-semibold text-text-secondary uppercase tracking-wider">Metrics</th>
              <th className="px-4 py-3 text-left text-xs font-semibold text-text-secondary uppercase tracking-wider">Duration</th>
              <th className="px-4 py-3 text-left text-xs font-semibold text-text-secondary uppercase tracking-wider">Created</th>
              <th className="px-4 py-3 text-right text-xs font-semibold text-text-secondary uppercase tracking-wider w-16"></th>
            </tr>
          </thead>
          <tbody className="divide-y divide-border">
            {loading ? (
              <tr>
                <td colSpan={7} className="px-4 py-12 text-center text-text-muted">
                  <div className="flex items-center justify-center gap-2">
                    <svg className="animate-spin h-5 w-5" viewBox="0 0 24 24">
                      <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" fill="none" />
                      <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z" />
                    </svg>
                    Loading runs...
                  </div>
                </td>
              </tr>
            ) : runs.length === 0 ? (
              <tr>
                <td colSpan={7} className="px-4 py-12 text-center text-text-muted">
                  No runs found
                </td>
              </tr>
            ) : (
              runs.map((run) => (
                <tr
                  key={run.run_id}
                  onClick={() => onRunClick?.(run)}
                  className="group hover:bg-surface-hover cursor-pointer transition-colors"
                >
                  <td className="px-4 py-3.5">
                    <div className="font-medium text-text-primary">
                      {run.name || run.run_id.slice(0, 8)}
                    </div>
                    <div className="text-xs text-text-muted font-mono mt-0.5">
                      {run.run_id.slice(0, 12)}
                    </div>
                  </td>
                  <td className="px-4 py-3.5">
                    <span
                      className={`inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-medium ${
                        STATUS_STYLES[run.status] || STATUS_STYLES.pending
                      }`}
                    >
                      <span className={`w-1.5 h-1.5 rounded-full ${STATUS_DOT[run.status] || STATUS_DOT.pending}`} />
                      {run.status}
                    </span>
                  </td>
                  <td className="px-4 py-3.5 text-sm text-text-secondary">{run.project_id}</td>
                  <td className="px-4 py-3.5">
                    <span className="text-sm font-mono text-text-secondary">{run.metrics_count}</span>
                  </td>
                  <td className="px-4 py-3.5 text-sm text-text-secondary font-mono">
                    {formatDuration(run.duration_seconds)}
                  </td>
                  <td className="px-4 py-3.5 text-sm text-text-muted">
                    {formatDate(run.created_at)}
                  </td>
                  <td className="px-4 py-3.5 text-right">
                    {deleteConfirmId === run.run_id ? (
                      <div className="flex items-center gap-1 justify-end" onClick={(e) => e.stopPropagation()}>
                        <button
                          onClick={(e) => {
                            e.stopPropagation();
                            handleDeleteRun(run.run_id);
                          }}
                          disabled={deletingRunId === run.run_id}
                          className="px-2 py-1 text-xs font-medium rounded bg-danger text-white hover:bg-danger/80 disabled:opacity-50 transition-colors"
                        >
                          {deletingRunId === run.run_id ? '...' : 'Yes'}
                        </button>
                        <button
                          onClick={(e) => {
                            e.stopPropagation();
                            setDeleteConfirmId(null);
                          }}
                          className="px-2 py-1 text-xs font-medium rounded bg-surface-secondary text-text-secondary hover:bg-surface-hover border border-border transition-colors"
                        >
                          No
                        </button>
                      </div>
                    ) : (
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          setDeleteConfirmId(run.run_id);
                        }}
                        className="p-1.5 rounded hover:bg-danger/10 text-text-muted hover:text-danger transition-colors opacity-0 group-hover:opacity-100"
                        title="Delete run"
                      >
                        <TrashIcon />
                      </button>
                    )}
                  </td>
                </tr>
              ))
            )}
          </tbody>
        </table>
      </div>

      {/* Pagination */}
      {totalPages > 1 && (
        <div className="mt-4 flex items-center justify-between">
          <div className="text-sm text-text-muted">
            {page * pageSize + 1}-{Math.min((page + 1) * pageSize, total)} of {total}
          </div>
          <div className="flex items-center gap-1">
            <button
              onClick={() => setPage((p) => Math.max(0, p - 1))}
              disabled={page === 0}
              className="p-2 rounded-lg border border-border text-text-secondary hover:bg-surface-hover disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
            >
              <ChevronLeftIcon />
            </button>
            <span className="px-3 py-1.5 text-sm text-text-secondary">
              {page + 1} / {totalPages}
            </span>
            <button
              onClick={() => setPage((p) => Math.min(totalPages - 1, p + 1))}
              disabled={page >= totalPages - 1}
              className="p-2 rounded-lg border border-border text-text-secondary hover:bg-surface-hover disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
            >
              <ChevronRightIcon />
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
