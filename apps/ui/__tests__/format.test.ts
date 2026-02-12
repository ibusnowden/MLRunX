import { describe, expect, it } from 'vitest';
import { formatDuration, formatFixed, safeMinMax } from '../src/lib/format';

describe('format helpers', () => {
  it('formats duration consistently', () => {
    expect(formatDuration(null)).toBe('-');
    expect(formatDuration(undefined)).toBe('-');
    expect(formatDuration(12.34)).toBe('12.3s');
    expect(formatDuration(90)).toBe('1m 30s');
    expect(formatDuration(3661)).toBe('1h 1m');
  });

  it('computes safe min/max with empty guard', () => {
    expect(safeMinMax([])).toEqual({});
    expect(safeMinMax([{ min: 0.3, max: 0.7 }, { min: 0.1, max: 1.1 }])).toEqual({
      min: 0.1,
      max: 1.1,
    });
  });

  it('formats fixed values safely', () => {
    expect(formatFixed(undefined)).toBe('-');
    expect(formatFixed(Number.NaN)).toBe('-');
    expect(formatFixed(Number.POSITIVE_INFINITY)).toBe('-');
    expect(formatFixed(0.12345)).toBe('0.1235');
    expect(formatFixed(2.5, 2)).toBe('2.50');
  });
});
