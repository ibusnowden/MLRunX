'use client';

import { useState, useEffect, useCallback, useMemo, Suspense } from 'react';
import { useRouter, useSearchParams } from 'next/navigation';
import { api, Run } from '@/lib/api';
import { ComparePanel } from '@/components/ComparePanel';
import { formatDuration } from '@/lib/format';
import { useAutoRefresh } from '@/lib/useAutoRefresh';
import {
  deleteComparePreset,
  loadComparePresets,
  normalizeRunIdSet,
  saveComparePresets,
  type ComparePreset,
  upsertComparePreset,
} from '@/lib/presets';

function ComparePageContent() {
  const router = useRouter();
  const searchParams = useSearchParams();

  // Get run IDs from URL
  const runIdsParam = searchParams.get('runs') || '';
  const selectedRunIds = useMemo(
    () => (runIdsParam ? runIdsParam.split(',') : []),
    [runIdsParam]
  );

  // Available runs for selection
  const [runs, setRuns] = useState<Run[]>([]);
  const [loading, setLoading] = useState(true);
  const [savedComparePresets, setSavedComparePresets] = useState<ComparePreset[]>([]);
  const [activeComparePresetId, setActiveComparePresetId] = useState('');

  // Fetch available runs
  const fetchRuns = useCallback(async ({ silent = false }: { silent?: boolean } = {}) => {
    if (!silent) {
      setLoading(true);
    }
    try {
      const response = await api.listRuns({ limit: 100 });
      setRuns(response.runs);
    } catch (err) {
      console.error('Failed to fetch runs:', err);
    } finally {
      if (!silent) {
        setLoading(false);
      }
    }
  }, []);

  useEffect(() => {
    void fetchRuns();
  }, [fetchRuns]);

  useEffect(() => {
    setSavedComparePresets(loadComparePresets());
  }, []);

  useEffect(() => {
    const normalizedSelection = normalizeRunIdSet(selectedRunIds);
    if (normalizedSelection.length === 0) {
      setActiveComparePresetId('');
      return;
    }

    const matchedPreset = savedComparePresets.find((preset) => {
      const normalizedPresetRuns = normalizeRunIdSet(preset.runIds);
      return (
        normalizedPresetRuns.length === normalizedSelection.length &&
        normalizedPresetRuns.every((runId, index) => runId === normalizedSelection[index])
      );
    });
    setActiveComparePresetId(matchedPreset?.id || '');
  }, [savedComparePresets, selectedRunIds]);

  useAutoRefresh(
    () => fetchRuns({ silent: true }),
    { intervalMs: 30000, enabled: true, runOnMount: false }
  );

  const applyRunSelection = useCallback(
    (runIds: string[]) => {
      const normalizedSelection = normalizeRunIdSet(runIds);
      const newParams = new URLSearchParams(searchParams);
      if (normalizedSelection.length > 0) {
        newParams.set('runs', normalizedSelection.join(','));
      } else {
        newParams.delete('runs');
      }
      const query = newParams.toString();
      router.push(query ? `/compare?${query}` : '/compare');
    },
    [router, searchParams]
  );

  // Toggle run selection
  const toggleRun = (runId: string) => {
    const newSelection = selectedRunIds.includes(runId)
      ? selectedRunIds.filter((id) => id !== runId)
      : [...selectedRunIds, runId];
    applyRunSelection(newSelection);
  };

  const handleSavePreset = () => {
    const normalizedSelection = normalizeRunIdSet(selectedRunIds);
    if (normalizedSelection.length === 0) return;

    const defaultName = `Selection ${normalizedSelection.length}`;
    const requestedName = window.prompt('Save compare preset as:', defaultName);
    if (requestedName === null) return;
    const name = requestedName.trim();
    if (!name) return;

    try {
      const result = upsertComparePreset(savedComparePresets, name, normalizedSelection);
      setSavedComparePresets(result.presets);
      setActiveComparePresetId(result.saved.id);
      saveComparePresets(result.presets);
    } catch (err) {
      console.error('Failed to save compare preset', err);
    }
  };

  const handleApplyPreset = (presetId: string) => {
    if (!presetId) {
      setActiveComparePresetId('');
      return;
    }
    const preset = savedComparePresets.find((entry) => entry.id === presetId);
    if (!preset) return;
    setActiveComparePresetId(preset.id);
    applyRunSelection(preset.runIds);
  };

  const handleDeletePreset = () => {
    if (!activeComparePresetId) return;
    const nextPresets = deleteComparePreset(savedComparePresets, activeComparePresetId);
    setSavedComparePresets(nextPresets);
    setActiveComparePresetId('');
    saveComparePresets(nextPresets);
  };

  // Clear selection
  const clearSelection = () => {
    applyRunSelection([]);
  };

  const selectedPreset = savedComparePresets.find((preset) => preset.id === activeComparePresetId);

  return (
    <main className="min-h-screen">
      {/* Page header */}
      <div className="border-b border-border bg-surface">
        <div className="max-w-[1600px] mx-auto px-4 sm:px-6 lg:px-8 py-4 sm:py-6">
          <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
            <div>
              <h1 className="text-xl sm:text-2xl font-bold text-text-primary">Compare Runs</h1>
              <p className="text-xs sm:text-sm text-text-secondary mt-0.5">
                Select runs to compare their metrics side by side
              </p>
            </div>
            <div className="flex flex-wrap gap-2">
              <button
                onClick={() => router.push('/')}
                className="px-3 sm:px-4 py-2 text-sm text-text-secondary hover:text-text-primary transition-colors"
              >
                Back to Runs
              </button>
              <select
                value={activeComparePresetId}
                onChange={(event) => handleApplyPreset(event.target.value)}
                className="px-3 sm:px-4 py-2 text-sm bg-surface-secondary rounded-lg text-text-primary font-medium hover:bg-surface-hover border border-border transition-colors focus:outline-none focus:ring-2 focus:ring-accent/40"
              >
                <option value="">Compare Presets</option>
                {savedComparePresets.map((preset) => (
                  <option key={preset.id} value={preset.id}>
                    {preset.name} ({preset.runIds.length})
                  </option>
                ))}
              </select>
              <button
                onClick={handleSavePreset}
                disabled={selectedRunIds.length === 0}
                className="px-3 sm:px-4 py-2 text-sm bg-surface-secondary rounded-lg text-text-primary font-medium hover:bg-surface-hover border border-border transition-colors disabled:cursor-not-allowed disabled:opacity-50"
              >
                Save Preset
              </button>
              {selectedPreset && (
                <button
                  onClick={handleDeletePreset}
                  className="px-3 sm:px-4 py-2 text-sm rounded-lg text-danger font-medium bg-danger/10 hover:bg-danger/15 border border-danger/30 transition-colors"
                >
                  Delete Preset
                </button>
              )}
              {selectedRunIds.length > 0 && (
                <button
                  onClick={clearSelection}
                  className="px-3 sm:px-4 py-2 text-sm bg-surface-secondary rounded-lg text-text-primary font-medium hover:bg-surface-hover border border-border transition-colors"
                >
                  Clear ({selectedRunIds.length})
                </button>
              )}
            </div>
          </div>
        </div>
      </div>

      {/* Content */}
      <div className="max-w-[1600px] mx-auto px-4 sm:px-6 lg:px-8 py-4 sm:py-6">
        <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
          {/* Run Selector */}
          <div className="lg:col-span-1">
            <div className="bg-surface rounded-xl border border-border p-4 sticky top-16 md:top-4">
              <h2 className="font-semibold text-text-primary mb-3">Select Runs</h2>
              {loading ? (
                <div className="text-text-muted text-sm">Loading runs...</div>
              ) : (
                <div className="space-y-1.5 max-h-96 overflow-y-auto">
                  {runs.map((run) => {
                    const isSelected = selectedRunIds.includes(run.run_id);
                    return (
                      <label
                        key={run.run_id}
                        className={`flex items-center gap-3 p-2.5 rounded-lg cursor-pointer transition-colors ${
                          isSelected
                            ? 'bg-accent-subtle ring-1 ring-accent/30'
                            : 'hover:bg-surface-hover'
                        }`}
                      >
                        <input
                          type="checkbox"
                          checked={isSelected}
                          onChange={() => toggleRun(run.run_id)}
                          className="rounded"
                        />
                        <div className="flex-1 min-w-0">
                          <div className="font-medium text-sm text-text-primary truncate">
                            {run.name || run.run_id.slice(0, 8)}
                          </div>
                          <div className="text-xs text-text-muted">
                            {run.status} - {run.metrics_count} metrics
                            {run.duration_seconds != null && (
                              <span> - {formatDuration(run.duration_seconds)}</span>
                            )}
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
    <Suspense fallback={
      <div className="min-h-screen flex items-center justify-center text-text-muted">
        Loading...
      </div>
    }>
      <ComparePageContent />
    </Suspense>
  );
}
