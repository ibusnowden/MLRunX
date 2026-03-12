import { describe, expect, it } from 'vitest'

import { buildCompareTooltipMetadata, buildCompareTooltipRows } from './compareTooltip'

describe('buildCompareTooltipMetadata', () => {
  it('prefers model, dataset, and one tuning field in a fixed order', () => {
    expect(
      buildCompareTooltipMetadata({
        model_name: 'Qwen/Qwen3-1.7B',
        dataset_name: 'gsm8k',
        learning_rate: '5e-6',
        batch_size: '8',
      })
    ).toEqual([
      { label: 'Model', value: 'Qwen/Qwen3-1.7B' },
      { label: 'Dataset', value: 'gsm8k' },
      { label: 'LR', value: '5e-6' },
    ])
  })

  it('falls back to the first two non-empty user tags when no canonical fields exist', () => {
    expect(
      buildCompareTooltipMetadata({
        framework: 'pytorch',
        task: 'classification',
        notes: '   ',
      })
    ).toEqual([
      { label: 'Framework', value: 'pytorch' },
      { label: 'Task', value: 'classification' },
    ])
  })

  it('preserves full values for rendering-time truncation and handles empty tags', () => {
    const longValue = 'x'.repeat(72)
    expect(buildCompareTooltipMetadata({ model: longValue })).toEqual([
      { label: 'Model', value: longValue },
    ])
    expect(buildCompareTooltipMetadata({})).toEqual([])
  })
})

describe('buildCompareTooltipRows', () => {
  it('filters missing values, sorts descending, and keeps active-row selection by cursor proximity', () => {
    const rows = buildCompareTooltipRows([
      {
        label: 'Run A',
        color: '#ff0000',
        value: 0.407987,
        hoverMeta: [{ label: 'Model', value: 'Qwen' }],
        yDistance: 12,
      },
      {
        label: 'Run B',
        tooltipLabel: 'Run B EMA',
        color: '#00ff00',
        value: 0.37389,
        hoverMeta: [{ label: 'Dataset', value: 'gsm8k' }],
        yDistance: 2,
      },
      {
        label: 'Run C',
        color: '#0000ff',
        value: Number.NaN,
        yDistance: 1,
      },
    ])

    expect(rows).toHaveLength(2)
    expect(rows.map((row) => row.label)).toEqual(['Run A', 'Run B EMA'])
    expect(rows.map((row) => row.valueLabel)).toEqual(['0.407987', '0.37389'])
    expect(rows.map((row) => row.isActive)).toEqual([false, true])
    expect(rows[1]?.hoverMeta).toEqual([{ label: 'Dataset', value: 'gsm8k' }])
  })
})
