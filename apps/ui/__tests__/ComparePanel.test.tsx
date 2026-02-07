import { beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'

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

describe('ComparePanel', () => {
  beforeEach(() => {
    compareRunsMock.mockReset()
    uPlotChartMock.mockReset()
    compareRunsMock.mockResolvedValue(compareResponse)
  })

  it('renders multi-run reward comparison data with reward selected by default', async () => {
    render(<ComparePanel runIds={['run-rloo', 'run-ppo']} />)

    await screen.findByText('Compare Runs')
    await screen.findAllByText('RLOO')
    await screen.findAllByText('PPO')
    await screen.findByTestId('uplot-chart')

    const metricSelect = screen.getByRole('combobox') as HTMLSelectElement
    expect(metricSelect.value).toBe('reward')

    await waitFor(() => {
      expect(compareRunsMock).toHaveBeenCalledWith(['run-rloo', 'run-ppo'], [], 5000)
    })

    await waitFor(() => {
      expect(uPlotChartMock).toHaveBeenCalled()
    })

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

  it('updates overlay when selecting a different metric', async () => {
    render(<ComparePanel runIds={['run-rloo', 'run-ppo']} />)

    const metricSelect = (await screen.findByRole('combobox')) as HTMLSelectElement
    expect(metricSelect.value).toBe('reward')

    fireEvent.change(metricSelect, { target: { value: 'loss' } })

    await waitFor(() => {
      expect(metricSelect.value).toBe('loss')
    })

    await waitFor(() => {
      const chartProps = uPlotChartMock.mock.calls.at(-1)?.[0] as {
        xData: number[];
        series: Array<{ label: string; data: Array<number | null> }>;
      }
      expect(chartProps.xData).toEqual([1, 2])
      expect(chartProps.series).toMatchObject([
        { label: 'RLOO', data: [0.4, 0.35] },
        { label: 'PPO', data: [0.3, 0.25] },
      ])
    })
  })
})
