'use client';

import { useState, useEffect, useCallback, Suspense } from 'react';
import { useRouter, useSearchParams } from 'next/navigation';
import { api, Run } from '@/lib/api';
import { ComparePanel } from '@/components/ComparePanel';

function ComparePageContent() {
  const router = useRouter();
  const searchParams = useSearchParams();

  // Get run IDs from URL
  const runIdsParam = searchParams.get('runs') || '';
  const selectedRunIds = runIdsParam ? runIdsParam.split(',') : [];

  // Available runs for selection
  const [runs, setRuns] = useState<Run[]>([]);
  const [loading, setLoading] = useState(true);

  // Fetch available runs
  const fetchRuns = useCallback(async () => {
    setLoading(true);
    try {
      const response = await api.listRuns({ limit: 100 });
      setRuns(response.runs);
    } catch (err) {
      console.error('Failed to fetch runs:', err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchRuns();
  }, [fetchRuns]);

  // Toggle run selection
  const toggleRun = (runId: string) => {
    const newSelection = selectedRunIds.includes(runId)
      ? selectedRunIds.filter((id) => id !== runId)
      : [...selectedRunIds, runId];

    // Update URL
    const newParams = new URLSearchParams(searchParams);
    if (newSelection.length > 0) {
      newParams.set('runs', newSelection.join(','));
    } else {
      newParams.delete('runs');
    }
    router.push(`/compare?${newParams.toString()}`);
  };

  // Clear selection
  const clearSelection = () => {
    router.push('/compare');
  };

  return (
    <main className="min-h-screen p-8">
      <div className="max-w-7xl mx-auto">
        {/* Header */}
        <div className="flex items-center justify-between mb-6">
          <div>
            <h1 className="text-3xl font-bold text-gray-900">Compare Runs</h1>
            <p className="text-gray-600 mt-1">
              Select runs to compare their metrics
            </p>
          </div>
          <div className="flex gap-2">
            <button
              onClick={() => router.push('/')}
              className="px-4 py-2 text-gray-600 hover:text-gray-900"
            >
              Back to Runs
            </button>
            {selectedRunIds.length > 0 && (
              <button
                onClick={clearSelection}
                className="px-4 py-2 bg-gray-100 rounded-lg text-gray-800 font-medium hover:bg-gray-200"
              >
                Clear Selection ({selectedRunIds.length})
              </button>
            )}
          </div>
        </div>

        <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
          {/* Run Selector */}
          <div className="lg:col-span-1">
            <div className="bg-white rounded-xl shadow-sm p-4 sticky top-4 text-gray-900">
              <h2 className="font-semibold text-gray-900 mb-3">Select Runs</h2>
              {loading ? (
                <div className="text-gray-700 text-sm">Loading runs...</div>
              ) : (
                <div className="space-y-2 max-h-96 overflow-y-auto">
                  {runs.map((run) => {
                    const isSelected = selectedRunIds.includes(run.run_id);
                    return (
                      <label
                        key={run.run_id}
                        className={`flex items-center gap-3 p-2 rounded cursor-pointer text-gray-900 hover:bg-gray-50 ${
                          isSelected ? 'bg-blue-50 ring-1 ring-blue-200' : ''
                        }`}
                      >
                        <input
                          type="checkbox"
                          checked={isSelected}
                          onChange={() => toggleRun(run.run_id)}
                          className="rounded"
                        />
                        <div className="flex-1 min-w-0">
                          <div className="font-semibold text-sm text-gray-900 truncate">
                            {run.name || run.run_id.slice(0, 8)}
                          </div>
                          <div className="text-xs text-gray-700">
                            {run.status} - {run.metrics_count} metrics
                          </div>
                        </div>
                      </label>
                    );
                  })}
                </div>
              )}
            </div>
          </div>

          {/* Compare Panel */}
          <div className="lg:col-span-2">
            <ComparePanel runIds={selectedRunIds} />
          </div>
        </div>
      </div>
    </main>
  );
}

export default function ComparePage() {
  return (
    <Suspense fallback={<div className="min-h-screen p-8 flex items-center justify-center">Loading...</div>}>
      <ComparePageContent />
    </Suspense>
  );
}
