'use client';

import React, { useState, useEffect, useCallback, useRef } from 'react';
import { api, MetricSeries } from '@/lib/api';
import { UPlotChart } from '@/components/charts/UPlotChart';
import { ChartControls } from '@/components/charts/ChartControls';
import { useTheme } from '@/components/ThemeProvider';

interface MetricChartPanelProps {
  runId: string;
  darkTheme?: boolean;
}

export function MetricChartPanel({ runId }: MetricChartPanelProps) {
  const { isDark } = useTheme();
  const [series, setSeries] = useState<MetricSeries[]>([]);
  const [availableMetrics, setAvailableMetrics] = useState<string[]>([]);
  const [selectedMetrics, setSelectedMetrics] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [maxPoints, setMaxPoints] = useState(500);
  const [smoothing, setSmoothing] = useState(0);
  const chartRef = useRef<{ resetZoom?: () => void }>(null);

  const fetchMetrics = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const response = await api.getMetrics(runId, {
        names: selectedMetrics.length > 0 ? selectedMetrics : undefined,
        maxPoints,
      });
      setSeries(response.series);
      setAvailableMetrics(response.available_metrics);

      if (response.available_metrics.length > 0 && selectedMetrics.length === 0) {
        setSelectedMetrics([response.available_metrics[0]]);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to fetch metrics');
    } finally {
      setLoading(false);
    }
  }, [runId, selectedMetrics, maxPoints]);

  useEffect(() => {
    fetchMetrics();
  }, [fetchMetrics]);

  const toggleMetric = (name: string) => {
    setSelectedMetrics((prev) =>
      prev.includes(name) ? prev.filter((n) => n !== name) : [...prev, name]
    );
  };

  const handleViewportChange = useCallback((_min: number, _max: number) => {
    // Could implement viewport-driven fetching here
  }, []);

  // Prepare chart data
  const chartData = {
    xData: [] as number[],
    series: [] as { label: string; data: number[]; color?: string }[],
  };

  if (series.length > 0) {
    const allSteps = new Set<number>();
    series.forEach((s) => s.points.forEach((p) => allSteps.add(p.step)));
    chartData.xData = Array.from(allSteps).sort((a, b) => a - b);

    const stepToIndex = new Map(chartData.xData.map((s, i) => [s, i]));

    series.forEach((s) => {
      const data = new Array(chartData.xData.length).fill(null);
      s.points.forEach((p) => {
        const idx = stepToIndex.get(p.step);
        if (idx !== undefined) {
          data[idx] = p.mean;
        }
      });
      chartData.series.push({ label: s.name, data });
    });
  }

  return (
    <div className="bg-surface rounded-xl shadow-sm border border-border">
      {/* Header */}
      <div className="flex items-center justify-between px-6 py-4 border-b border-border">
        <h2 className="text-xl font-semibold text-text-primary">Metrics</h2>
        <div className="flex items-center gap-3">
          <select
            value={maxPoints}
            onChange={(e) => setMaxPoints(parseInt(e.target.value, 10))}
            className="px-3 py-1.5 border border-border rounded-lg text-sm bg-surface-secondary text-text-primary outline-none focus:ring-2 focus:ring-accent"
          >
            <option value={100}>100 points</option>
            <option value={500}>500 points</option>
            <option value={1000}>1000 points</option>
            <option value={5000}>5000 points</option>
          </select>
        </div>
      </div>

      {/* Metric selector */}
      {availableMetrics.length > 0 && (
        <div className="px-6 py-3 border-b border-border flex flex-wrap gap-2">
          {availableMetrics.map((name) => {
            const isSelected = selectedMetrics.includes(name);
            return (
              <button
                key={name}
                onClick={() => toggleMetric(name)}
                className={`px-3 py-1 rounded-full text-sm transition-colors ${
                  isSelected
                    ? 'bg-accent text-white'
                    : 'bg-surface-secondary text-text-secondary hover:bg-surface-hover'
                }`}
              >
                {name}
              </button>
            );
          })}
        </div>
      )}

      {/* Content area */}
      <div className="p-4">
        {error && (
          <div className="p-4 rounded-lg bg-danger-subtle border border-danger/20 text-danger">
            {error}
          </div>
        )}

        {loading ? (
          <div className="h-64 flex items-center justify-center text-text-muted">
            <div className="flex items-center gap-2">
              <svg className="animate-spin h-5 w-5" viewBox="0 0 24 24">
                <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" fill="none"/>
                <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"/>
              </svg>
              Loading metrics...
            </div>
          </div>
        ) : availableMetrics.length === 0 ? (
          <div className="h-64 flex items-center justify-center text-text-muted">
            No metrics recorded for this run
          </div>
        ) : chartData.xData.length > 0 ? (
          <div>
            <ChartControls
              title={selectedMetrics.length === 1 ? selectedMetrics[0] : 'Metrics'}
              smoothing={smoothing}
              onSmoothingChange={setSmoothing}
              onResetZoom={() => chartRef.current?.resetZoom?.()}
              darkTheme={isDark}
            >
              <UPlotChart
                xData={chartData.xData}
                series={chartData.series}
                xLabel="Step"
                yLabel="Value"
                height={300}
                interactive={true}
                darkTheme={isDark}
                smoothing={smoothing}
                onViewportChange={handleViewportChange}
              />
            </ChartControls>

            {/* Info bar */}
            <div className="mt-3 px-2 text-sm text-text-muted flex flex-wrap gap-x-4 gap-y-1">
              {series.map((s) => (
                <span key={s.name}>
                  <span className="font-medium">{s.name}:</span> {s.total_points.toLocaleString()} points
                  {s.downsampled && <span className="text-warning ml-1">(downsampled)</span>}
                </span>
              ))}
            </div>

            {/* Summary stats */}
            <div className="mt-4 grid grid-cols-3 gap-3 text-sm">
              {series.slice(0, 1).map((s) => (
                <React.Fragment key={s.name}>
                  <div className="rounded-lg p-3 bg-surface-secondary">
                    <div className="text-text-muted">Min ({s.name})</div>
                    <div className="font-mono text-lg text-text-primary">
                      {Math.min(...s.points.map((p) => p.min)).toFixed(4)}
                    </div>
                  </div>
                  <div className="rounded-lg p-3 bg-surface-secondary">
                    <div className="text-text-muted">Max ({s.name})</div>
                    <div className="font-mono text-lg text-text-primary">
                      {Math.max(...s.points.map((p) => p.max)).toFixed(4)}
                    </div>
                  </div>
                  <div className="rounded-lg p-3 bg-surface-secondary">
                    <div className="text-text-muted">Last ({s.name})</div>
                    <div className="font-mono text-lg text-text-primary">
                      {s.points.length > 0
                        ? s.points[s.points.length - 1].mean.toFixed(4)
                        : '-'}
                    </div>
                  </div>
                </React.Fragment>
              ))}
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}
