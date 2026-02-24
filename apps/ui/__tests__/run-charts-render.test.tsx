import { Suspense, type ReactNode } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { act, render, screen, waitFor } from '@testing-library/react'

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
      metrics_count: 4,
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

    getMetricsMock.mockResolvedValue({
      run_id: 'run-1',
      available_metrics: ['loss', 'val_loss'],
      series: [
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
      ],
    })

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

  it('loads run-detail metrics and renders per-metric line charts without area fill', async () => {
    await act(async () => {
      render(
        <Suspense fallback={<div>Loading run route...</div>}>
          <RunDetailPage params={Promise.resolve({ run_id: 'run-1' })} />
        </Suspense>
      )
    })

    await screen.findByRole('heading', { name: 'char-gpt-scratch' })
    await screen.findByText('Metric Groups')
    await screen.findAllByTestId('uplot-chart')

    await waitFor(() => {
      expect(getRunMock).toHaveBeenCalledWith('run-1')
      expect(getMetricsMock).toHaveBeenCalledWith('run-1', { maxPoints: 1200 })
      expect(getRunEventsMock).toHaveBeenCalledWith('run-1', { afterId: undefined, limit: 200 })
    })

    const runMetricChartCalls = uPlotChartMock.mock.calls
      .map(([props]) => props as { yLabel?: string; areaFill?: boolean })
      .filter((props) => props.yLabel === 'loss' || props.yLabel === 'val_loss')

    expect(runMetricChartCalls.length).toBeGreaterThanOrEqual(2)
    runMetricChartCalls.forEach((props) => {
      expect(props.areaFill).toBe(false)
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
      expect(compareRunsMock).toHaveBeenCalledWith(['run-a', 'run-b'], [], 5000)
    })

    const compareCall = uPlotChartMock.mock.calls
      .map(([props]) => props as {
        xData?: number[];
        series?: Array<{ label: string; data: Array<number | null> }>;
      })
      .find((props) => props.xData?.length === 3 && props.series?.length === 2)

    expect(compareCall).toBeDefined()
    expect(compareCall?.xData).toEqual([1, 2, 3])
    expect(compareCall?.series).toMatchObject([
      { label: 'Run A', data: [1.1, null, 1.4] },
      { label: 'Run B', data: [null, 0.9, null] },
    ])
  })
})
