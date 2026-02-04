'use client';

import { Suspense } from 'react';
import { useRouter } from 'next/navigation';
import { RunsTable } from '@/components/RunsTable';
import { Run } from '@/lib/api';

export default function Home() {
  const router = useRouter();

  const handleRunClick = (run: Run) => {
    router.push(`/runs/${run.run_id}`);
  };

  return (
    <main className="min-h-screen p-8">
      <div className="max-w-7xl mx-auto">
        {/* Header */}
        <div className="mb-8">
          <h1 className="text-3xl font-bold text-gray-900">MLRunX</h1>
          <p className="text-gray-600 mt-1">ML Experiment Tracking Dashboard</p>
        </div>

        {/* Runs Table */}
        <div className="bg-white rounded-xl shadow-sm p-6">
          <h2 className="text-xl font-semibold mb-4">Runs</h2>
          <Suspense fallback={<div className="text-gray-500">Loading runs…</div>}>
            <RunsTable onRunClick={handleRunClick} />
          </Suspense>
        </div>
      </div>
    </main>
  );
}
