'use client';

import { useState, useEffect, useCallback, useMemo, Suspense } from 'react';
import { useRouter, useSearchParams } from 'next/navigation';
import { api, Run } from '@/lib/api';
import { ComparePanel, type CompareRunMetadata } from '@/components/ComparePanel';
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
import {
  normalizeMetricObjectiveOverrides,
  parseMetricObjectiveOverridesParam,
  serializeMetricObjectiveOverridesParam,
  type MetricObjective,
} from '@/lib/compareObjectives';

const RUN_SELECTOR_FETCH_LIMIT = 1000;
const MAX_COMPARE_SELECTION = 5000;

function objectivesEqual(
  left: Record<string, MetricObjective>,
  right: Record<string, MetricObjective>
): boolean {
  const leftEntries = Object.entries(normalizeMetricObjectiveOverrides(left));
  const rightEntries = Object.entries(normalizeMetricObjectiveOverrides(right));
  if (leftEntries.length !== rightEntries.length) return false;
  return leftEntries.every(([metricName, objective], index) => {
    const [otherMetricName, otherObjective] = rightEntries[index] ?? [];
    return metricName === otherMetricName && objective === otherObjective;
  });
}

function formatComparePresetOptionLabel(preset: ComparePreset): string {
  const runCount = preset.runIds.length;
  const objectiveOverrideCount = Object.keys(preset.metricObjectives ?? {}).length;
  const runLabel = `${runCount} run${runCount === 1 ? '' : 's'}`;
  const objectiveLabel = objectiveOverrideCount === 0
    ? 'default objectives'
    : `${objectiveOverrideCount} objective override${objectiveOverrideCount === 1 ? '' : 's'}`;
  return `${preset.name} (${runLabel}, ${objectiveLabel})`;
}

function ComparePageContent() {
  const router = useRouter();
  const searchParams = useSearchParams();

  // Get run IDs from URL
  const runIdsParam = searchParams.get('runs') || '';
  const metricObjectivesParam = searchParams.get('metricObjectives');
  const selectedRunIds = useMemo(
    () => normalizeRunIdSet(runIdsParam ? runIdsParam.split(',') : []),
    [runIdsParam]
  );
  const metricObjectiveOverrides = useMemo(
    () => parseMetricObjectiveOverridesParam(metricObjectivesParam),
    [metricObjectivesParam]
  );

  // Available runs for selection
  const [runs, setRuns] = useState<Run[]>([]);
  const [runSearch, setRunSearch] = useState('');
  const [loading, setLoading] = useState(true);
  const [savedComparePresets, setSavedComparePresets] = useState<ComparePreset[]>([]);
  const [activeComparePresetId, setActiveComparePresetId] = useState('');
  const [selectorStatus, setSelectorStatus] = useState<string | null>(null);
  const [invalidSelectedRunIds, setInvalidSelectedRunIds] = useState<string[]>([]);

  // Fetch available runs
  const fetchRuns = useCallback(async ({ silent = false }: { silent?: boolean } = {}) => {
    if (!silent) {
      setLoading(true);
    }
    try {
      const response = await api.listRuns({ limit: RUN_SELECTOR_FETCH_LIMIT });
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

  const runIndex = useMemo(
    () => new Map(runs.map((run) => [run.run_id, run])),
    [runs]
  );
  const compareRunMetadataById = useMemo<Record<string, CompareRunMetadata>>(() => {
    const metadata: Record<string, CompareRunMetadata> = {};
    selectedRunIds.forEach((runId) => {
      const run = runIndex.get(runId);
      if (!run) return;
      metadata[runId] = {
        run_id: run.run_id,
        name: run.name,
        status: run.status,
        tags: run.tags,
      };
    });
    return metadata;
  }, [runIndex, selectedRunIds]);

  useEffect(() => {
    const normalizedSelection = normalizeRunIdSet(selectedRunIds);
    const normalizedObjectives = normalizeMetricObjectiveOverrides(metricObjectiveOverrides);
    if (normalizedSelection.length === 0) {
      setActiveComparePresetId('');
      return;
    }

    const matchedPreset = savedComparePresets.find((preset) => {
      const normalizedPresetRuns = normalizeRunIdSet(preset.runIds);
      const normalizedPresetObjectives = normalizeMetricObjectiveOverrides(preset.metricObjectives ?? {});
      return (
        normalizedPresetRuns.length === normalizedSelection.length &&
        normalizedPresetRuns.every((runId, index) => runId === normalizedSelection[index]) &&
        objectivesEqual(normalizedPresetObjectives, normalizedObjectives)
      );
    });
    setActiveComparePresetId(matchedPreset?.id || '');
  }, [savedComparePresets, selectedRunIds, metricObjectiveOverrides]);

  useAutoRefresh(
    () => fetchRuns({ silent: true }),
    { intervalMs: 30000, enabled: true, runOnMount: false }
  );

  useEffect(() => {
    let cancelled = false;

    const unresolvedRunIds = selectedRunIds.filter((runId) => !runIndex.has(runId));
    if (unresolvedRunIds.length === 0) {
      setInvalidSelectedRunIds([]);
      return () => {};
    }

    const resolveSelection = async () => {
      const resolvedRuns: Run[] = [];
      const invalidIds: string[] = [];

      await Promise.all(
        unresolvedRunIds.map(async (runId) => {
          try {
            const run = await api.getRun(runId);
            resolvedRuns.push(run);
          } catch {
            invalidIds.push(runId);
          }
        })
      );

      if (cancelled) return;
      if (resolvedRuns.length > 0) {
        setRuns((previous) => {
          const merged = new Map(previous.map((run) => [run.run_id, run]));
          for (const run of resolvedRuns) {
            merged.set(run.run_id, run);
          }
          return [...merged.values()];
        });
      }
      setInvalidSelectedRunIds(invalidIds);
      if (invalidIds.length > 0) {
        setSelectorStatus(
          `${invalidIds.length} selected run(s) were not found and can be removed from this sweep comparison.`
        );
      }
    };

    void resolveSelection();
    return () => {
      cancelled = true;
    };
  }, [runIndex, selectedRunIds]);

  const applyCompareState = useCallback(
    (
      runIds: string[],
      objectives: Record<string, MetricObjective>,
      { clearStatus = false }: { clearStatus?: boolean } = {}
    ) => {
      const normalizedSelection = normalizeRunIdSet(runIds);
      const normalizedObjectives = normalizeMetricObjectiveOverrides(objectives);
      const serializedObjectives = serializeMetricObjectiveOverridesParam(normalizedObjectives);

      const newParams = new URLSearchParams(searchParams);
      if (normalizedSelection.length > 0) {
        newParams.set('runs', normalizedSelection.join(','));
      } else {
        newParams.delete('runs');
      }
      if (serializedObjectives) {
        newParams.set('metricObjectives', serializedObjectives);
      } else {
        newParams.delete('metricObjectives');
      }

      const currentQuery = searchParams.toString();
      const nextQuery = newParams.toString();
      if (nextQuery === currentQuery) return;

      if (clearStatus) {
        setSelectorStatus(null);
      }
      router.push(nextQuery ? `/compare?${nextQuery}` : '/compare');
    },
    [router, searchParams]
  );

  const applyRunSelection = useCallback(
    (runIds: string[]) => {
      applyCompareState(runIds, metricObjectiveOverrides, { clearStatus: true });
    },
    [applyCompareState, metricObjectiveOverrides]
  );

  const handleMetricObjectiveOverridesChange = useCallback(
    (objectives: Record<string, MetricObjective>) => {
      applyCompareState(selectedRunIds, objectives);
    },
    [applyCompareState, selectedRunIds]
  );

  // Toggle run selection
  const toggleRun = (runId: string) => {
    if (!selectedRunIds.includes(runId) && selectedRunIds.length >= MAX_COMPARE_SELECTION) {
      setSelectorStatus(`Maximum ${MAX_COMPARE_SELECTION} runs can be selected at once.`);
      return;
    }
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
      const result = upsertComparePreset(
        savedComparePresets,
        name,
        normalizedSelection,
        metricObjectiveOverrides
      );
      setSavedComparePresets(result.presets);
      setActiveComparePresetId(result.saved.id);
      saveComparePresets(result.presets);
      setSelectorStatus(`Saved compare preset '${result.saved.name}'.`);
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
    applyCompareState(preset.runIds, preset.metricObjectives ?? {}, { clearStatus: true });
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

  const removeInvalidSelection = () => {
    if (invalidSelectedRunIds.length === 0) return;
    applyRunSelection(selectedRunIds.filter((runId) => !invalidSelectedRunIds.includes(runId)));
    setInvalidSelectedRunIds([]);
  };

  const filteredRuns = useMemo(() => {
    const query = runSearch.trim().toLowerCase();
    if (!query) return runs;
    return runs.filter((run) => {
      const runName = (run.name || '').toLowerCase();
      return runName.includes(query) || run.run_id.toLowerCase().includes(query);
    });
  }, [runSearch, runs]);

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
                    {formatComparePresetOptionLabel(preset)}
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
        {selectorStatus && (
          <div className="mb-4 rounded-lg border border-border bg-surface-secondary px-3 py-2 text-sm text-text-secondary">
            {selectorStatus}
          </div>
        )}
        <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
          {/* Run Selector */}
          <div className="lg:col-span-1">
            <div className="bg-surface rounded-xl border border-border p-4 sticky top-16 md:top-4">
              <h2 className="font-semibold text-text-primary mb-3">Select Runs</h2>
              <div className="mb-3">
                <input
                  type="text"
                  value={runSearch}
                  onChange={(event) => setRunSearch(event.target.value)}
                  placeholder="Search run name or ID..."
                  className="w-full rounded-lg border border-border bg-surface-secondary px-3 py-2 text-sm text-text-primary placeholder:text-text-muted focus:outline-none focus:ring-2 focus:ring-accent"
                />
              </div>
              {invalidSelectedRunIds.length > 0 && (
                <div className="mb-3 rounded-lg border border-warning/30 bg-warning/10 px-3 py-2 text-xs text-warning">
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <span>{invalidSelectedRunIds.length} invalid selected run(s).</span>
                    <button
                      type="button"
                      onClick={removeInvalidSelection}
                      className="rounded border border-warning/40 px-2 py-1 font-medium hover:bg-warning/10"
                    >
                      Remove invalid
                    </button>
                  </div>
                </div>
              )}
              {loading ? (
                <div className="text-text-muted text-sm">Loading runs...</div>
              ) : (
                <div className="space-y-1.5 max-h-96 overflow-y-auto">
                  {filteredRuns.map((run) => {
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
                  {filteredRuns.length === 0 && (
                    <div className="rounded-lg border border-border bg-surface-secondary px-3 py-2 text-sm text-text-muted">
                      No runs match your search.
                    </div>
                  )}
                </div>
              )}
            </div>
          </div>

          {/* Compare Panel */}
          <div className="lg:col-span-2">
            <ComparePanel
              runIds={selectedRunIds}
              onRunIdsChange={applyRunSelection}
              metricObjectiveOverrides={metricObjectiveOverrides}
              onMetricObjectiveOverridesChange={handleMetricObjectiveOverridesChange}
              runMetadataById={compareRunMetadataById}
            />
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
