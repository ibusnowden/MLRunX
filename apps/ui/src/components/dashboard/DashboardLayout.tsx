'use client';

import { useState } from 'react';

interface DashboardLayoutProps {
  /** Run name for the header */
  runName: string;
  /** Run status badge */
  status?: 'running' | 'finished' | 'failed' | 'killed' | 'pending';
  /** Total number of metrics */
  totalMetrics?: number;
  /** Duration in seconds */
  durationSeconds?: number | null;
  /** Filter callback when search changes */
  onFilterChange?: (filter: string) => void;
  /** Callback when delete is triggered */
  onDelete?: () => void;
  /** Children (metric sections) */
  children: React.ReactNode;
  /** Use dark theme */
  darkTheme?: boolean;
}

// Icons
const SearchIcon = () => (
  <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
  </svg>
);

const ClockIcon = () => (
  <svg className="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" />
  </svg>
);

const TrashIcon = () => (
  <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
  </svg>
);

function formatDuration(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined) return '-';
  if (seconds < 60) return `${seconds.toFixed(1)}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${Math.floor(seconds % 60)}s`;
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  return `${h}h ${m}m`;
}

// Status badge component
function StatusBadge({ status }: { status: string }) {
  return (
    <span className={`inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-medium bg-[var(--badge-${status}-bg)] text-[var(--badge-${status}-text)]`}>
      <span className={`w-2 h-2 rounded-full ${status === 'running' ? 'bg-accent animate-pulse' : status === 'finished' ? 'bg-success' : status === 'failed' ? 'bg-danger' : status === 'killed' ? 'bg-warning' : 'bg-text-muted'}`} />
      {status.charAt(0).toUpperCase() + status.slice(1)}
    </span>
  );
}

export function DashboardLayout({
  runName,
  status = 'finished',
  totalMetrics = 0,
  durationSeconds,
  onFilterChange,
  onDelete,
  children,
}: DashboardLayoutProps) {
  const [filter, setFilter] = useState('');
  const [showDeleteConfirm, setShowDeleteConfirm] = useState(false);

  const handleFilterChange = (value: string) => {
    setFilter(value);
    onFilterChange?.(value);
  };

  return (
    <div className="min-h-screen bg-background">
      {/* Sticky Header */}
      <div className="sticky top-0 z-40 bg-surface border-b border-border">
        <div className="max-w-[1600px] mx-auto px-6 py-4">
          <div className="flex items-center justify-between gap-4">
            {/* Left: Run name, status, duration */}
            <div className="flex items-center gap-3 min-w-0">
              <h1 className="text-xl font-semibold truncate text-text-primary">
                {runName}
              </h1>
              <StatusBadge status={status} />
              {durationSeconds != null && (
                <span className="flex items-center gap-1 text-sm text-text-muted">
                  <ClockIcon />
                  {formatDuration(durationSeconds)}
                </span>
              )}
              {totalMetrics > 0 && (
                <span className="text-sm text-text-muted">
                  {totalMetrics} metrics
                </span>
              )}
            </div>

            {/* Right: Search bar + Delete */}
            <div className="flex items-center gap-3">
              <div className="flex-shrink-0 w-72">
                <div className="relative">
                  <div className="absolute inset-y-0 left-0 flex items-center pl-3 pointer-events-none text-text-muted">
                    <SearchIcon />
                  </div>
                  <input
                    type="text"
                    value={filter}
                    onChange={(e) => handleFilterChange(e.target.value)}
                    placeholder="Filter metrics..."
                    className="w-full pl-10 pr-4 py-2 text-sm rounded-lg border border-border bg-surface-secondary text-text-primary placeholder:text-text-muted focus:outline-none focus:ring-2 focus:ring-accent focus:border-transparent"
                  />
                </div>
              </div>

              {onDelete && (
                <div className="relative">
                  {showDeleteConfirm ? (
                    <div className="flex items-center gap-2">
                      <span className="text-xs text-text-muted whitespace-nowrap">Delete?</span>
                      <button
                        onClick={() => {
                          onDelete();
                          setShowDeleteConfirm(false);
                        }}
                        className="px-2.5 py-1.5 text-xs font-medium rounded-lg bg-danger text-white hover:bg-danger/80 transition-colors"
                      >
                        Yes
                      </button>
                      <button
                        onClick={() => setShowDeleteConfirm(false)}
                        className="px-2.5 py-1.5 text-xs font-medium rounded-lg bg-surface-secondary text-text-secondary hover:bg-surface-hover border border-border transition-colors"
                      >
                        Cancel
                      </button>
                    </div>
                  ) : (
                    <button
                      onClick={() => setShowDeleteConfirm(true)}
                      className="p-2 rounded-lg hover:bg-danger/10 text-text-muted hover:text-danger transition-colors"
                      title="Delete run"
                    >
                      <TrashIcon />
                    </button>
                  )}
                </div>
              )}
            </div>
          </div>
        </div>
      </div>

      {/* Main Content Area */}
      <div className="max-w-[1600px] mx-auto px-6 py-6">
        {children}
      </div>
    </div>
  );
}
