import { test } from 'node:test'
import assert from 'node:assert/strict'
import { LIMITS, changedKeys, isOverridden, limitProblem, savedLimits } from './limits.ts'

const timeout = LIMITS[0]

test('saved values fall back to the default and are clamped into range', () => {
  assert.deepEqual(savedLimits({}), { shellTimeoutSecs: 120, maxActiveSubagents: 8 })
  assert.deepEqual(savedLimits(null), { shellTimeoutSecs: 120, maxActiveSubagents: 8 })
  assert.deepEqual(savedLimits({ shellTimeoutSecs: 9999, maxActiveSubagents: 0 }), { shellTimeoutSecs: 600, maxActiveSubagents: 1 })
  assert.deepEqual(savedLimits({ shellTimeoutSecs: 'soon', maxActiveSubagents: NaN }), { shellTimeoutSecs: 120, maxActiveSubagents: 8 })
})

test('a typed value is refused with the reason, and accepted inside the range', () => {
  assert.equal(limitProblem(timeout, ''), 'Enter a number.')
  assert.equal(limitProblem(timeout, '1.5'), 'Use a whole number.')
  assert.equal(limitProblem(timeout, '4'), 'Between 5 and 600 seconds.')
  assert.equal(limitProblem(timeout, '601'), 'Between 5 and 600 seconds.')
  assert.equal(limitProblem(timeout, '300'), null)
})

test('overridden means different from the default; changed means different from what is saved', () => {
  assert.equal(isOverridden(timeout, 120), false)
  assert.equal(isOverridden(timeout, 300), true)
  const saved = { shellTimeoutSecs: 300, maxActiveSubagents: 8 }
  assert.deepEqual(changedKeys(saved, { shellTimeoutSecs: '300', maxActiveSubagents: '8' }), [])
  assert.deepEqual(changedKeys(saved, { shellTimeoutSecs: '120', maxActiveSubagents: '4' }), ['shellTimeoutSecs', 'maxActiveSubagents'])
})
