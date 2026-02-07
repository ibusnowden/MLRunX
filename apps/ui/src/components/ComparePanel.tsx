'use client';

import { useState, useEffect } from 'react';
import { api, MetricSeries } from '@/lib/api';
import { UPlotChart } from '@/components/charts/UPlotChart';
import { useTheme } from '@/components/ThemeProvider';

interface CompareData {
  run_id: string;
  run_name: string | null;
  status: string;
  series: MetricSeries[];
}

interface ComparePanelProps {
  runIds: string[];
}

const RUN_COLORS = [
  '#2563eb', '#dc2626', '#16a34a', '#9333ea',
  '#ea580c', '#db2777', '#0d9488', '#ca8a04',
];
const COMPARE_MAX_POINTS = 5000;

export function ComparePanel({ runIds }: ComparePanelProps) {
  const { isDark } = useTheme();
  const [runs, setRuns] = useState<CompareData[]>([]);
  const [commonMetrics, setCommonMetrics] = useState<string[]>([]);
  const [selectedMetric, setSelectedMetric] = useState<string>('');
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    async function fetchComparison() {
      if (runIds.length === 0) {
        setRuns([]);
        setCommonMetrics([]);
        setSelectedMetric('');
        setLoading(false);
        return;
      }

      setLoading(true);
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
        setLoading(false);
      }
    }
    fetchComparison();
  }, [runIds]);

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

  // Get series for selected metric from each run
  const comparisonData = runs.map((run, idx) => ({
    ...run,
    color: RUN_COLORS[idx % RUN_COLORS.length],
    metricSeries: run.series.find((s) => s.name === selectedMetric),
  }));

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
    points.forEach((point) => {
      const idx = stepToIndex.get(point.step);
      if (idx !== undefined) {
        data[idx] = point.mean;
      }
    });
    return {
      label: run.run_name || run.run_id.slice(0, 8),
      color: run.color,
      data,
    };
  });

  return (
    <div className="bg-surface rounded-xl border border-border p-6">
      <div className="flex items-center justify-between mb-4">
        <h2 className="text-xl font-semibold text-text-primary">Compare Runs</h2>
        {commonMetrics.length > 0 && (
          <select
            value={selectedMetric}
            onChange={(e) => setSelectedMetric(e.target.value)}
            className="px-3 py-2 border border-border rounded-lg text-text-primary bg-surface-secondary focus:outline-none focus:ring-2 focus:ring-accent"
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
              {run.run_name || run.run_id.slice(0, 8)}
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
          <div className="rounded-lg border border-border overflow-hidden">
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
          </div>

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
                  return (
                    <tr key={run.run_id} className="border-b border-border">
                      <td className="py-2 px-3 flex items-center gap-2 font-medium text-text-primary">
                        <div
                          className="w-2 h-2 rounded-full"
                          style={{ backgroundColor: run.color }}
                        />
                        {run.run_name || run.run_id.slice(0, 8)}
                      </td>
                      <td className="text-right py-2 px-3 font-mono text-text-secondary">
                        {Math.min(...points.map((p) => p.min)).toFixed(4)}
                      </td>
                      <td className="text-right py-2 px-3 font-mono text-text-secondary">
                        {Math.max(...points.map((p) => p.max)).toFixed(4)}
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
