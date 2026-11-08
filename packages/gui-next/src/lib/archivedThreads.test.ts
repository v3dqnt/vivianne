import { test } from 'node:test'
import assert from 'node:assert/strict'
import { loadArchived, saveArchived, toggled } from './archivedThreads.ts'

function memory() {
  const m = new Map<string, string>()
  return { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v) }
}

test('archiving adds once and restoring removes', () => {
  assert.deepEqual(toggled(['a'], 'b', true), ['a', 'b'])
  assert.deepEqual(toggled(['a', 'b'], 'b', true), ['a', 'b'])
  assert.deepEqual(toggled(['a', 'b'], 'a', false), ['b'])
  assert.deepEqual(toggled([], 'x', false), [])
})

test('the saved list round-trips and damaged storage reads as empty', () => {
  const s = memory()
  saveArchived(['a', 'b'], s)
  assert.deepEqual(loadArchived(s), ['a', 'b'])
  s.setItem('viv.archivedThreads', '{nope')
  assert.deepEqual(loadArchived(s), [])
  s.setItem('viv.archivedThreads', JSON.stringify(['ok', 5, null]))
  assert.deepEqual(loadArchived(s), ['ok'])
})
