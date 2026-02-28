'use client';

import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { api, MetricSeries } from '@/lib/api';
import { UPlotChart } from '@/components/charts/UPlotChart';
import { createSeriesColorScale } from '@/components/charts/chartColors';
import { useTheme } from '@/components/ThemeProvider';
import { formatFixed, safeMinMax } from '@/lib/format';
import { useAutoRefresh } from '@/lib/useAutoRefresh';

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
}

interface ComparePanelProps {
  runIds: string[];
}

const COMPARE_MAX_POINTS = 5000;

export function ComparePanel({ runIds }: ComparePanelProps) {
  const { isDark } = useTheme();
  const [runs, setRuns] = useState<CompareData[]>([]);
  const [commonMetrics, setCommonMetrics] = useState<string[]>([]);
  const [selectedMetric, setSelectedMetric] = useState<string>('');
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
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
    if (runIds.length === 0) {
      setRuns([]);
      setCommonMetrics([]);
      setSelectedMetric('');
      setLoading(false);
      return;
    }

    if (!silent) {
      setLoading(true);
    }
    setError(null);

    try {
      const response = await api.compareRuns(runIds, [], COMPARE_MAX_POINTS);
      setRuns(response.runs);
      setCommonMetrics(response.common_metrics);
      setSelectedMetric((prev) => {
        if (response.common_metrics.length === 0) return '';
        if (prev && response.common_metrics.includes(prev)) return prev;
        return response.common_metrics[0];
      });
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

  const seriesLabels = useMemo(
    () => runs.map((run) => run.run_name || run.run_id.slice(0, 8)),
    [runs]
  );
  const runColorScale = useMemo(
    () => createSeriesColorScale(seriesLabels, isDark),
    [seriesLabels, isDark]
  );

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
  const chartSeries = comparisonData.map((run) => {
    const points = run.metricSeries?.points ?? [];
    const data = new Array(sortedSteps.length).fill(null);
    const upper = new Array(sortedSteps.length).fill(null);
    const lower = new Array(sortedSteps.length).fill(null);
    points.forEach((point) => {
      const idx = stepToIndex.get(point.step);
      if (idx !== undefined) {
        data[idx] = point.mean;
        upper[idx] = point.max;
        lower[idx] = point.min;
      }
    });
    return {
      label: run.label,
      color: run.color,
      data,
      upper,
      lower,
    };
  });

  return (
    <div className="bg-surface rounded-xl border border-border p-4 sm:p-6">
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between mb-4">
        <h2 className="text-lg sm:text-xl font-semibold text-text-primary">Compare Runs</h2>
        {commonMetrics.length > 0 && (
          <select
            value={selectedMetric}
            onChange={(e) => setSelectedMetric(e.target.value)}
            className="w-full sm:w-auto px-3 py-2 border border-border rounded-lg text-text-primary bg-surface-secondary focus:outline-none focus:ring-2 focus:ring-accent"
          >
            {commonMetrics.map((name) => (
              <option key={name} value={name}>
                {name}
              </option>
            ))}
          </select>
        )}
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
          No data points available for {selectedMetric}
        </div>
      ) : (
        <div>
          {/* Inline chart with expand button */}
          <div className="rounded-lg border border-border overflow-hidden relative" data-compare-chart>
            <UPlotChart
              title={`${selectedMetric} across runs`}
              xData={sortedSteps}
              series={chartSeries}
              xLabel="Step"
              yLabel={selectedMetric}
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
                    {selectedMetric} across runs
                  </h2>
                  {commonMetrics.length > 1 && (
                    <select
                      value={selectedMetric}
                      onChange={(e) => setSelectedMetric(e.target.value)}
                      className="px-3 py-1.5 border border-border rounded-lg text-sm text-text-primary bg-surface-secondary focus:outline-none focus:ring-2 focus:ring-accent"
                    >
                      {commonMetrics.map((name) => (
                        <option key={name} value={name}>
                          {name}
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
                  selectedMetric={selectedMetric}
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
  selectedMetric,
  sortedSteps,
  chartSeries,
  isDark,
}: {
  selectedMetric: string;
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
        title={`${selectedMetric} across runs`}
        xData={sortedSteps}
        series={chartSeries}
        xLabel="Step"
        yLabel={selectedMetric}
        height={chartHeight}
        interactive={true}
        darkTheme={isDark}
        showLegend={true}
      />
    </div>
  );
}
