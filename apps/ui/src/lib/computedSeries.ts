export type DerivedSeriesMode = 'none' | 'ema' | 'delta' | 'pct_change' | 'cumulative_avg';

function isFinitePoint(value: number): boolean {
  return Number.isFinite(value);
}

function sanitizeWindow(window: number): number {
  if (!Number.isFinite(window)) return 2;
  return Math.max(2, Math.min(512, Math.floor(window)));
}

function computeEma(data: number[], window: number): number[] {
  const alpha = 2 / (window + 1);
  const output = new Array<number>(data.length).fill(Number.NaN);
  let last = Number.NaN;

  for (let idx = 0; idx < data.length; idx += 1) {
    const value = data[idx];
    if (!isFinitePoint(value)) {
      output[idx] = Number.NaN;
      continue;
    }

    if (!isFinitePoint(last)) {
      last = value;
    } else {
      last = alpha * value + (1 - alpha) * last;
    }
    output[idx] = last;
  }

  return output;
}

function computeDelta(data: number[]): number[] {
  const output = new Array<number>(data.length).fill(Number.NaN);
  for (let idx = 1; idx < data.length; idx += 1) {
    const prev = data[idx - 1];
    const curr = data[idx];
    if (isFinitePoint(prev) && isFinitePoint(curr)) {
      output[idx] = curr - prev;
    }
  }
  return output;
}

function computePctChange(data: number[]): number[] {
  const output = new Array<number>(data.length).fill(Number.NaN);
  for (let idx = 1; idx < data.length; idx += 1) {
    const prev = data[idx - 1];
    const curr = data[idx];
    if (!isFinitePoint(prev) || !isFinitePoint(curr) || prev === 0) continue;
    output[idx] = ((curr - prev) / Math.abs(prev)) * 100;
  }
  return output;
}

function computeCumulativeAvg(data: number[]): number[] {
  const output = new Array<number>(data.length).fill(Number.NaN);
  let sum = 0;
  let count = 0;
  for (let idx = 0; idx < data.length; idx += 1) {
    const value = data[idx];
    if (!isFinitePoint(value)) {
      output[idx] = Number.NaN;
      continue;
    }
    sum += value;
    count += 1;
    output[idx] = sum / count;
  }
  return output;
}

export function computeDerivedSeries(
  data: number[],
  mode: DerivedSeriesMode,
  window = 16
): number[] {
  const normalizedWindow = sanitizeWindow(window);
  switch (mode) {
    case 'ema':
      return computeEma(data, normalizedWindow);
    case 'delta':
      return computeDelta(data);
    case 'pct_change':
      return computePctChange(data);
    case 'cumulative_avg':
      return computeCumulativeAvg(data);
    case 'none':
    default:
      return data.slice();
  }
}

export function derivedModeLabel(mode: DerivedSeriesMode, window = 16): string {
  const normalizedWindow = sanitizeWindow(window);
  switch (mode) {
    case 'ema':
      return `EMA(${normalizedWindow})`;
    case 'delta':
      return 'Delta';
    case 'pct_change':
      return '% Change';
    case 'cumulative_avg':
      return 'Cumulative Avg';
    case 'none':
    default:
      return 'Raw';
  }
}

