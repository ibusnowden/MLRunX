export function formatDuration(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined) return '-';
  if (seconds < 60) return `${seconds.toFixed(1)}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${Math.floor(seconds % 60)}s`;
  return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`;
}

export function safeMinMax(points: Array<{ min: number; max: number }>): { min?: number; max?: number } {
  if (points.length === 0) return {};
  const mins = points.map((p) => p.min);
  const maxes = points.map((p) => p.max);
  return {
    min: Math.min(...mins),
    max: Math.max(...maxes),
  };
}

export function formatFixed(value: number | undefined, decimals = 4): string {
  if (value === undefined || Number.isNaN(value) || !Number.isFinite(value)) return '-';
  return value.toFixed(decimals);
}
