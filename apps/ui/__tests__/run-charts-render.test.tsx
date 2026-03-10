import { Suspense, type ReactNode } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'

const {
  getRunMock,
  getMetricsMock,
  getRunEventsMock,
  deleteRunMock,
  compareRunsMock,
  uPlotChartMock,
  pushMock,
  replaceMock,
} = vi.hoisted(() => ({
  getRunMock: vi.fn(),
  getMetricsMock: vi.fn(),
  getRunEventsMock: vi.fn(),
  deleteRunMock: vi.fn(),
  compareRunsMock: vi.fn(),
  uPlotChartMock: vi.fn(),
  pushMock: vi.fn(),
  replaceMock: vi.fn(),
}))

vi.mock('next/link', () => ({
  default: ({ href, children }: { href: string; children: ReactNode }) => (
    <a href={href}>{children}</a>
  ),
}))

vi.mock('next/navigation', () => ({
  useRouter: () => ({
    push: pushMock,
    replace: replaceMock,
  }),
  usePathname: () => '/runs/run-1',
  useSearchParams: () => new URLSearchParams('view=charts'),
}))

vi.mock('@/components/ThemeProvider', () => ({
  useTheme: () => ({
    isDark: true,
    toggleTheme: vi.fn(),
  }),
}))

vi.mock('@/lib/useAutoRefresh', () => ({
  useAutoRefresh: vi.fn(),
}))

vi.mock('@/lib/api', () => ({
  api: {
    getRun: getRunMock,
    getMetrics: getMetricsMock,
    getRunEvents: getRunEventsMock,
    deleteRun: deleteRunMock,
    compareRuns: compareRunsMock,
  },
}))

vi.mock('@/components/charts/UPlotChart', () => ({
  UPlotChart: (props: unknown) => {
    uPlotChartMock(props)
    return <div data-testid="uplot-chart" />
  },
}))

import RunDetailPage from '../src/app/runs/[run_id]/page'
import { ComparePanel } from '../src/components/ComparePanel'

const allRunMetricSeries = [
  {
    name: 'loss',
    points: [
      { step: 1, mean: 2.6, min: 2.5, max: 2.7, count: 1 },
      { step: 2, mean: 2.2, min: 2.1, max: 2.3, count: 1 },
    ],
    total_points: 2,
    downsampled: false,
  },
  {
    name: 'val_loss',
    points: [
      { step: 1, mean: 2.8, min: 2.7, max: 2.9, count: 1 },
      { step: 2, mean: 2.4, min: 2.3, max: 2.5, count: 1 },
    ],
    total_points: 2,
    downsampled: false,
  },
  {
    name: 'learning_rate',
    points: [
      { step: 1, mean: 0.001, min: 0.001, max: 0.001, count: 1 },
      { step: 2, mean: 0.0008, min: 0.0008, max: 0.0008, count: 1 },
    ],
    total_points: 2,
    downsampled: false,
  },
  {
    name: 'accuracy',
    points: [
      { step: 1, mean: 0.42, min: 0.42, max: 0.42, count: 1 },
      { step: 2, mean: 0.54, min: 0.54, max: 0.54, count: 1 },
    ],
    total_points: 2,
    downsampled: false,
  },
  {
    name: 'reward',
    points: [
      { step: 1, mean: 0.4, min: 0.35, max: 0.45, count: 1 },
      { step: 2, mean: 0.55, min: 0.5, max: 0.6, count: 1 },
    ],
    total_points: 2,
    downsampled: false,
  },
  {
    name: 'throughput',
    points: [
      { step: 1, mean: 850, min: 840, max: 860, count: 1 },
      { step: 2, mean: 910, min: 900, max: 920, count: 1 },
    ],
    total_points: 2,
    downsampled: false,
  },
  {
    name: 'epoch_time',
    points: [
      { step: 1, mean: 12.2, min: 12.1, max: 12.3, count: 1 },
      { step: 2, mean: 11.4, min: 11.3, max: 11.5, count: 1 },
    ],
    total_points: 2,
    downsampled: false,
  },
  {
    name: 'grad_norm/layer1',
    points: [
      { step: 1, mean: 0.9, min: 0.9, max: 0.9, count: 1 },
      { step: 2, mean: 0.7, min: 0.7, max: 0.7, count: 1 },
    ],
    total_points: 2,
    downsampled: false,
  },
  {
    name: 'grad_norm/layer2',
    points: [
      { step: 1, mean: 1.1, min: 1.1, max: 1.1, count: 1 },
      { step: 2, mean: 0.95, min: 0.95, max: 0.95, count: 1 },
    ],
    total_points: 2,
    downsampled: false,
  },
  {
    name: 'gpu/memory',
    points: [
      { step: 1, mean: 18.4, min: 18.4, max: 18.4, count: 1 },
      { step: 2, mean: 18.8, min: 18.8, max: 18.8, count: 1 },
    ],
    total_points: 2,
    downsampled: false,
  },
  {
    name: 'gpu/utilization',
    points: [
      { step: 1, mean: 72, min: 72, max: 72, count: 1 },
      { step: 2, mean: 76, min: 76, max: 76, count: 1 },
    ],
    total_points: 2,
    downsampled: false,
  },
  {
    name: 'cpu_utilization',
    points: [
      { step: 1, mean: 48, min: 48, max: 48, count: 1 },
      { step: 2, mean: 53, min: 53, max: 53, count: 1 },
    ],
    total_points: 2,
    downsampled: false,
  },
]

const allRunMetricNames = allRunMetricSeries.map((series) => series.name)
const runMetricSeriesByName = new Map(allRunMetricSeries.map((series) => [series.name, series]))

describe('Run charts rendering', () => {
  beforeEach(() => {
    pushMock.mockReset()
    replaceMock.mockReset()
    getRunMock.mockReset()
    getMetricsMock.mockReset()
    getRunEventsMock.mockReset()
    deleteRunMock.mockReset()
    compareRunsMock.mockReset()
    uPlotChartMock.mockReset()

    getRunMock.mockResolvedValue({
      run_id: 'run-1',
      project_id: 'project-1',
      name: 'char-gpt-scratch',
      status: 'running',
      metrics_count: 24,
      params_count: 0,
      tags: {
        model: 'mini-gpt',
        dataset: 'names',
      },
      created_at: '2026-02-22 10:00:00',
      updated_at: '2026-02-22 10:03:00',
      duration_seconds: 180,
      metrics_summary: [
        { name: 'loss', last_value: 2.2, last_step: 2 },
        { name: 'val_loss', last_value: 2.4, last_step: 2 },
      ],
    })

    getMetricsMock.mockImplementation(async (runId: string, params?: { names?: string[]; maxPoints?: number }) => ({
      run_id: runId,
      available_metrics: allRunMetricNames,
      metric_aliases: {
        learning_rate: 'lr',
      },
      series: (params?.names?.length ? params.names : allRunMetricNames)
        .map((name) => runMetricSeriesByName.get(name))
        .filter((series) => Boolean(series)),
    }))

    getRunEventsMock.mockResolvedValue({
      run_id: 'run-1',
      events: [
        {
          id: 1,
          run_id: 'run-1',
          level: 'info',
          source: 'trainer',
          message: 'step 1 / 1000\nloss 2.6',
          step: 1,
          timestamp: 1739920831,
          created_at: '2026-02-22 10:00:31',
        },
      ],
      next_after_id: 1,
      has_more: false,
    })
  })

  it('loads run-detail metrics and renders compact reference-style charts without area fill', async () => {
    await act(async () => {
      render(
        <Suspense fallback={<div>Loading run route...</div>}>
          <RunDetailPage params={Promise.resolve({ run_id: 'run-1' })} />
        </Suspense>
      )
    })

    await screen.findByRole('heading', { name: 'char-gpt-scratch' })
    await screen.findByText('Chart Controls')
    await screen.findByRole('button', { name: 'Loss Metrics (2)' })
    await screen.findAllByTestId('uplot-chart')

    await waitFor(() => {
      expect(getRunMock).toHaveBeenCalledWith('run-1')
      expect(getMetricsMock).toHaveBeenCalledWith('run-1', {
        names: ['loss', 'val_loss'],
        maxPoints: 1200,
      })
      expect(getRunEventsMock).toHaveBeenCalledWith('run-1', { afterId: undefined, limit: 200 })
    })

    expect(screen.getAllByTestId('uplot-chart')).toHaveLength(2)

    const chartCalls = uPlotChartMock.mock.calls
      .map(([props]) => props as {
        areaFill?: boolean
        dashedGrid?: boolean
        lineWidth?: number
        showLegend?: boolean
        showXAxisLabel?: boolean
        showYAxisLabel?: boolean
        variant?: string
        xLabel?: string
        yLabel?: string
      })

    const heroLossCall = chartCalls.find((props) => props.yLabel === 'loss' && props.showYAxisLabel === true)
    const valLossCall = chartCalls.find((props) => props.yLabel === 'val_loss')

    expect(heroLossCall).toBeDefined()
    expect(valLossCall).toBeDefined()
    expect(heroLossCall).toMatchObject({
      areaFill: false,
      dashedGrid: true,
      lineWidth: 1.9,
      showLegend: false,
      showXAxisLabel: true,
      showYAxisLabel: true,
      variant: 'reference',
      xLabel: 'step',
    })
    expect(valLossCall).toMatchObject({
      areaFill: false,
      dashedGrid: true,
      showLegend: false,
      showXAxisLabel: false,
      showYAxisLabel: false,
      variant: 'reference',
    })
  })

  it('loads additional metric groups on demand, reuses cached groups, and batches all-metrics rendering', async () => {
    await act(async () => {
      render(
        <Suspense fallback={<div>Loading run route...</div>}>
          <RunDetailPage params={Promise.resolve({ run_id: 'run-1' })} />
        </Suspense>
      )
    })

    await screen.findByRole('button', { name: 'Loss Metrics (2)' })
    await waitFor(() => {
      expect(getMetricsMock).toHaveBeenCalledTimes(1)
    })
    expect(screen.getAllByTestId('uplot-chart')).toHaveLength(2)

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Learning Rate (1)' }))
    })

    await waitFor(() => {
      expect(getMetricsMock).toHaveBeenCalledWith('run-1', {
        names: ['learning_rate'],
        maxPoints: 1200,
      })
    })
    await waitFor(() => {
      expect(screen.getAllByTestId('uplot-chart')).toHaveLength(1)
    })

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Loss Metrics (2)' }))
    })
    await waitFor(() => {
      expect(screen.getAllByTestId('uplot-chart')).toHaveLength(2)
    })

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Learning Rate (1)' }))
    })
    await waitFor(() => {
      expect(screen.getAllByTestId('uplot-chart')).toHaveLength(1)
    })
    expect(getMetricsMock).toHaveBeenCalledTimes(2)

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'All (12)' }))
    })

    await waitFor(() => {
      expect(getMetricsMock).toHaveBeenCalledWith('run-1', {
        names: [
          'accuracy',
          'reward',
          'throughput',
          'epoch_time',
          'grad_norm/layer1',
          'grad_norm/layer2',
          'gpu/memory',
          'gpu/utilization',
          'cpu_utilization',
        ],
        maxPoints: 1200,
      })
    })

    const showMoreButton = await screen.findByRole('button', { name: 'Show 3 more charts' })
    expect(screen.getAllByTestId('uplot-chart')).toHaveLength(9)
    await act(async () => {
      fireEvent.click(showMoreButton)
    })

    await waitFor(() => {
      expect(screen.getAllByTestId('uplot-chart')).toHaveLength(12)
    })
  })

  it('renders compare chart for multiple runs with sparse points without crashing', async () => {
    compareRunsMock.mockResolvedValue({
      runs: [
        {
          run_id: 'run-a',
          run_name: 'Run A',
          status: 'finished',
          series: [
            {
              name: 'reward',
              points: [
                { step: 1, mean: 1.1, min: 1.0, max: 1.2, count: 1 },
                { step: 3, mean: 1.4, min: 1.3, max: 1.5, count: 1 },
              ],
              total_points: 2,
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
              name: 'reward',
              points: [{ step: 2, mean: 0.9, min: 0.8, max: 1.0, count: 1 }],
              total_points: 1,
              downsampled: false,
            },
          ],
        },
      ],
      common_metrics: ['reward'],
      alignment: 'step',
    })

    render(<ComparePanel runIds={['run-a', 'run-b']} />)

    await screen.findByText('Compare Runs')
    await screen.findByTestId('uplot-chart')

    await waitFor(() => {
      expect(compareRunsMock).toHaveBeenCalledWith(['run-a', 'run-b'], [], 5000, {
        limit: 200,
        offset: 0,
      })
    })

    const compareCall = uPlotChartMock.mock.calls
      .map(([props]) => props as {
        xData?: number[];
        series?: Array<{ label: string; data: number[] }>;
      })
      .find((props) => props.xData?.length === 3 && props.series?.length === 2)

    expect(compareCall).toBeDefined()
    expect(compareCall?.xData).toEqual([1, 2, 3])
    expect(compareCall?.series).toMatchObject([
      { label: 'Run A', data: [1.1, Number.NaN, 1.4] },
      { label: 'Run B', data: [Number.NaN, 0.9, Number.NaN] },
    ])
  })
})
