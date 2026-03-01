export type MetricObjective = 'higher' | 'lower';

const MAX_METRIC_OVERRIDES = 100;
const MAX_METRIC_NAME_LEN = 128;

function isMetricObjective(value: unknown): value is MetricObjective {
  return value === 'higher' || value === 'lower';
}

function normalizeMetricName(metricName: string): string {
  return metricName.trim().slice(0, MAX_METRIC_NAME_LEN);
}

export function normalizeMetricObjectiveOverrides(
  overrides: Record<string, MetricObjective>
): Record<string, MetricObjective> {
  const entries = Object.entries(overrides)
    .map(([metricName, objective]) => [normalizeMetricName(metricName), objective] as const)
    .filter(([metricName, objective]) => metricName.length > 0 && isMetricObjective(objective))
    .slice(0, MAX_METRIC_OVERRIDES)
    .sort(([left], [right]) => left.localeCompare(right));
  return Object.fromEntries(entries);
}

export function parseMetricObjectiveOverridesParam(
  raw: string | null
): Record<string, MetricObjective> {
  if (!raw) return {};
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return {};
    const entries = Object.entries(parsed as Record<string, unknown>)
      .filter(([, objective]) => isMetricObjective(objective))
      .map(([metricName, objective]) => [metricName, objective as MetricObjective] as const);
    return normalizeMetricObjectiveOverrides(Object.fromEntries(entries));
  } catch {
    return {};
  }
}

export function serializeMetricObjectiveOverridesParam(
  overrides: Record<string, MetricObjective>
): string | null {
  const normalized = normalizeMetricObjectiveOverrides(overrides);
  if (Object.keys(normalized).length === 0) return null;
  return JSON.stringify(normalized);
}
