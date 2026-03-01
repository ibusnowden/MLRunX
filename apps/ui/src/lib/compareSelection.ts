import { useEffect, useState } from 'react';

const COMPARE_SELECTION_STORAGE_KEY = 'mlrunx_compare_selection_v1';
const COMPARE_SELECTION_EVENT = 'mlrunx:compare-selection-updated';
const MAX_RUN_ID_LEN = 128;

export interface CompareSelection {
  baselineRunId: string;
  candidateRunId: string;
}

const EMPTY_SELECTION: CompareSelection = {
  baselineRunId: '',
  candidateRunId: '',
};

function clampRunId(value: string): string {
  return value.trim().slice(0, MAX_RUN_ID_LEN);
}

function normalizeSelection(selection: Partial<CompareSelection> | null | undefined): CompareSelection {
  const baselineRunId = clampRunId(selection?.baselineRunId || '');
  let candidateRunId = clampRunId(selection?.candidateRunId || '');
  if (candidateRunId && candidateRunId === baselineRunId) {
    candidateRunId = '';
  }
  return {
    baselineRunId,
    candidateRunId,
  };
}

function emitCompareSelectionUpdated() {
  if (typeof window === 'undefined') return;
  window.dispatchEvent(new Event(COMPARE_SELECTION_EVENT));
}

function readFromStorage(): CompareSelection {
  if (typeof window === 'undefined') return EMPTY_SELECTION;
  try {
    const raw = window.localStorage.getItem(COMPARE_SELECTION_STORAGE_KEY);
    if (!raw) return EMPTY_SELECTION;
    const parsed = JSON.parse(raw) as Partial<CompareSelection>;
    return normalizeSelection(parsed);
  } catch {
    return EMPTY_SELECTION;
  }
}

function writeToStorage(selection: CompareSelection): CompareSelection {
  if (typeof window !== 'undefined') {
    try {
      if (!selection.baselineRunId && !selection.candidateRunId) {
        window.localStorage.removeItem(COMPARE_SELECTION_STORAGE_KEY);
      } else {
        window.localStorage.setItem(COMPARE_SELECTION_STORAGE_KEY, JSON.stringify(selection));
      }
    } catch {
      // Ignore storage failures in restricted browser contexts.
    }
    emitCompareSelectionUpdated();
  }
  return selection;
}

function updateSelection(
  updater: (current: CompareSelection) => CompareSelection
): CompareSelection {
  const current = readFromStorage();
  const next = normalizeSelection(updater(current));
  return writeToStorage(next);
}

export function getCompareSelectionSnapshot(): CompareSelection {
  return readFromStorage();
}

export function setCompareBaseline(runId: string): CompareSelection {
  const normalized = clampRunId(runId);
  return updateSelection((current) => ({
    baselineRunId: normalized,
    candidateRunId:
      current.candidateRunId && current.candidateRunId !== normalized
        ? current.candidateRunId
        : '',
  }));
}

export function setCompareCandidate(runId: string): CompareSelection {
  const normalized = clampRunId(runId);
  return updateSelection((current) => ({
    baselineRunId:
      current.baselineRunId && current.baselineRunId !== normalized
        ? current.baselineRunId
        : '',
    candidateRunId: normalized,
  }));
}

export function clearCompareSelection(): CompareSelection {
  return writeToStorage(EMPTY_SELECTION);
}

export function swapCompareSelection(): CompareSelection {
  return updateSelection((current) => ({
    baselineRunId: current.candidateRunId,
    candidateRunId: current.baselineRunId,
  }));
}

export function getCompareRunIds(selection: CompareSelection): string[] {
  const ordered = [selection.baselineRunId, selection.candidateRunId].filter(Boolean);
  return Array.from(new Set(ordered));
}

export function getCompareUrl(selection: CompareSelection): string {
  const runIds = getCompareRunIds(selection);
  if (runIds.length === 0) return '/compare';
  return `/compare?runs=${encodeURIComponent(runIds.join(','))}`;
}

export function getQuickCompareTarget(
  runId: string,
  selection: CompareSelection
): 'baseline' | 'candidate' | null {
  const normalized = clampRunId(runId);
  if (selection.baselineRunId && selection.baselineRunId !== normalized) {
    return 'baseline';
  }
  if (selection.candidateRunId && selection.candidateRunId !== normalized) {
    return 'candidate';
  }
  return null;
}

export function startCompareWithRun(runId: string): CompareSelection {
  const normalized = clampRunId(runId);
  return updateSelection((current) => {
    if (current.baselineRunId && current.baselineRunId !== normalized) {
      return {
        baselineRunId: current.baselineRunId,
        candidateRunId: normalized,
      };
    }

    if (current.candidateRunId && current.candidateRunId !== normalized) {
      return {
        baselineRunId: normalized,
        candidateRunId: current.candidateRunId,
      };
    }

    return {
      baselineRunId: normalized,
      candidateRunId: '',
    };
  });
}

export function useCompareSelectionState(): CompareSelection {
  const [selection, setSelection] = useState<CompareSelection>(() => getCompareSelectionSnapshot());

  useEffect(() => {
    const sync = () => setSelection(getCompareSelectionSnapshot());
    sync();
    window.addEventListener(COMPARE_SELECTION_EVENT, sync);
    window.addEventListener('storage', sync);
    return () => {
      window.removeEventListener(COMPARE_SELECTION_EVENT, sync);
      window.removeEventListener('storage', sync);
    };
  }, []);

  return selection;
}
