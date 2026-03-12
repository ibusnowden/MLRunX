import { beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'

const { compareRunsMock, uPlotChartMock } = vi.hoisted(() => ({
  compareRunsMock: vi.fn(),
  uPlotChartMock: vi.fn(),
}))

vi.mock('@/lib/api', () => ({
  api: {
    compareRuns: compareRunsMock,
  },
}))

vi.mock('@/components/charts/UPlotChart', () => ({
  UPlotChart: (props: unknown) => {
    uPlotChartMock(props)
    return <div data-testid="uplot-chart" />
  },
}))

import { ComparePanel } from '../src/components/ComparePanel'

const compareResponse = {
  runs: [
    {
      run_id: 'run-rloo',
      run_name: 'RLOO',
      status: 'finished',
      series: [
        {
          name: 'reward',
          points: [
            { step: 1, mean: 1.0, min: 0.9, max: 1.1, count: 1 },
            { step: 2, mean: 1.2, min: 1.0, max: 1.3, count: 1 },
          ],
          total_points: 2,
          downsampled: false,
        },
        {
          name: 'loss',
          points: [
            { step: 1, mean: 0.4, min: 0.35, max: 0.45, count: 1 },
            { step: 2, mean: 0.35, min: 0.3, max: 0.4, count: 1 },
          ],
          total_points: 2,
          downsampled: false,
        },
      ],
    },
    {
      run_id: 'run-ppo',
      run_name: 'PPO',
      status: 'finished',
      series: [
        {
          name: 'reward',
          points: [
            { step: 1, mean: 0.8, min: 0.75, max: 0.85, count: 1 },
            { step: 2, mean: 0.9, min: 0.85, max: 0.95, count: 1 },
          ],
          total_points: 2,
          downsampled: false,
        },
        {
          name: 'loss',
          points: [
            { step: 1, mean: 0.3, min: 0.28, max: 0.32, count: 1 },
            { step: 2, mean: 0.25, min: 0.2, max: 0.3, count: 1 },
          ],
          total_points: 2,
          downsampled: false,
        },
      ],
    },
  ],
  common_metrics: ['reward', 'loss'],
  alignment: 'step',
}

const compareRunMetadata = {
  'run-rloo': {
    run_id: 'run-rloo',
    name: 'RLOO',
    status: 'finished',
    tags: {
      model_name: 'Qwen/Qwen3-1.7B',
      dataset_name: 'gsm8k',
      learning_rate: '5e-6',
    },
  },
  'run-ppo': {
    run_id: 'run-ppo',
    name: 'PPO',
    status: 'finished',
    tags: {
      model: 'Qwen/Qwen3-1.7B',
      dataset: 'math',
      seed: '42',
    },
  },
}

describe('ComparePanel', () => {
  beforeEach(() => {
    compareRunsMock.mockReset()
    uPlotChartMock.mockReset()
    compareRunsMock.mockResolvedValue(compareResponse)
  })

  it('renders multi-run chart data with alphabetically-first metric selected by default', async () => {
    render(<ComparePanel runIds={['run-rloo', 'run-ppo']} runMetadataById={compareRunMetadata} />)

    await screen.findByText('Compare Runs')
    await screen.findAllByText('RLOO')
    await screen.findAllByText('PPO')
    await screen.findByTestId('uplot-chart')

    const metricSelect = screen.getByRole('combobox', { name: 'Metric' }) as HTMLSelectElement
    expect(metricSelect.value).toBe('loss')

    await waitFor(() => {
      expect(compareRunsMock).toHaveBeenCalledWith(['run-rloo', 'run-ppo'], [], 5000, {
        limit: 200,
        offset: 0,
      })
    })

    await waitFor(() => {
      expect(uPlotChartMock).toHaveBeenCalled()
    })

    const chartProps = uPlotChartMock.mock.calls.at(-1)?.[0] as {
      xData: number[];
      tooltipVariant: string;
      series: Array<{
        label: string;
        data: Array<number | null>;
        tooltipLabel?: string;
        hoverMeta?: Array<{ label: string; value: string }>;
      }>;
    }
    expect(chartProps.xData).toEqual([1, 2])
    expect(chartProps.tooltipVariant).toBe('compare')
    expect(chartProps.series).toMatchObject([
      {
        label: 'RLOO',
        data: [0.4, 0.35],
        tooltipLabel: 'RLOO',
        hoverMeta: [
          { label: 'Model', value: 'Qwen/Qwen3-1.7B' },
          { label: 'Dataset', value: 'gsm8k' },
          { label: 'LR', value: '5e-6' },
        ],
      },
      {
        label: 'PPO',
        data: [0.3, 0.25],
        tooltipLabel: 'PPO',
        hoverMeta: [
          { label: 'Model', value: 'Qwen/Qwen3-1.7B' },
          { label: 'Dataset', value: 'math' },
          { label: 'Seed', value: '42' },
        ],
      },
    ])
  })

  it('updates overlay when selecting a different metric', async () => {
    render(<ComparePanel runIds={['run-rloo', 'run-ppo']} runMetadataById={compareRunMetadata} />)

    const metricSelect = (await screen.findByRole('combobox', { name: 'Metric' })) as HTMLSelectElement
    expect(metricSelect.value).toBe('loss')

    fireEvent.change(metricSelect, { target: { value: 'reward' } })

    await waitFor(() => {
      expect(metricSelect.value).toBe('reward')
    })

    await waitFor(() => {
      const chartProps = uPlotChartMock.mock.calls.at(-1)?.[0] as {
        xData: number[];
        series: Array<{ label: string; data: Array<number | null> }>;
      }
      expect(chartProps.xData).toEqual([1, 2])
      expect(chartProps.series).toMatchObject([
        { label: 'RLOO', data: [1.0, 1.2] },
        { label: 'PPO', data: [0.8, 0.9] },
      ])
    })
  })

  it('keeps hover metadata on derived series', async () => {
    render(<ComparePanel runIds={['run-rloo', 'run-ppo']} runMetadataById={compareRunMetadata} />)

    const derivedSelect = (await screen.findByRole('combobox', { name: 'Derived Series' })) as HTMLSelectElement
    fireEvent.change(derivedSelect, { target: { value: 'ema' } })

    const showRawCheckbox = await screen.findByRole('checkbox', { name: 'Show raw lines' })
    fireEvent.click(showRawCheckbox)

    await waitFor(() => {
      const chartProps = uPlotChartMock.mock.calls.at(-1)?.[0] as {
        series: Array<{
          label: string;
          tooltipLabel?: string;
          hoverMeta?: Array<{ label: string; value: string }>;
        }>;
      }

      expect(chartProps.series).toMatchObject([
        {
          label: 'RLOO EMA(16)',
          tooltipLabel: 'RLOO EMA(16)',
          hoverMeta: [
            { label: 'Model', value: 'Qwen/Qwen3-1.7B' },
            { label: 'Dataset', value: 'gsm8k' },
            { label: 'LR', value: '5e-6' },
          ],
        },
        {
          label: 'PPO EMA(16)',
          tooltipLabel: 'PPO EMA(16)',
          hoverMeta: [
            { label: 'Model', value: 'Qwen/Qwen3-1.7B' },
            { label: 'Dataset', value: 'math' },
            { label: 'Seed', value: '42' },
          ],
        },
      ])
    })
  })

  it('renders head-to-head delta rows for two-run comparison', async () => {
    render(<ComparePanel runIds={['run-rloo', 'run-ppo']} runMetadataById={compareRunMetadata} />)

    await screen.findByText('Selected metric head-to-head (loss)')
    const objectiveSelect = (await screen.findByRole('combobox', { name: 'Metric Objective' })) as HTMLSelectElement
    expect(objectiveSelect.value).toBe('lower')
    expect(screen.getByText('Objective: Lower is better')).toBeTruthy()
    expect(screen.getByText('Winner: PPO')).toBeTruthy()
    expect(screen.getByText('Baseline')).toBeTruthy()
    expect(screen.getByText('Candidate')).toBeTruthy()
    expect(screen.getAllByText('-0.1000').length).toBeGreaterThan(0)
    expect(screen.getAllByText('-28.57%').length).toBeGreaterThan(0)
  })

  it('allows overriding metric objective per metric', async () => {
    render(<ComparePanel runIds={['run-rloo', 'run-ppo']} runMetadataById={compareRunMetadata} />)

    const objectiveSelect = (await screen.findByRole('combobox', { name: 'Metric Objective' })) as HTMLSelectElement
    expect(objectiveSelect.value).toBe('lower')

    fireEvent.change(objectiveSelect, { target: { value: 'higher' } })

    await waitFor(() => {
      expect(screen.getByText('Objective: Higher is better')).toBeTruthy()
      expect(screen.getByText('Winner: RLOO')).toBeTruthy()
    })

    const metricSelect = screen.getByRole('combobox', { name: 'Metric' }) as HTMLSelectElement
    fireEvent.change(metricSelect, { target: { value: 'reward' } })
    await waitFor(() => {
      expect(objectiveSelect.value).toBe('higher')
    })

    fireEvent.change(metricSelect, { target: { value: 'loss' } })
    await waitFor(() => {
      expect(objectiveSelect.value).toBe('higher')
      expect(screen.getByText('Winner: RLOO')).toBeTruthy()
    })
  })

  it('emits objective override updates when objective is changed', async () => {
    const onMetricObjectiveOverridesChange = vi.fn()
    render(
      <ComparePanel
        runIds={['run-rloo', 'run-ppo']}
        onMetricObjectiveOverridesChange={onMetricObjectiveOverridesChange}
        runMetadataById={compareRunMetadata}
      />
    )

    const objectiveSelect = (await screen.findByRole('combobox', { name: 'Metric Objective' })) as HTMLSelectElement
    fireEvent.change(objectiveSelect, { target: { value: 'higher' } })

    await waitFor(() => {
      expect(onMetricObjectiveOverridesChange).toHaveBeenLastCalledWith({ loss: 'higher' })
    })

    fireEvent.change(objectiveSelect, { target: { value: 'lower' } })
    await waitFor(() => {
      expect(onMetricObjectiveOverridesChange).toHaveBeenLastCalledWith({})
    })
  })

  it('supports all-metrics scope and shows missing metric values', async () => {
    compareRunsMock.mockResolvedValue({
      runs: [
        {
          run_id: 'run-a',
          run_name: 'Run A',
          status: 'finished',
          series: [
            {
              name: 'loss',
              points: [{ step: 1, mean: 0.4, min: 0.4, max: 0.4, count: 1 }],
              total_points: 1,
              downsampled: false,
            },
          ],
        },
        {
          run_id: 'run-b',
          run_name: 'Run B',
          status: 'finished',
          series: [
            {
              name: 'loss',
              points: [{ step: 1, mean: 0.35, min: 0.35, max: 0.35, count: 1 }],
              total_points: 1,
              downsampled: false,
            },
            {
              name: 'reward',
              points: [{ step: 1, mean: 1.1, min: 1.1, max: 1.1, count: 1 }],
              total_points: 1,
              downsampled: false,
            },
          ],
        },
      ],
      common_metrics: ['loss'],
      alignment: 'step',
    })

    render(<ComparePanel runIds={['run-a', 'run-b']} />)

    const scopeSelect = (await screen.findByRole('combobox', { name: 'Metric Scope' })) as HTMLSelectElement
    fireEvent.change(scopeSelect, { target: { value: 'all' } })

    const rewardRow = (await screen.findAllByRole('row')).find(
      (row) => row.textContent?.includes('reward') && row.textContent?.includes('1.1000')
    )
    expect(rewardRow).toBeTruthy()
    expect(within(rewardRow as HTMLElement).getAllByText('-').length).toBeGreaterThan(0)
  })

  it('swaps baseline and candidate order when swap is clicked', async () => {
    const onRunIdsChange = vi.fn()
    render(
      <ComparePanel
        runIds={['run-rloo', 'run-ppo']}
        onRunIdsChange={onRunIdsChange}
        runMetadataById={compareRunMetadata}
      />
    )

    const swapButton = await screen.findByRole('button', { name: 'Swap A/B' })
    fireEvent.click(swapButton)

    expect(onRunIdsChange).toHaveBeenCalledWith(['run-ppo', 'run-rloo'])
  })
})
