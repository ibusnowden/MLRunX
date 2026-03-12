export interface CompareTooltipMetaItem {
  label: string;
  value: string;
}

export interface CompareTooltipRowSource {
  label: string;
  tooltipLabel?: string;
  color?: string;
  value: number;
  hoverMeta?: CompareTooltipMetaItem[];
  yDistance: number;
}

export interface CompareTooltipRow {
  label: string;
  color: string;
  hoverMeta: CompareTooltipMetaItem[];
  isActive: boolean;
  value: number;
  valueLabel: string;
}

const COMPARE_TOOLTIP_VALUE_FORMATTER = new Intl.NumberFormat('en-US', {
  maximumFractionDigits: 6,
});

const PRIMARY_METADATA_CANDIDATES: Array<{ label: string; keys: string[] }> = [
  { label: 'Model', keys: ['model', 'model_name'] },
  { label: 'Dataset', keys: ['dataset', 'data', 'dataset_name'] },
];

const TUNING_METADATA_CANDIDATES: Array<{ label: string; keys: string[] }> = [
  { label: 'Seed', keys: ['seed'] },
  { label: 'LR', keys: ['lr', 'learning_rate'] },
  { label: 'Batch Size', keys: ['batch_size'] },
  { label: 'Optimizer', keys: ['optimizer'] },
];

function normalizeKey(value: string): string {
  return value.toLowerCase().replace(/[^a-z0-9]/g, '_');
}

function pickTagValue(tags: Record<string, string>, candidates: string[]): string | undefined {
  const index = new Map<string, string>();

  Object.entries(tags).forEach(([key, value]) => {
    const trimmed = value.trim();
    if (!trimmed) return;
    index.set(normalizeKey(key), trimmed);
  });

  for (const candidate of candidates) {
    const value = index.get(normalizeKey(candidate));
    if (value) return value;
  }

  return undefined;
}

function humanizeTagLabel(raw: string): string {
  return raw
    .replace(/[_/.-]+/g, ' ')
    .replace(/\b\w/g, (char) => char.toUpperCase());
}

export function buildCompareTooltipMetadata(tags: Record<string, string>): CompareTooltipMetaItem[] {
  const items: CompareTooltipMetaItem[] = [];

  for (const entry of PRIMARY_METADATA_CANDIDATES) {
    const value = pickTagValue(tags, entry.keys);
    if (value) {
      items.push({ label: entry.label, value });
    }
  }

  for (const entry of TUNING_METADATA_CANDIDATES) {
    const value = pickTagValue(tags, entry.keys);
    if (value) {
      items.push({ label: entry.label, value });
      break;
    }
  }

  if (items.length > 0) {
    // Cap at 3 items (model + dataset + one tuning field) to keep the tooltip compact.
    return items.slice(0, 3);
  }

  return Object.entries(tags)
    .map(([key, value]) => ({ key, value: value.trim() }))
    .filter(({ value }) => value.length > 0)
    .slice(0, 2)
    .map(({ key, value }) => ({
      label: humanizeTagLabel(key),
      value,
    }));
}

export function formatCompareTooltipValue(value: number): string {
  return COMPARE_TOOLTIP_VALUE_FORMATTER.format(value);
}

export function buildCompareTooltipRows(entries: CompareTooltipRowSource[]): CompareTooltipRow[] {
  const visibleEntries = entries.filter((entry) => Number.isFinite(entry.value));
  if (visibleEntries.length === 0) return [];

  const activeEntry = visibleEntries.reduce((best, current) => {
    if (!best) return current;
    return current.yDistance < best.yDistance ? current : best;
  }, visibleEntries[0]);

  return [...visibleEntries]
    .sort((left, right) => right.value - left.value)
    .map((entry) => ({
      label: entry.tooltipLabel || entry.label,
      color: entry.color || '#9ca3af',
      hoverMeta: entry.hoverMeta ?? [],
      isActive: entry === activeEntry,
      value: entry.value,
      valueLabel: formatCompareTooltipValue(entry.value),
    }));
}

export function formatCompareTooltipMetaLine(meta: CompareTooltipMetaItem[]): string {
  return meta.map((entry) => `${entry.label}: ${entry.value}`).join(' • ');
}
