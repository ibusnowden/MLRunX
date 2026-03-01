import { describe, expect, it } from 'vitest'

import {
  normalizeMetricObjectiveOverrides,
  parseMetricObjectiveOverridesParam,
  serializeMetricObjectiveOverridesParam,
} from './compareObjectives'

describe('compareObjectives', () => {
  it('normalizes and sorts objective overrides', () => {
    expect(
      normalizeMetricObjectiveOverrides({
        reward: 'higher',
        ' loss ': 'lower',
      })
    ).toEqual({
      loss: 'lower',
      reward: 'higher',
    })
  })

  it('parses valid JSON objective payloads', () => {
    expect(parseMetricObjectiveOverridesParam('{"reward":"higher","loss":"lower"}')).toEqual({
      loss: 'lower',
      reward: 'higher',
    })
  })

  it('returns empty objectives for invalid payloads', () => {
    expect(parseMetricObjectiveOverridesParam('{bad')).toEqual({})
    expect(parseMetricObjectiveOverridesParam('["loss","lower"]')).toEqual({})
    expect(parseMetricObjectiveOverridesParam(null)).toEqual({})
  })

  it('serializes normalized objectives and omits empty values', () => {
    expect(serializeMetricObjectiveOverridesParam({})).toBeNull()
    expect(
      serializeMetricObjectiveOverridesParam({
        reward: 'higher',
        loss: 'lower',
      })
    ).toBe('{"loss":"lower","reward":"higher"}')
  })
})
