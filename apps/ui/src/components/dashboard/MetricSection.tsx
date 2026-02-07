'use client';

import { useState } from 'react';

interface MetricSectionProps {
  /** Section title */
  title: string;
  /** Number of metrics/charts in this section */
  metricCount: number;
  /** Whether the section is expanded by default */
  defaultExpanded?: boolean;
  /** Children elements (charts) */
  children: React.ReactNode;
  /** Use dark theme */
  darkTheme?: boolean;
}

const ChevronIcon = ({ expanded }: { expanded: boolean }) => (
  <svg
    className={`w-5 h-5 transition-transform duration-200 ${expanded ? 'rotate-90' : ''}`}
    fill="none"
    stroke="currentColor"
    viewBox="0 0 24 24"
  >
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 5l7 7-7 7" />
  </svg>
);

export function MetricSection({
  title,
  metricCount,
  defaultExpanded = true,
  children,
}: MetricSectionProps) {
  const [expanded, setExpanded] = useState(defaultExpanded);

  return (
    <div className="bg-surface rounded-xl border border-border overflow-hidden mb-4">
      {/* Section Header - Clickable */}
      <button
        onClick={() => setExpanded(!expanded)}
        className="w-full flex items-center justify-between px-5 py-4 hover:bg-surface-hover transition-colors"
      >
        <div className="flex items-center gap-3">
          <span className="text-text-secondary">
            <ChevronIcon expanded={expanded} />
          </span>
          <h2 className="text-lg font-semibold text-text-primary">{title}</h2>
          <span className="bg-accent text-white text-xs font-medium px-2 py-0.5 rounded-full">
            {metricCount}
          </span>
        </div>
        <span className="text-sm text-text-muted">
          {expanded ? 'Collapse' : 'Expand'}
        </span>
      </button>

      {/* Section Content - Grid of Charts */}
      {expanded && (
        <div className="px-5 pb-5 border-t border-border">
          <div className="grid grid-cols-1 lg:grid-cols-2 gap-4 pt-4">
            {children}
          </div>
        </div>
      )}
    </div>
  );
}
