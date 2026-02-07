'use client';

import { Suspense } from 'react';
import { useRouter } from 'next/navigation';
import { RunsTable } from '@/components/RunsTable';
import { Run } from '@/lib/api';

// ── Icons ──

const ActivityIcon = () => (
  <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.75} d="M13 10V3L4 14h7v7l9-11h-7z" />
  </svg>
);

const RunsIcon = () => (
  <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.75} d="M4 6h16M4 10h16M4 14h16M4 18h16" />
  </svg>
);

function LoadingSkeleton() {
  return (
    <div className="animate-pulse space-y-4">
      <div className="h-10 bg-surface rounded-lg w-full" />
      <div className="space-y-3">
        {[...Array(5)].map((_, i) => (
          <div key={i} className="h-14 bg-surface rounded-lg w-full" />
        ))}
      </div>
    </div>
  );
}

export default function Home() {
  const router = useRouter();

  const handleRunClick = (run: Run) => {
    router.push(`/runs/${run.run_id}`);
  };

  return (
    <main className="min-h-screen">
      {/* Page header */}
      <div className="border-b border-border bg-surface">
        <div className="max-w-[1600px] mx-auto px-8 py-6">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-4">
              <div className="p-2.5 rounded-xl bg-accent-subtle">
                <ActivityIcon />
              </div>
              <div>
                <h1 className="text-2xl font-bold text-text-primary">
                  Experiments
                </h1>
                <p className="text-sm text-text-secondary mt-0.5">
                  Track, compare and analyze your ML training runs
                </p>
              </div>
            </div>
          </div>
        </div>
      </div>

      {/* Main content */}
      <div className="max-w-[1600px] mx-auto px-8 py-6">
        {/* Runs card */}
        <div className="bg-surface rounded-xl border border-border overflow-hidden">
          {/* Card header */}
          <div className="flex items-center gap-3 px-6 py-4 border-b border-border">
            <RunsIcon />
            <h2 className="text-lg font-semibold text-text-primary">Training Runs</h2>
          </div>

          {/* Card body */}
          <div className="p-6">
            <Suspense fallback={<LoadingSkeleton />}>
              <RunsTable onRunClick={handleRunClick} />
            </Suspense>
          </div>
        </div>
      </div>
    </main>
  );
}
