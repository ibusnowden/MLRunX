'use client';

import { useState, useEffect, useCallback, useMemo, use } from 'react';
import { useRouter } from 'next/navigation';
import { api, RunDetail, MetricSeries } from '@/lib/api';
import { DashboardLayout, MetricSection, ChartCard } from '@/components/dashboard';
import { UPlotChart } from '@/components/charts/UPlotChart';
import { useTheme } from '@/components/ThemeProvider';
import { useAutoRefresh } from '@/lib/useAutoRefresh';
import {
  groupMetrics,
  filterMetrics,
  shouldUseLogScale,
  getMetricDisplayTitle,
  getMetricSeriesLabel,
  GroupedMetrics,
} from '@/lib/metricGroups';

// Back arrow icon
const BackIcon = () => (
  <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15 19l-7-7 7-7" />
  </svg>
);

// Loading spinner
const LoadingSpinner = () => (
  <div className="flex items-center gap-2">
    <svg className="animate-spin h-5 w-5" viewBox="0 0 24 24">
      <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" fill="none" />
      <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z" />
    </svg>
    Loading...
  </div>
);

interface MetricChartData {
  xData: number[];
  series: { label: string; data: number[]; color?: string; upper?: number[]; lower?: number[] }[];
}

const RUN_METRICS_MAX_POINTS = 1000;

export default function RunDetailPage({ params }: { params: Promise<{ run_id: string }> }) {
  // Unwrap the async params using React.use()
  const { run_id: runId } = use(params);
  const router = useRouter();
  const { isDark } = useTheme();

  // Run state
  const [run, setRun] = useState<RunDetail | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Metrics state
  const [allMetrics, setAllMetrics] = useState<MetricSeries[]>([]);
  const [availableMetrics, setAvailableMetrics] = useState<string[]>([]);
  const [metricsLoading, setMetricsLoading] = useState(true);
  const [metricsError, setMetricsError] = useState<string | null>(null);

  // UI state
  const [filter, setFilter] = useState('');
  const [smoothing, setSmoothing] = useState(0);

  const darkTheme = isDark;

  const fetchRun = useCallback(async ({ silent = false }: { silent?: boolean } = {}) => {
    if (!silent) {
      setLoading(true);
    }
    setError(null);
    try {
      const data = await api.getRun(runId);
      setRun(data);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to fetch run');
    } finally {
      if (!silent) {
        setLoading(false);
      }
    }
  }, [runId]);

  const fetchAllMetrics = useCallback(async ({ silent = false }: { silent?: boolean } = {}) => {
    if (!silent) {
      setMetricsLoading(true);
    }
    setMetricsError(null);
    try {
      // First get the list of available metrics
      const response = await api.getMetrics(runId, { maxPoints: RUN_METRICS_MAX_POINTS });
      setAvailableMetrics(response.available_metrics);

      // Then fetch all metrics data
      if (response.available_metrics.length > 0) {
        const allData = await api.getMetrics(runId, {
          names: response.available_metrics,
          maxPoints: RUN_METRICS_MAX_POINTS,
        });
        setAllMetrics(allData.series);
      } else {
        setAllMetrics([]);
      }
    } catch (err) {
      setMetricsError(err instanceof Error ? err.message : 'Failed to fetch metrics');
    } finally {
      if (!silent) {
        setMetricsLoading(false);
      }
    }
  }, [runId]);

  useEffect(() => {
    void fetchRun();
  }, [fetchRun]);

  useEffect(() => {
    void fetchAllMetrics();
  }, [fetchAllMetrics]);

  const shouldAutoRefresh = run?.status === 'running'
    || (!!run && run.metrics_count > 0 && availableMetrics.length === 0);

  useAutoRefresh(
    async () => {
      await Promise.all([
        fetchRun({ silent: true }),
        fetchAllMetrics({ silent: true }),
      ]);
    },
    { enabled: shouldAutoRefresh, intervalMs: 15000, runOnMount: false }
  );

  // Filter and group metrics
  const filteredMetrics = useMemo(() => {
    return filterMetrics(availableMetrics, filter);
  }, [availableMetrics, filter]);

  const groupedMetrics = useMemo(() => {
    return groupMetrics(filteredMetrics);
  }, [filteredMetrics]);

  // Prepare chart data for a metric
  const prepareChartData = useCallback(
    (metricName: string): MetricChartData => {
      const series = allMetrics.find((s) => s.name === metricName);
      if (!series) {
        return { xData: [], series: [] };
      }

      const xData = series.points.map((p) => p.step);
      const yData = series.points.map((p) => p.mean);
      const upper = series.points.map((p) => p.max);
      const lower = series.points.map((p) => p.min);

      return {
        xData,
        series: [{ label: metricName, data: yData, upper, lower }],
      };
    },
    [allMetrics]
  );

  // Prepare chart data for multiple metrics (overlay)
  const prepareMultiMetricChartData = useCallback(
    (metricNames: string[]): MetricChartData => {
      if (metricNames.length === 0) {
        return { xData: [], series: [] };
      }

      // Collect all unique steps
      const allSteps = new Set<number>();
      const seriesData: MetricSeries[] = [];

      for (const name of metricNames) {
        const series = allMetrics.find((s) => s.name === name);
        if (series) {
          seriesData.push(series);
          series.points.forEach((p) => allSteps.add(p.step));
        }
      }

      const xData = Array.from(allSteps).sort((a, b) => a - b);
      const stepToIndex = new Map(xData.map((s, i) => [s, i]));

      const chartSeries = seriesData.map((s) => {
        const data = new Array(xData.length).fill(null);
        const upper = new Array(xData.length).fill(null);
        const lower = new Array(xData.length).fill(null);
        s.points.forEach((p) => {
          const idx = stepToIndex.get(p.step);
          if (idx !== undefined) {
            data[idx] = p.mean;
            upper[idx] = p.max;
            lower[idx] = p.min;
          }
        });
        // Use the suffix for label (e.g., "fc1" from "grad_norm/fc1")
        const label = getMetricSeriesLabel(s.name);
        return { label, data, upper, lower };
      });

      return { xData, series: chartSeries };
    },
    [allMetrics]
  );

  // Group metrics by prefix for overlay charts with smart grouping
  const getMetricsByPrefix = useCallback(
    (metrics: string[]): Map<string, string[]> => {
      const prefixGroups = new Map<string, string[]>();

      for (const metric of metrics) {
        const parts = metric.split('/');
        let groupKey: string;

        if (parts.length > 1) {
          const prefix = parts[0];
          const suffix = parts.slice(1).join('/');

          // Special handling for grad_norm: separate global from per-layer
          if (prefix === 'grad_norm') {
            if (suffix.includes('global')) {
              groupKey = 'grad_norm_global';
            } else {
              groupKey = 'grad_norm_layers';
            }
          }
          // All activations grouped together
          else if (prefix === 'activation') {
            groupKey = 'activation';
          }
          // Loss metrics: group by layer type
          else if (prefix === 'loss') {
            groupKey = 'loss';
          }
          // GPU metrics: sub-group by metric type across GPUs
          // Only keep memory_total_gb and memory_used_percent charts
          else if (prefix === 'gpu') {
            const SHOWN_GPU_METRICS = ['memory_total_gb', 'memory_used_percent'];
            if (parts.length >= 3) {
              const metricType = parts.slice(2).join('/');
              if (SHOWN_GPU_METRICS.includes(metricType)) {
                groupKey = `gpu_${metricType}`;
              } else {
                continue; // skip unwanted GPU metrics
              }
            } else {
              // gpu/{something} with no GPU ID (e.g. gpu/device_count)
              continue; // skip device_count etc.
            }
          }
          // Other system metrics: group by type (cpu, memory, disk, network)
          else if (['cpu', 'memory', 'disk', 'network'].includes(prefix)) {
            groupKey = prefix;
          }
          // Default: group by first-level prefix
          else {
            groupKey = prefix;
          }
        } else {
          // No prefix, treat as individual
          groupKey = metric;
        }

        if (!prefixGroups.has(groupKey)) {
          prefixGroups.set(groupKey, []);
        }
        prefixGroups.get(groupKey)!.push(metric);
      }

      return prefixGroups;
    },
    []
  );

  // Handle filter change from dashboard layout
  const handleFilterChange = useCallback((newFilter: string) => {
    setFilter(newFilter);
  }, []);

  // Handle delete run
  const handleDeleteRun = useCallback(async () => {
    try {
      await api.deleteRun(runId);
      router.push('/');
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to delete run');
    }
  }, [runId, router]);

  // Loading state
  if (loading) {
    return (
      <main className="min-h-screen p-4 sm:p-6 bg-background">
        <div className="max-w-7xl mx-auto">
          <div className="text-center py-16 text-text-muted">
            <LoadingSpinner />
          </div>
        </div>
      </main>
    );
  }

  // Error state
  if (error || !run) {
    return (
      <main className="min-h-screen p-4 sm:p-6 bg-background">
        <div className="max-w-7xl mx-auto">
          <div className="bg-danger-subtle border border-danger/20 rounded-lg p-4 text-danger">
            {error || 'Run not found'}
          </div>
          <button
            onClick={() => router.push('/')}
            className="mt-4 px-4 py-2 bg-surface text-text-secondary rounded-lg hover:bg-surface-hover flex items-center gap-2 border border-border"
          >
            <BackIcon />
            Back to Runs
          </button>
        </div>
      </main>
    );
  }

  return (
    <DashboardLayout
      runName={run.name || run.run_id}
      status={run.status}
      totalMetrics={availableMetrics.length}
      durationSeconds={run.duration_seconds}
      onFilterChange={handleFilterChange}
      onDelete={handleDeleteRun}
      darkTheme={darkTheme}
    >
      {/* Back button */}
      <button
        onClick={() => router.push('/')}
        className="mb-4 text-text-secondary hover:text-text-primary flex items-center gap-1 transition-colors"
      >
        <BackIcon />
        Back to Runs
      </button>

      {/* Metrics Loading State */}
      {metricsLoading && (
        <div className="text-center py-16 text-text-muted">
          <LoadingSpinner />
        </div>
      )}

      {/* Metrics Error State */}
      {metricsError && (
        <div className="bg-danger-subtle border border-danger/20 rounded-lg p-4 text-danger mb-4">
          {metricsError}
        </div>
      )}

      {/* No Metrics State */}
      {!metricsLoading && !metricsError && availableMetrics.length === 0 && (
        <div className="text-center py-16 text-text-muted">
          {run?.metrics_count && run.metrics_count > 0
            ? `Metrics are still syncing (${run.metrics_count} ingested so far).`
            : 'No metrics recorded for this run'}
        </div>
      )}

      {/* No Results After Filter */}
      {!metricsLoading && !metricsError && availableMetrics.length > 0 && filteredMetrics.length === 0 && (
        <div className="text-center py-16 text-text-muted">
          No metrics match filter &quot;{filter}&quot;
        </div>
      )}

      {/* Metric Sections */}
      {!metricsLoading && groupedMetrics.map((group: GroupedMetrics) => (
        <MetricSectionRenderer
          key={group.groupKey}
          group={group}
          darkTheme={darkTheme}
          smoothing={smoothing}
          onSmoothingChange={setSmoothing}
          prepareChartData={prepareChartData}
          prepareMultiMetricChartData={prepareMultiMetricChartData}
          getMetricsByPrefix={getMetricsByPrefix}
        />
      ))}

      {/* Parameters Section */}
      {run.params_count > 0 && (
        <div className="bg-surface rounded-xl border border-border p-6 mt-4">
          <h2 className="text-xl font-semibold text-text-primary mb-4">Parameters</h2>
          <div className="text-sm text-text-secondary">
            {run.params_count} parameters logged
            <span className="ml-2 text-xs text-text-muted">(detail view coming soon)</span>
          </div>
        </div>
      )}
    </DashboardLayout>
  );
}

// Separate component for rendering a metric section
interface MetricSectionRendererProps {
  group: GroupedMetrics;
  darkTheme: boolean;
  smoothing: number;
  onSmoothingChange: (value: number) => void;
  prepareChartData: (metricName: string) => MetricChartData;
  prepareMultiMetricChartData: (metricNames: string[]) => MetricChartData;
  getMetricsByPrefix: (metrics: string[]) => Map<string, string[]>;
}

function MetricSectionRenderer({
  group,
  darkTheme,
  smoothing,
  prepareChartData,
  prepareMultiMetricChartData,
  getMetricsByPrefix,
}: MetricSectionRendererProps) {
  // Group metrics by prefix for potential overlay
  const metricsByPrefix = useMemo(
    () => getMetricsByPrefix(group.metrics),
    [group.metrics, getMetricsByPrefix]
  );

  // Determine how to render: individual charts or grouped overlay charts
  const renderCharts = useMemo(() => {
    const charts: Array<{
      key: string;
      title: string;
      metrics: string[];
      isGrouped: boolean;
    }> = [];

    // Groups that should always be overlaid regardless of count
    // Include gpu_* sub-groups so each GPU metric type is overlaid across GPUs
    const alwaysGrouped = ['activation', 'grad_norm_layers', 'grad_norm_global', 'cpu', 'memory', 'disk', 'network'];

    metricsByPrefix.forEach((metrics, prefix) => {
      // Always group these prefixes together (including gpu_* sub-groups)
      const isAlwaysGrouped = alwaysGrouped.includes(prefix) || prefix.startsWith('gpu_');
      if (isAlwaysGrouped && metrics.length > 1) {
        charts.push({
          key: prefix,
          title: getMetricDisplayTitle(prefix),
          metrics,
          isGrouped: true,
        });
      } else if (metrics.length > 1 && metrics.length <= 15) {
        // Group as overlay chart (increased limit to 15)
        charts.push({
          key: prefix,
          title: getMetricDisplayTitle(prefix),
          metrics,
          isGrouped: true,
        });
      } else if (metrics.length === 1) {
        // Single metric chart
        charts.push({
          key: metrics[0],
          title: getMetricDisplayTitle(metrics[0]),
          metrics,
          isGrouped: false,
        });
      } else {
        // Too many metrics for one chart, render individually
        metrics.forEach((m) => {
          charts.push({
            key: m,
            title: getMetricDisplayTitle(m),
            metrics: [m],
            isGrouped: false,
          });
        });
      }
    });

    return charts;
  }, [metricsByPrefix]);

  return (
    <MetricSection
      title={group.title}
      metricCount={group.metrics.length}
      defaultExpanded={true}
      darkTheme={darkTheme}
    >
      {renderCharts.map((chart) => {
        const chartData = chart.isGrouped
          ? prepareMultiMetricChartData(chart.metrics)
          : prepareChartData(chart.metrics[0]);

        // Determine if we should use log scale
        const useLogScale =
          group.chartDefaults?.logScale ?? shouldUseLogScale(chart.metrics[0]);

        return (
          <ChartCard
            key={chart.key}
            title={chart.title}
            subtitle={chart.isGrouped ? `${chart.metrics.length} series` : undefined}
            darkTheme={darkTheme}
          >
            <UPlotChart
              xData={chartData.xData}
              series={chartData.series}
              xLabel="Step"
              height={340}
              interactive={true}
              darkTheme={darkTheme}
              showLegend={chart.isGrouped}
              smoothing={smoothing}
              logScale={useLogScale}
              areaFill={false}
            />
          </ChartCard>
        );
      })}
    </MetricSection>
  );
}
