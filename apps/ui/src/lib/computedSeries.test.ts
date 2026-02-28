import { describe, expect, it } from 'vitest';
import { computeDerivedSeries, derivedModeLabel } from './computedSeries';

describe('computedSeries', () => {
  it('computes EMA with a finite output after first finite point', () => {
    const result = computeDerivedSeries([1, 2, 3, 4], 'ema', 3);
    expect(result[0]).toBeCloseTo(1);
    expect(result[3]).toBeGreaterThan(2.5);
  });

  it('computes deltas and keeps first value as NaN', () => {
    const result = computeDerivedSeries([2, 5, 4], 'delta');
    expect(Number.isNaN(result[0])).toBe(true);
    expect(result[1]).toBe(3);
    expect(result[2]).toBe(-1);
  });

  it('computes percent change and skips divide-by-zero', () => {
    const result = computeDerivedSeries([0, 10, 5], 'pct_change');
    expect(Number.isNaN(result[1])).toBe(true);
    expect(result[2]).toBeCloseTo(-50);
  });

  it('computes cumulative average with sparse NaNs', () => {
    const result = computeDerivedSeries([1, Number.NaN, 3], 'cumulative_avg');
    expect(result[0]).toBe(1);
    expect(Number.isNaN(result[1])).toBe(true);
    expect(result[2]).toBe(2);
  });

  it('formats derived labels', () => {
    expect(derivedModeLabel('none')).toBe('Raw');
    expect(derivedModeLabel('ema', 10)).toBe('EMA(10)');
  });
});

