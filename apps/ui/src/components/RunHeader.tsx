'use client';

import { RunDetail } from '@/lib/api';

interface RunHeaderProps {
  run: RunDetail;
  darkTheme?: boolean;
}

function formatDuration(seconds: number | null): string {
  if (seconds === null) return '-';
  if (seconds < 60) return `${seconds.toFixed(1)}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${Math.floor(seconds % 60)}s`;
  return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`;
}

export function RunHeader({ run }: RunHeaderProps) {
  return (
    <div className="bg-surface rounded-xl border border-border p-6 mb-6">
      {/* Title Row */}
      <div className="flex items-start justify-between mb-4">
        <div>
          <h1 className="text-2xl font-bold text-text-primary">
            {run.name || 'Unnamed Run'}
          </h1>
          <p className="text-sm font-mono mt-1 text-text-muted">{run.run_id}</p>
        </div>
        <span className={`inline-flex items-center gap-1.5 px-3 py-1 rounded-full text-sm font-medium bg-[var(--badge-${run.status}-bg)] text-[var(--badge-${run.status}-text)]`}>
          {run.status}
        </span>
      </div>

      {/* Stats Grid */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
        <div className="bg-surface-secondary rounded-lg p-3">
          <div className="text-sm text-text-muted">Project</div>
          <div className="font-medium text-text-primary">{run.project_id}</div>
        </div>
        <div className="bg-surface-secondary rounded-lg p-3">
          <div className="text-sm text-text-muted">Duration</div>
          <div className="font-medium text-text-primary">
            {formatDuration(run.duration_seconds)}
          </div>
        </div>
        <div className="bg-surface-secondary rounded-lg p-3">
          <div className="text-sm text-text-muted">Metrics</div>
          <div className="font-medium text-text-primary">{run.metrics_count}</div>
        </div>
        <div className="bg-surface-secondary rounded-lg p-3">
          <div className="text-sm text-text-muted">Parameters</div>
          <div className="font-medium text-text-primary">{run.params_count}</div>
        </div>
      </div>

      {/* Tags */}
      {Object.keys(run.tags).length > 0 && (
        <div className="mt-4">
          <div className="text-sm text-text-muted mb-2">Tags</div>
          <div className="flex flex-wrap gap-2">
            {Object.entries(run.tags).map(([key, value]) => (
              <span
                key={key}
                className="px-2 py-1 rounded text-sm bg-surface-secondary text-text-secondary"
              >
                <span className="font-medium">{key}:</span> {value}
              </span>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
