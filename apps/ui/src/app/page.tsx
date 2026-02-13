'use client';

import Link from 'next/link';
import { Suspense, useCallback, useEffect, useMemo, useState } from 'react';
import { useRouter } from 'next/navigation';
import { RunsTable, RunsTableStats } from '@/components/RunsTable';
import { api, Run } from '@/lib/api';
import { formatDuration } from '@/lib/format';

type OverviewStats = {
  totalRuns: number;
  runningRuns: number;
  finishedRuns: number;
};

const EMPTY_OVERVIEW: OverviewStats = {
  totalRuns: 0,
  runningRuns: 0,
  finishedRuns: 0,
};

const BoltIcon = () => (
  <svg className="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M13 10V3L4 14h7v7l9-11h-7z" />
  </svg>
);

const BookIcon = () => (
  <svg className="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M12 6.042A8.967 8.967 0 006 3.75c-1.052 0-2.062.18-3 .512v14.25A8.987 8.987 0 016 18c2.305 0 4.408.867 6 2.292m0-14.25a8.966 8.966 0 016-2.292c1.052 0 2.062.18 3 .512v14.25A8.987 8.987 0 0018 18a8.967 8.967 0 00-6 2.292m0-14.25v14.25" />
  </svg>
);

const PlusIcon = () => (
  <svg className="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 4.5v15m7.5-7.5h-15" />
  </svg>
);

function LoadingSkeleton() {
  return (
    <div className="animate-pulse">
      <div className="h-12 w-full rounded-xl bg-surface-secondary" />
      <div className="mt-4 space-y-3">
        {[...Array(5)].map((_, index) => (
          <div key={index} className="h-16 w-full rounded-xl bg-surface-secondary" />
        ))}
      </div>
    </div>
  );
}

function StatCard({
  label,
  value,
  sublabel,
  dotClassName,
  className,
}: {
  label: string;
  value: string;
  sublabel?: string;
  dotClassName?: string;
  className?: string;
}) {
  return (
    <div className={`rounded-2xl border border-border bg-surface px-5 py-4 shadow-[0_0_0_1px_rgba(255,255,255,0.01)] transition-all duration-200 hover:-translate-y-0.5 hover:border-[color:rgba(91,148,255,0.35)] sm:px-6 sm:py-5 ${className || ''}`}>
      <div className="flex items-center gap-2 text-sm font-medium text-text-secondary">
        {dotClassName && <span className={`h-2 w-2 rounded-full ${dotClassName}`} />}
        {label}
      </div>
      <div className="mt-3 text-4xl font-semibold tracking-[-0.04em] text-text-primary">{value}</div>
      {sublabel && <div className="mt-1 text-sm text-text-muted">{sublabel}</div>}
    </div>
  );
}

export default function Home() {
  const router = useRouter();
  const [tableStats, setTableStats] = useState<RunsTableStats>({
    totalRuns: 0,
    pageRuns: 0,
    averageDurationSeconds: null,
  });
  const [overviewStats, setOverviewStats] = useState<OverviewStats>(EMPTY_OVERVIEW);
  const [loadingOverview, setLoadingOverview] = useState(true);

  const refreshOverview = useCallback(async () => {
    try {
      const [allRuns, runningRuns, finishedRuns] = await Promise.all([
        api.listRuns({ limit: 1 }),
        api.listRuns({ status: 'running', limit: 1 }),
        api.listRuns({ status: 'finished', limit: 1 }),
      ]);
      setOverviewStats({
        totalRuns: allRuns.total,
        runningRuns: runningRuns.total,
        finishedRuns: finishedRuns.total,
      });
    } catch {
      // Keep latest rendered values if overview refresh fails.
    } finally {
      setLoadingOverview(false);
    }
  }, []);

  useEffect(() => {
    void refreshOverview();
    const intervalId = window.setInterval(() => {
      void refreshOverview();
    }, 20000);
    return () => {
      window.clearInterval(intervalId);
    };
  }, [refreshOverview]);

  const handleRunClick = (run: Run) => {
    router.push(`/runs/${run.run_id}`);
  };

  const overviewTotal = loadingOverview ? tableStats.totalRuns : overviewStats.totalRuns;
  const overviewRunning = loadingOverview ? 0 : overviewStats.runningRuns;
  const overviewFinished = loadingOverview ? 0 : overviewStats.finishedRuns;

  const avgDurationLabel = useMemo(
    () => formatDuration(tableStats.averageDurationSeconds),
    [tableStats.averageDurationSeconds]
  );

  return (
    <main className="min-h-screen bg-[radial-gradient(circle_at_top,_rgba(59,125,255,0.08)_0%,_transparent_38%),linear-gradient(180deg,_rgba(10,13,18,1)_0%,_rgba(10,13,18,1)_100%)]">
      <div className="sticky top-0 z-20 border-b border-border bg-background/95 backdrop-blur-sm">
        <div className="mx-auto flex w-full max-w-[1600px] items-center justify-between gap-4 px-5 py-5 sm:px-8">
          <div className="mlrx-fade-up flex items-center gap-3">
            <div className="inline-flex h-10 w-10 items-center justify-center rounded-xl bg-accent-subtle text-accent">
              <BoltIcon />
            </div>
            <div>
              <h1 className="text-2xl font-semibold tracking-[-0.03em] text-text-primary sm:text-[34px]">MLRunX Experiments</h1>
              <p className="text-sm text-text-muted">Experiment tracking for ML runs anywhere</p>
            </div>
          </div>
          <div className="mlrx-fade-up mlrx-fade-up-delay-1 hidden items-center gap-2 sm:flex">
            <Link
              href="/onboarding"
              className="inline-flex h-11 items-center gap-2 rounded-xl border border-border bg-surface px-4 text-sm font-medium text-text-secondary transition-all duration-200 hover:-translate-y-0.5 hover:bg-surface-hover hover:text-text-primary"
            >
              <BookIcon />
              Docs
            </Link>
            <Link
              href="/onboarding"
              className="inline-flex h-11 items-center gap-2 rounded-xl bg-accent px-4 text-sm font-semibold text-white transition-all duration-200 hover:-translate-y-0.5 hover:bg-accent-hover"
            >
              <PlusIcon />
              New Run
            </Link>
          </div>
        </div>
      </div>

      <div className="mx-auto w-full max-w-[1600px] px-5 py-6 sm:px-8 sm:py-7">
        <div className="mb-6 grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-4">
          <StatCard
            label="Total Runs"
            value={overviewTotal.toLocaleString()}
            sublabel={`${tableStats.pageRuns} on this page`}
            className="mlrx-fade-up"
          />
          <StatCard
            label="Running"
            value={overviewRunning.toLocaleString()}
            dotClassName="bg-[var(--badge-running-text)]"
            className="mlrx-fade-up mlrx-fade-up-delay-1"
          />
          <StatCard
            label="Finished"
            value={overviewFinished.toLocaleString()}
            dotClassName="bg-[var(--badge-finished-text)]"
            className="mlrx-fade-up mlrx-fade-up-delay-2"
          />
          <StatCard label="Avg Duration" value={avgDurationLabel} className="mlrx-fade-up mlrx-fade-up-delay-3" />
        </div>

        <Suspense fallback={<LoadingSkeleton />}>
          <RunsTable onRunClick={handleRunClick} onStatsChange={setTableStats} />
        </Suspense>
      </div>
    </main>
  );
}
