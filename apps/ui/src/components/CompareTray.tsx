'use client';

import { useMemo } from 'react';
import { usePathname, useRouter } from 'next/navigation';
import {
  clearCompareSelection,
  getCompareRunIds,
  getCompareUrl,
  swapCompareSelection,
  useCompareSelectionState,
} from '@/lib/compareSelection';

const CompareIcon = () => (
  <svg className="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M7.5 21L3 16.5m0 0L7.5 12M3 16.5h13.5m0-13.5L21 7.5m0 0L16.5 12M21 7.5H7.5" />
  </svg>
);

function shortenRunId(runId: string): string {
  return runId.length <= 12 ? runId : `${runId.slice(0, 12)}...`;
}

export function CompareTray() {
  const router = useRouter();
  const pathname = usePathname();
  const selection = useCompareSelectionState();
  const runIds = useMemo(() => getCompareRunIds(selection), [selection]);

  if (runIds.length === 0) return null;

  const isComparePage = pathname === '/compare';

  return (
    <div className="fixed bottom-4 right-4 z-40 w-[min(92vw,420px)]">
      <div className="rounded-xl border border-border bg-surface/95 p-3 shadow-xl backdrop-blur">
        <div className="mb-2 flex items-center gap-2 text-sm font-semibold text-text-primary">
          <CompareIcon />
          Compare Tray
        </div>
        <div className="grid grid-cols-1 gap-1 text-xs text-text-secondary">
          <div className="rounded-md border border-border bg-surface-secondary px-2 py-1">
            <span className="font-semibold text-text-primary">Baseline:</span>{' '}
            {selection.baselineRunId ? shortenRunId(selection.baselineRunId) : '-'}
          </div>
          <div className="rounded-md border border-border bg-surface-secondary px-2 py-1">
            <span className="font-semibold text-text-primary">Candidate:</span>{' '}
            {selection.candidateRunId ? shortenRunId(selection.candidateRunId) : '-'}
          </div>
        </div>
        <div className="mt-2 flex flex-wrap gap-2">
          <button
            type="button"
            onClick={() => swapCompareSelection()}
            disabled={runIds.length < 2}
            className="rounded-md border border-border px-2.5 py-1.5 text-xs font-medium text-text-secondary hover:bg-surface-secondary hover:text-text-primary disabled:cursor-not-allowed disabled:opacity-50"
          >
            Swap
          </button>
          <button
            type="button"
            onClick={() => clearCompareSelection()}
            className="rounded-md border border-danger/30 bg-danger/10 px-2.5 py-1.5 text-xs font-medium text-danger hover:bg-danger/15"
          >
            Clear
          </button>
          <button
            type="button"
            onClick={() => router.push(getCompareUrl(selection))}
            disabled={isComparePage}
            className="rounded-md bg-accent px-2.5 py-1.5 text-xs font-semibold text-white hover:bg-accent-hover disabled:cursor-not-allowed disabled:opacity-60"
          >
            {isComparePage ? 'Open Compare' : `Open Compare (${runIds.length})`}
          </button>
        </div>
      </div>
    </div>
  );
}
