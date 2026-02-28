'use client';

import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { api, MetricSeries } from '@/lib/api';
import { UPlotChart } from '@/components/charts/UPlotChart';
import { createSeriesColorScale } from '@/components/charts/chartColors';
import { useTheme } from '@/components/ThemeProvider';
import { formatFixed, safeMinMax } from '@/lib/format';
import { useAutoRefresh } from '@/lib/useAutoRefresh';
import { computeDerivedSeries, derivedModeLabel, type DerivedSeriesMode } from '@/lib/computedSeries';

// Icons
const ExpandIcon = () => (
  <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 8V4m0 0h4M4 4l5 5m11-1V4m0 0h-4m4 0l-5 5M4 16v4m0 0h4m-4 0l5-5m11 5l-5-5m5 5v-4m0 4h-4" />
  </svg>
);

const CollapseIcon = () => (
  <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
  </svg>
);

const DownloadIcon = () => (
  <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4" />
  </svg>
);

interface CompareData {
  run_id: string;
  run_name: string | null;
  status: string;
  series: MetricSeries[];
  metric_aliases?: Record<string, string>;
}

interface ComparePanelProps {
  runIds: string[];
}

const COMPARE_MAX_POINTS = 5000;
const COMPARE_PAGE_SIZE = 200;

function normalizeRunIds(runIds: string[]): string[] {
  const seen = new Set<string>();
  const normalized: string[] = [];
  for (const id of runIds) {
    const trimmed = id.trim();
    if (!trimmed || seen.has(trimmed)) continue;
    seen.add(trimmed);
    normalized.push(trimmed);
  }
  return normalized;
}

function computeCommonMetricNames(runs: CompareData[]): string[] {
  if (runs.length === 0) return [];
  let common = new Set<string>(runs[0].series.map((series) => series.name));
  for (const run of runs.slice(1)) {
    const runMetricNames = new Set<string>(run.series.map((series) => series.name));
    common = new Set([...common].filter((name) => runMetricNames.has(name)));
  }
  return [...common].sort();
}

export function ComparePanel({ runIds }: ComparePanelProps) {
  const { isDark } = useTheme();
  const [runs, setRuns] = useState<CompareData[]>([]);
  const [commonMetrics, setCommonMetrics] = useState<string[]>([]);
  const [selectedMetric, setSelectedMetric] = useState<string>('');
  const [derivedMode, setDerivedMode] = useState<DerivedSeriesMode>('none');
  const [derivedWindow, setDerivedWindow] = useState(16);
  const [showRawSeries, setShowRawSeries] = useState(true);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [isExpanded, setIsExpanded] = useState(false);
  const expandedChartRef = useRef<HTMLDivElement>(null);

  // Close expanded view on Escape key
  useEffect(() => {
    if (!isExpanded) return;
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setIsExpanded(false);
    };
    document.addEventListener('keydown', handleKeyDown);
    // Prevent body scroll when expanded
    document.body.style.overflow = 'hidden';
    return () => {
      document.removeEventListener('keydown', handleKeyDown);
      document.body.style.overflow = '';
    };
  }, [isExpanded]);

  const handleDownload = useCallback(() => {
    const container = expandedChartRef.current ?? document.querySelector('[data-compare-chart]');
    const canvas = container?.querySelector('canvas');
    if (canvas) {
      const link = document.createElement('a');
      link.download = `${selectedMetric || 'compare'}_chart.png`;
      link.href = canvas.toDataURL('image/png');
      link.click();
    }
  }, [selectedMetric]);

  const fetchComparison = useCallback(async ({ silent = false }: { silent?: boolean } = {}) => {
    const normalizedRunIds = normalizeRunIds(runIds);
    if (normalizedRunIds.length === 0) {
      setRuns([]);
      setCommonMetrics([]);
      setSelectedMetric('');
      setNotice(null);
      setLoading(false);
      return;
    }

    if (!silent) {
      setLoading(true);
    }
    setError(null);
    setNotice(null);

    try {
      const collectedRuns: CompareData[] = [];
      let offset = 0;
      let total = normalizedRunIds.length;
      while (offset < total) {
        const response = await api.compareRuns(
          normalizedRunIds,
          [],
          COMPARE_MAX_POINTS,
          { limit: COMPARE_PAGE_SIZE, offset }
        );
        collectedRuns.push(...response.runs);

        if (typeof response.total === 'number') {
          total = response.total;
        }
        if (response.runs.length === 0) {
          break;
        }
        offset += response.runs.length;
      }

      const computedCommonMetrics = computeCommonMetricNames(collectedRuns);
      setRuns(collectedRuns);
      setCommonMetrics(computedCommonMetrics);
      setSelectedMetric((prev) => {
        if (computedCommonMetrics.length === 0) return '';
        if (prev && computedCommonMetrics.includes(prev)) return prev;
        return computedCommonMetrics[0];
      });
      if (collectedRuns.length > COMPARE_PAGE_SIZE) {
        const pageCount = Math.ceil(collectedRuns.length / COMPARE_PAGE_SIZE);
        setNotice(`Loaded ${collectedRuns.length} runs across ${pageCount} compare pages.`);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to compare runs');
    } finally {
      if (!silent) {
        setLoading(false);
      }
    }
  }, [runIds]);

  useEffect(() => {
    void fetchComparison();
  }, [fetchComparison]);

  useAutoRefresh(
    () => fetchComparison({ silent: true }),
    { enabled: runIds.length > 0, intervalMs: 30000, runOnMount: false }
  );

  useEffect(() => {
    if (derivedMode === 'none') {
      setShowRawSeries(true);
    }
  }, [derivedMode]);

  const seriesLabels = useMemo(
    () => runs.map((run) => run.run_name || run.run_id.slice(0, 8)),
    [runs]
  );
  const runColorScale = useMemo(
    () => createSeriesColorScale(seriesLabels, isDark),
    [seriesLabels, isDark]
  );
  const metricDisplayNameByRaw = useMemo(() => {
    const map = new Map<string, string>();
    for (const run of runs) {
      const aliasMap = run.metric_aliases ?? {};
      for (const [metricName, alias] of Object.entries(aliasMap)) {
        const normalized = alias.trim();
        if (!normalized || map.has(metricName)) continue;
        map.set(metricName, normalized);
      }
    }
    return map;
  }, [runs]);
  const selectedMetricLabel = useMemo(
    () => metricDisplayNameByRaw.get(selectedMetric) ?? selectedMetric,
    [metricDisplayNameByRaw, selectedMetric]
  );
  const computedLabel = useMemo(
    () => derivedModeLabel(derivedMode, derivedWindow),
    [derivedMode, derivedWindow]
  );
  const yAxisLabel = useMemo(() => {
    if (derivedMode === 'pct_change' && !showRawSeries) {
      return `${selectedMetricLabel} (% change)`;
    }
    if (derivedMode !== 'none' && !showRawSeries) {
      return `${selectedMetricLabel} (${computedLabel})`;
    }
    return selectedMetricLabel;
  }, [computedLabel, derivedMode, selectedMetricLabel, showRawSeries]);

  // Get series for selected metric from each run
  const comparisonData = runs.map((run, idx) => {
    const label = run.run_name || run.run_id.slice(0, 8);
    return {
      ...run,
      label,
      color: runColorScale(label, idx),
      metricSeries: run.series.find((s) => s.name === selectedMetric),
    };
  });

  if (runIds.length === 0) {
    return (
      <div className="bg-surface rounded-xl border border-border p-6">
        <div className="text-center py-8 text-text-muted">
          Select runs to compare from the runs table
        </div>
      </div>
    );
  }

  if (loading) {
    return (
      <div className="bg-surface rounded-xl border border-border p-6">
        <div className="text-center py-8 text-text-muted">Loading comparison...</div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="bg-surface rounded-xl border border-border p-6">
        <div className="p-4 bg-danger-subtle border border-danger/20 rounded-lg text-danger">
          {error}
        </div>
      </div>
    );
  }

  // Find all steps
  const allSteps = new Set<number>();
  comparisonData.forEach((r) => {
    r.metricSeries?.points.forEach((p) => allSteps.add(p.step));
  });
  const sortedSteps = Array.from(allSteps).sort((a, b) => a - b);
  const stepToIndex = new Map(sortedSteps.map((step, idx) => [step, idx]));
  const chartSeries = comparisonData.flatMap((run) => {
    const points = run.metricSeries?.points ?? [];
    const data = new Array<number>(sortedSteps.length).fill(Number.NaN);
    const upper = new Array<number>(sortedSteps.length).fill(Number.NaN);
    const lower = new Array<number>(sortedSteps.length).fill(Number.NaN);
    points.forEach((point) => {
      const idx = stepToIndex.get(point.step);
      if (idx !== undefined) {
        data[idx] = point.mean;
        upper[idx] = point.max;
        lower[idx] = point.min;
      }
    });
    const entries: Array<{
      label: string;
      color: string;
      data: number[];
      upper?: number[];
      lower?: number[];
    }> = [];

    if (showRawSeries || derivedMode === 'none') {
      entries.push({
        label: run.label,
        color: run.color,
        data,
        upper,
        lower,
      });
    }

    if (derivedMode !== 'none') {
      const derivedData = computeDerivedSeries(data, derivedMode, derivedWindow);
      entries.push({
        label: `${run.label} ${computedLabel}`,
        color: run.color,
        data: derivedData,
      });
    }

    return entries;
  });

  return (
    <div className="bg-surface rounded-xl border border-border p-4 sm:p-6">
      <div className="flex flex-col gap-3 mb-4">
        <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
          <h2 className="text-lg sm:text-xl font-semibold text-text-primary">Compare Runs</h2>
          {commonMetrics.length > 0 && (
            <select
              value={selectedMetric}
              onChange={(e) => setSelectedMetric(e.target.value)}
              className="w-full sm:w-auto px-3 py-2 border border-border rounded-lg text-text-primary bg-surface-secondary focus:outline-none focus:ring-2 focus:ring-accent"
            >
              {commonMetrics.map((name) => (
                <option key={name} value={name}>
                  {metricDisplayNameByRaw.get(name) || name}
                </option>
              ))}
            </select>
          )}
        </div>
        <div className="flex flex-wrap items-end gap-2">
          <label className="text-xs text-text-muted">
            Derived Series
            <select
              value={derivedMode}
              onChange={(event) => setDerivedMode(event.target.value as DerivedSeriesMode)}
              className="mt-1 block rounded-md border border-border bg-surface-secondary px-2 py-1.5 text-sm text-text-primary focus:outline-none focus:ring-2 focus:ring-accent"
            >
              <option value="none">Raw only</option>
              <option value="ema">EMA</option>
              <option value="delta">Delta</option>
              <option value="pct_change">% Change</option>
              <option value="cumulative_avg">Cumulative Avg</option>
            </select>
          </label>
          {derivedMode === 'ema' && (
            <label className="text-xs text-text-muted">
              EMA Window
              <input
                type="number"
                min={2}
                max={512}
                value={derivedWindow}
                onChange={(event) => {
                  const next = Number(event.target.value);
                  if (!Number.isFinite(next)) return;
                  setDerivedWindow(Math.max(2, Math.min(512, Math.floor(next))));
                }}
                className="mt-1 block w-24 rounded-md border border-border bg-surface-secondary px-2 py-1.5 text-sm text-text-primary focus:outline-none focus:ring-2 focus:ring-accent"
              />
            </label>
          )}
          <label className="inline-flex items-center gap-2 rounded-md border border-border bg-surface-secondary px-3 py-2 text-sm text-text-secondary">
            <input
              type="checkbox"
              checked={showRawSeries}
              disabled={derivedMode === 'none'}
              onChange={(event) => setShowRawSeries(event.target.checked)}
              className="rounded border-border bg-surface"
            />
            Show raw lines
          </label>
          {notice && <span className="text-xs text-text-muted">{notice}</span>}
        </div>
      </div>

      {/* Legend */}
      <div className="flex flex-wrap gap-4 mb-4">
        {comparisonData.map((run) => (
          <div key={run.run_id} className="flex items-center gap-2">
            <div
              className="w-3 h-3 rounded-full"
              style={{ backgroundColor: run.color }}
            />
            <span className="text-sm font-medium text-text-primary">
              {run.label}
            </span>
            <span className="text-xs text-text-muted">({run.status})</span>
          </div>
        ))}
      </div>

      {commonMetrics.length === 0 ? (
        <div className="text-center py-8 text-text-muted">
          No common metrics found between selected runs
        </div>
      ) : sortedSteps.length === 0 ? (
        <div className="text-center py-8 text-text-muted">
          No data points available for {selectedMetricLabel}
        </div>
      ) : (
        <div>
          {/* Inline chart with expand button */}
          <div className="rounded-lg border border-border overflow-hidden relative" data-compare-chart>
            <UPlotChart
              title={`${selectedMetricLabel} across runs${derivedMode === 'none' ? '' : ` • ${computedLabel}`}`}
              xData={sortedSteps}
              series={chartSeries}
              xLabel="Step"
              yLabel={yAxisLabel}
              height={320}
              interactive={true}
              darkTheme={isDark}
              showLegend={true}
            />
            {/* Expand button overlay */}
            <button
              onClick={() => setIsExpanded(true)}
              className="absolute top-2 right-2 p-1.5 rounded-md bg-surface/80 backdrop-blur-sm border border-border/50 hover:bg-surface-hover text-text-muted hover:text-text-primary transition-colors z-10"
              title="Expand chart"
            >
              <ExpandIcon />
            </button>
          </div>

          {/* Expanded overlay */}
          {isExpanded && (
            <div className="fixed inset-0 z-50 flex flex-col bg-background">
              {/* Expanded header */}
              <div className="flex items-center justify-between px-6 py-3 border-b border-border shrink-0">
                <div className="flex items-center gap-4">
                  <h2 className="text-lg font-semibold text-text-primary">
                    {selectedMetricLabel} across runs{derivedMode === 'none' ? '' : ` • ${computedLabel}`}
                  </h2>
                  {commonMetrics.length > 1 && (
                    <select
                      value={selectedMetric}
                      onChange={(e) => setSelectedMetric(e.target.value)}
                      className="px-3 py-1.5 border border-border rounded-lg text-sm text-text-primary bg-surface-secondary focus:outline-none focus:ring-2 focus:ring-accent"
                    >
                      {commonMetrics.map((name) => (
                        <option key={name} value={name}>
                          {metricDisplayNameByRaw.get(name) || name}
                        </option>
                      ))}
                    </select>
                  )}
                </div>
                <div className="flex items-center gap-2">
                  <button
                    onClick={handleDownload}
                    className="p-2 rounded-lg hover:bg-surface-hover text-text-muted hover:text-text-primary transition-colors"
                    title="Download PNG"
                  >
                    <DownloadIcon />
                  </button>
                  <button
                    onClick={() => setIsExpanded(false)}
                    className="p-2 rounded-lg hover:bg-surface-hover text-text-muted hover:text-text-primary transition-colors"
                    title="Close (Esc)"
                  >
                    <CollapseIcon />
                  </button>
                </div>
              </div>

              {/* Expanded legend */}
              <div className="flex flex-wrap gap-4 px-6 py-2 border-b border-border shrink-0">
                {comparisonData.map((run) => (
                  <div key={run.run_id} className="flex items-center gap-2">
                    <div
                      className="w-3 h-0.5 rounded-full"
                      style={{ backgroundColor: run.color }}
                    />
                    <span className="text-sm font-medium text-text-primary">
                      {run.label}
                    </span>
                    <span className="text-xs text-text-muted">({run.status})</span>
                  </div>
                ))}
              </div>

              {/* Expanded chart — takes remaining space */}
              <div ref={expandedChartRef} className="flex-1 min-h-0 p-4">
                <ExpandedChart
                  title={`${selectedMetricLabel} across runs${derivedMode === 'none' ? '' : ` • ${computedLabel}`}`}
                  yLabel={yAxisLabel}
                  sortedSteps={sortedSteps}
                  chartSeries={chartSeries}
                  isDark={isDark}
                />
              </div>
            </div>
          )}

          {/* Summary table */}
          <div className="mt-4 overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border">
                  <th className="text-left py-2 px-3 text-text-secondary font-semibold">Run</th>
                  <th className="text-right py-2 px-3 text-text-secondary font-semibold">Min</th>
                  <th className="text-right py-2 px-3 text-text-secondary font-semibold">Max</th>
                  <th className="text-right py-2 px-3 text-text-secondary font-semibold">Last</th>
                  <th className="text-right py-2 px-3 text-text-secondary font-semibold">Points</th>
                </tr>
              </thead>
              <tbody>
                {comparisonData.map((run) => {
                  const series = run.metricSeries;
                  if (!series) return null;
                  const points = series.points;
                  const bounds = safeMinMax(points);
                  return (
                    <tr key={run.run_id} className="border-b border-border">
                      <td className="py-2 px-3 flex items-center gap-2 font-medium text-text-primary">
                        <div
                          className="w-2 h-2 rounded-full"
                          style={{ backgroundColor: run.color }}
                        />
                        {run.label}
                      </td>
                      <td className="text-right py-2 px-3 font-mono text-text-secondary">
                        {formatFixed(bounds.min)}
                      </td>
                      <td className="text-right py-2 px-3 font-mono text-text-secondary">
                        {formatFixed(bounds.max)}
                      </td>
                      <td className="text-right py-2 px-3 font-mono text-text-secondary">
                        {points.length > 0 ? points[points.length - 1].mean.toFixed(4) : '-'}
                      </td>
                      <td className="text-right py-2 px-3 text-text-secondary">{series.total_points}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </div>
      )}
    </div>
  );
}

/**
 * A separate component for the expanded chart so it measures its own
 * container height independently and passes it to UPlotChart.
 */
function ExpandedChart({
  title,
  yLabel,
  sortedSteps,
  chartSeries,
  isDark,
}: {
  title: string;
  yLabel: string;
  sortedSteps: number[];
  chartSeries: { label: string; color: string; data: number[]; upper?: number[]; lower?: number[] }[];
  isDark: boolean;
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [chartHeight, setChartHeight] = useState(500);

  useEffect(() => {
    if (!containerRef.current) return;
    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const h = entry.contentRect.height;
        if (h > 100) {
          // Leave room for the legend inside UPlotChart (approx 44px)
          setChartHeight(Math.floor(h - 44));
        }
      }
    });
    observer.observe(containerRef.current);
    return () => observer.disconnect();
  }, []);

  return (
    <div ref={containerRef} className="w-full h-full">
      <UPlotChart
        title={title}
        xData={sortedSteps}
        series={chartSeries}
        xLabel="Step"
        yLabel={yLabel}
        height={chartHeight}
        interactive={true}
        darkTheme={isDark}
        showLegend={true}
      />
    </div>
  );
}
