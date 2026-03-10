import { beforeEach, describe, expect, it, vi } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'

const {
  listRunsMock,
  compareRunsMock,
  getRunMock,
  pushMock,
  replaceMock,
  uPlotChartMock,
  navigationState,
} = vi.hoisted(() => ({
  listRunsMock: vi.fn(),
  compareRunsMock: vi.fn(),
  getRunMock: vi.fn(),
  pushMock: vi.fn(),
  replaceMock: vi.fn(),
  uPlotChartMock: vi.fn(),
  navigationState: {
    search: '',
  },
}))

vi.mock('next/navigation', () => ({
  useRouter: () => ({
    push: pushMock,
    replace: replaceMock,
  }),
  useSearchParams: () => new URLSearchParams(navigationState.search),
}))

vi.mock('@/components/ThemeProvider', () => ({
  useTheme: () => ({
    isDark: false,
    toggleTheme: vi.fn(),
  }),
}))

vi.mock('@/lib/useAutoRefresh', () => ({
  useAutoRefresh: vi.fn(),
}))

vi.mock('@/lib/api', () => ({
  api: {
    listRuns: listRunsMock,
    compareRuns: compareRunsMock,
    getRun: getRunMock,
  },
}))

vi.mock('@/components/charts/UPlotChart', () => ({
  UPlotChart: (props: unknown) => {
    uPlotChartMock(props)
    return <div data-testid="uplot-chart" />
  },
}))

import ComparePage from '../src/app/compare/page'

function ensureLocalStorageMock(): void {
  const localStorageValue = window.localStorage as Partial<Storage>
  if (
    typeof localStorageValue.getItem === 'function' &&
    typeof localStorageValue.setItem === 'function' &&
    typeof localStorageValue.removeItem === 'function' &&
    typeof localStorageValue.clear === 'function'
  ) {
    return
  }

  const store = new Map<string, string>()
  const mockStorage: Storage = {
    get length() {
      return store.size
    },
    clear() {
      store.clear()
    },
    getItem(key: string) {
      return store.get(key) ?? null
    },
    key(index: number) {
      return Array.from(store.keys())[index] ?? null
    },
    removeItem(key: string) {
      store.delete(key)
    },
    setItem(key: string, value: string) {
      store.set(key, value)
    },
  }

  Object.defineProperty(window, 'localStorage', {
    value: mockStorage,
    configurable: true,
  })
}

describe('ComparePage', () => {
  beforeEach(() => {
    navigationState.search =
      'runs=run-ppo%2Crun-rloo&metricObjectives=%7B%22loss%22%3A%22higher%22%7D'
    pushMock.mockReset()
    replaceMock.mockReset()
    listRunsMock.mockReset()
    compareRunsMock.mockReset()
    getRunMock.mockReset()
    uPlotChartMock.mockReset()
    ensureLocalStorageMock()
    window.localStorage.clear()

    const runs = {
      'run-rloo': {
        run_id: 'run-rloo',
        project_id: 'project-1',
        name: 'RLOO',
        status: 'finished',
        metrics_count: 2,
        params_count: 0,
        tags: {},
        created_at: '2026-03-01 09:00:00',
        updated_at: '2026-03-01 09:05:00',
        duration_seconds: 300,
      },
      'run-ppo': {
        run_id: 'run-ppo',
        project_id: 'project-1',
        name: 'PPO',
        status: 'finished',
        metrics_count: 2,
        params_count: 0,
        tags: {},
        created_at: '2026-03-01 10:00:00',
        updated_at: '2026-03-01 10:05:00',
        duration_seconds: 240,
      },
    }

    listRunsMock.mockResolvedValue({
      runs: [runs['run-rloo'], runs['run-ppo']],
      total: 2,
      limit: 1000,
      offset: 0,
    })
    getRunMock.mockImplementation(async (runId: keyof typeof runs) => runs[runId])

    compareRunsMock.mockResolvedValue({
      runs: [
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
      ],
      common_metrics: ['loss', 'reward'],
      alignment: 'step',
    })
  })

  it('restores selected runs and metric objectives from URL state without rewriting history', async () => {
    render(<ComparePage />)

    await screen.findByRole('heading', { name: 'Compare Runs' })

    await waitFor(() => {
      expect(listRunsMock).toHaveBeenCalledWith({ limit: 1000 })
      expect(compareRunsMock).toHaveBeenCalledWith(['run-ppo', 'run-rloo'], [], 5000, {
        limit: 200,
        offset: 0,
      })
    })

    const ppoCheckbox = await screen.findByRole('checkbox', { name: /PPO/i })
    const rlooCheckbox = await screen.findByRole('checkbox', { name: /RLOO/i })
    expect((ppoCheckbox as HTMLInputElement).checked).toBe(true)
    expect((rlooCheckbox as HTMLInputElement).checked).toBe(true)

    expect(screen.getByRole('button', { name: 'Clear (2)' })).toBeTruthy()

    const objectiveSelect = (await screen.findByRole('combobox', {
      name: 'Metric Objective',
    })) as HTMLSelectElement
    expect(objectiveSelect.value).toBe('higher')

    expect(pushMock).not.toHaveBeenCalled()
    expect(replaceMock).not.toHaveBeenCalled()
  })
})
