import { beforeEach, describe, expect, it } from 'vitest'
import {
  deleteComparePreset,
  deleteRunFilterPreset,
  loadComparePresets,
  loadRunFilterPresets,
  normalizeRunIdSet,
  saveComparePresets,
  saveRunFilterPresets,
  upsertComparePreset,
  upsertRunFilterPreset,
} from './presets'

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

describe('presets', () => {
  beforeEach(() => {
    ensureLocalStorageMock()
    window.localStorage.clear()
  })

  it('round-trips run filter presets through storage', () => {
    const result = upsertRunFilterPreset([], 'Loss Runs', 'status:running tag:loss=true')
    saveRunFilterPresets(result.presets)

    const loaded = loadRunFilterPresets()
    expect(loaded).toHaveLength(1)
    expect(loaded[0].name).toBe('Loss Runs')
    expect(loaded[0].query).toBe('status:running tag:loss=true')
  })

  it('updates run filter preset by name and can delete by id', () => {
    const first = upsertRunFilterPreset([], 'Morning Sweep', 'status:running')
    const second = upsertRunFilterPreset(first.presets, 'morning sweep', 'status:finished')

    expect(second.presets).toHaveLength(1)
    expect(second.presets[0].query).toBe('status:finished')

    const afterDelete = deleteRunFilterPreset(second.presets, second.saved.id)
    expect(afterDelete).toHaveLength(0)
  })

  it('normalizes run id sets for compare presets', () => {
    expect(normalizeRunIdSet(['run-b', 'run-a', 'run-b', ''])).toEqual(['run-b', 'run-a'])
  })

  it('round-trips compare presets and supports delete', () => {
    const first = upsertComparePreset(
      [],
      'A/B',
      ['run-b', 'run-a'],
      { reward: 'higher', loss: 'lower' }
    )
    saveComparePresets(first.presets)

    const loaded = loadComparePresets()
    expect(loaded).toHaveLength(1)
    expect(loaded[0].runIds).toEqual(['run-b', 'run-a'])
    expect(loaded[0].metricObjectives).toEqual({ loss: 'lower', reward: 'higher' })

    const afterDelete = deleteComparePreset(loaded, loaded[0].id)
    expect(afterDelete).toHaveLength(0)
  })

  it('updates compare preset by name and clears objective overrides when omitted', () => {
    const first = upsertComparePreset([], 'A/B', ['run-a', 'run-b'], { loss: 'lower' })
    const second = upsertComparePreset(first.presets, 'a/b', ['run-c', 'run-d'])

    expect(second.presets).toHaveLength(1)
    expect(second.presets[0].runIds).toEqual(['run-c', 'run-d'])
    expect(second.presets[0].metricObjectives).toBeUndefined()
  })

  it('returns empty lists when stored payload is invalid JSON', () => {
    window.localStorage.setItem('mlrunx_run_filter_presets_v1', 'not-json')
    window.localStorage.setItem('mlrunx_compare_presets_v1', '{bad')

    expect(loadRunFilterPresets()).toEqual([])
    expect(loadComparePresets()).toEqual([])
  })
})
