const RUN_FILTER_PRESETS_KEY = 'mlrunx_run_filter_presets_v1';
const COMPARE_PRESETS_KEY = 'mlrunx_compare_presets_v1';

const MAX_PRESETS = 20;
const MAX_NAME_LEN = 64;
const MAX_QUERY_LEN = 512;
const MAX_RUN_IDS_PER_PRESET = 100;

export interface RunFilterPreset {
  id: string;
  name: string;
  query: string;
  updatedAt: string;
}

export interface ComparePreset {
  id: string;
  name: string;
  runIds: string[];
  updatedAt: string;
}

function safeReadStorage(key: string): string | null {
  if (typeof window === 'undefined') return null;
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}

function safeWriteStorage(key: string, value: string) {
  if (typeof window === 'undefined') return;
  try {
    window.localStorage.setItem(key, value);
  } catch {
    // Ignore storage failures in restricted environments.
  }
}

function clampText(value: string, maxLen: number): string {
  return value.trim().slice(0, maxLen);
}

function makePresetId(prefix: string): string {
  return `${prefix}_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 8)}`;
}

function parseRunFilterPresets(raw: string | null): RunFilterPreset[] {
  if (!raw) return [];
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) return [];
    return parsed
      .filter((entry): entry is RunFilterPreset => {
        if (!entry || typeof entry !== 'object') return false;
        const candidate = entry as Partial<RunFilterPreset>;
        return (
          typeof candidate.id === 'string' &&
          typeof candidate.name === 'string' &&
          typeof candidate.query === 'string' &&
          typeof candidate.updatedAt === 'string'
        );
      })
      .map((entry) => ({
        id: clampText(entry.id, 128),
        name: clampText(entry.name, MAX_NAME_LEN),
        query: clampText(entry.query, MAX_QUERY_LEN),
        updatedAt: entry.updatedAt,
      }))
      .filter((entry) => entry.id.length > 0 && entry.name.length > 0 && entry.query.length > 0)
      .slice(0, MAX_PRESETS);
  } catch {
    return [];
  }
}

function parseComparePresets(raw: string | null): ComparePreset[] {
  if (!raw) return [];
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) return [];
    return parsed
      .filter((entry): entry is ComparePreset => {
        if (!entry || typeof entry !== 'object') return false;
        const candidate = entry as Partial<ComparePreset>;
        return (
          typeof candidate.id === 'string' &&
          typeof candidate.name === 'string' &&
          Array.isArray(candidate.runIds) &&
          typeof candidate.updatedAt === 'string'
        );
      })
      .map((entry) => ({
        id: clampText(entry.id, 128),
        name: clampText(entry.name, MAX_NAME_LEN),
        runIds: entry.runIds
          .filter((value): value is string => typeof value === 'string')
          .map((value) => clampText(value, 128))
          .filter(Boolean)
          .slice(0, MAX_RUN_IDS_PER_PRESET),
        updatedAt: entry.updatedAt,
      }))
      .filter((entry) => entry.id.length > 0 && entry.name.length > 0 && entry.runIds.length > 0)
      .slice(0, MAX_PRESETS);
  } catch {
    return [];
  }
}

export function loadRunFilterPresets(): RunFilterPreset[] {
  return parseRunFilterPresets(safeReadStorage(RUN_FILTER_PRESETS_KEY));
}

export function saveRunFilterPresets(presets: RunFilterPreset[]) {
  safeWriteStorage(RUN_FILTER_PRESETS_KEY, JSON.stringify(presets.slice(0, MAX_PRESETS)));
}

export function upsertRunFilterPreset(
  presets: RunFilterPreset[],
  name: string,
  query: string
): { presets: RunFilterPreset[]; saved: RunFilterPreset } {
  const normalizedName = clampText(name, MAX_NAME_LEN);
  const normalizedQuery = clampText(query, MAX_QUERY_LEN);
  if (!normalizedName || !normalizedQuery) {
    throw new Error('Preset name and query are required.');
  }

  const now = new Date().toISOString();
  const existingIndex = presets.findIndex(
    (preset) => preset.name.toLowerCase() === normalizedName.toLowerCase()
  );

  if (existingIndex >= 0) {
    const updated: RunFilterPreset = {
      ...presets[existingIndex],
      name: normalizedName,
      query: normalizedQuery,
      updatedAt: now,
    };
    const next = [updated, ...presets.filter((_, idx) => idx !== existingIndex)].slice(
      0,
      MAX_PRESETS
    );
    return { presets: next, saved: updated };
  }

  const created: RunFilterPreset = {
    id: makePresetId('filter'),
    name: normalizedName,
    query: normalizedQuery,
    updatedAt: now,
  };
  const next = [created, ...presets].slice(0, MAX_PRESETS);
  return { presets: next, saved: created };
}

export function deleteRunFilterPreset(
  presets: RunFilterPreset[],
  presetId: string
): RunFilterPreset[] {
  return presets.filter((preset) => preset.id !== presetId);
}

export function loadComparePresets(): ComparePreset[] {
  return parseComparePresets(safeReadStorage(COMPARE_PRESETS_KEY));
}

export function saveComparePresets(presets: ComparePreset[]) {
  safeWriteStorage(COMPARE_PRESETS_KEY, JSON.stringify(presets.slice(0, MAX_PRESETS)));
}

export function normalizeRunIdSet(runIds: string[]): string[] {
  return Array.from(
    new Set(
      runIds
        .map((runId) => clampText(runId, 128))
        .filter(Boolean)
    )
  )
    .sort((a, b) => a.localeCompare(b))
    .slice(0, MAX_RUN_IDS_PER_PRESET);
}

export function upsertComparePreset(
  presets: ComparePreset[],
  name: string,
  runIds: string[]
): { presets: ComparePreset[]; saved: ComparePreset } {
  const normalizedName = clampText(name, MAX_NAME_LEN);
  const normalizedRunIds = normalizeRunIdSet(runIds);
  if (!normalizedName || normalizedRunIds.length === 0) {
    throw new Error('Preset name and run IDs are required.');
  }

  const now = new Date().toISOString();
  const existingIndex = presets.findIndex(
    (preset) => preset.name.toLowerCase() === normalizedName.toLowerCase()
  );

  if (existingIndex >= 0) {
    const updated: ComparePreset = {
      ...presets[existingIndex],
      name: normalizedName,
      runIds: normalizedRunIds,
      updatedAt: now,
    };
    const next = [updated, ...presets.filter((_, idx) => idx !== existingIndex)].slice(
      0,
      MAX_PRESETS
    );
    return { presets: next, saved: updated };
  }

  const created: ComparePreset = {
    id: makePresetId('compare'),
    name: normalizedName,
    runIds: normalizedRunIds,
    updatedAt: now,
  };
  const next = [created, ...presets].slice(0, MAX_PRESETS);
  return { presets: next, saved: created };
}

export function deleteComparePreset(
  presets: ComparePreset[],
  presetId: string
): ComparePreset[] {
  return presets.filter((preset) => preset.id !== presetId);
}
