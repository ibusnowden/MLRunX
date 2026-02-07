'use client';

import { useState } from 'react';

interface DashboardLayoutProps {
  /** Run name for the header */
  runName: string;
  /** Run status badge */
  status?: 'running' | 'finished' | 'failed' | 'killed' | 'pending';
  /** Total number of metrics */
  totalMetrics?: number;
  /** Filter callback when search changes */
  onFilterChange?: (filter: string) => void;
  /** Children (metric sections) */
  children: React.ReactNode;
  /** Use dark theme */
  darkTheme?: boolean;
}

// Search icon
const SearchIcon = () => (
  <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
  </svg>
);

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
  onFilterChange,
  children,
}: DashboardLayoutProps) {
  const [filter, setFilter] = useState('');

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
            {/* Left: Run name and status */}
            <div className="flex items-center gap-3 min-w-0">
              <h1 className="text-xl font-semibold truncate text-text-primary">
                {runName}
              </h1>
              <StatusBadge status={status} />
              {totalMetrics > 0 && (
                <span className="text-sm text-text-muted">
                  {totalMetrics} metrics
                </span>
              )}
            </div>

            {/* Right: Search bar */}
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
