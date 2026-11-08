import { test } from 'node:test'
import assert from 'node:assert/strict'
import { MAX_PROGRESS_LINES, phaseName, withProgress } from './scriptProgress.ts'

test('lines are added only for the call they belong to', () => {
  assert.deepEqual(withProgress([], { callId: 'a', text: 'one' }, 'a'), ['one'])
  assert.deepEqual(withProgress(['one'], { callId: 'b', text: 'two' }, 'a'), ['one'])
  assert.deepEqual(withProgress(['one'], { callId: 'a', text: '' }, 'a'), ['one'])
  assert.deepEqual(withProgress(['one'], null, 'a'), ['one'])
})

test('only the newest lines are kept', () => {
  let lines: string[] = []
  for (let i = 0; i < MAX_PROGRESS_LINES + 25; i++) lines = withProgress(lines, { callId: 'a', text: `l${i}` }, 'a')
  assert.equal(lines.length, MAX_PROGRESS_LINES)
  assert.equal(lines[0], 'l25')
  assert.equal(lines.at(-1), `l${MAX_PROGRESS_LINES + 24}`)
})

test('phase markers are recognised', () => {
  assert.equal(phaseName('== scan =='), 'scan')
  assert.equal(phaseName('== two words =='), 'two words')
  assert.equal(phaseName('plain'), null)
})
