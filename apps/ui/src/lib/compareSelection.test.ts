import { beforeEach, describe, expect, it } from 'vitest'
import {
  clearCompareSelection,
  getCompareRunIds,
  getCompareSelectionSnapshot,
  getCompareUrl,
  setCompareBaseline,
  setCompareCandidate,
  startCompareWithRun,
  swapCompareSelection,
} from './compareSelection'

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

describe('compareSelection', () => {
  beforeEach(() => {
    ensureLocalStorageMock()
    window.localStorage.clear()
    clearCompareSelection()
  })

  it('tracks baseline/candidate and keeps order for compare URL', () => {
    setCompareBaseline('run-a')
    setCompareCandidate('run-b')
    const selection = getCompareSelectionSnapshot()
    expect(selection).toEqual({
      baselineRunId: 'run-a',
      candidateRunId: 'run-b',
    })
    expect(getCompareRunIds(selection)).toEqual(['run-a', 'run-b'])
    expect(getCompareUrl(selection)).toBe('/compare?runs=run-a%2Crun-b')
  })

  it('startCompareWithRun prefers baseline pairing when baseline exists', () => {
    setCompareBaseline('run-base')
    const next = startCompareWithRun('run-new')
    expect(next).toEqual({
      baselineRunId: 'run-base',
      candidateRunId: 'run-new',
    })
  })

  it('can swap and clear selection', () => {
    setCompareBaseline('run-a')
    setCompareCandidate('run-b')
    const swapped = swapCompareSelection()
    expect(swapped).toEqual({
      baselineRunId: 'run-b',
      candidateRunId: 'run-a',
    })
    const cleared = clearCompareSelection()
    expect(cleared).toEqual({
      baselineRunId: '',
      candidateRunId: '',
    })
  })
})
