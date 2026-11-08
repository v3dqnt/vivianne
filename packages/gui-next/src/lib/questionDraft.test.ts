import { test } from 'node:test'
import assert from 'node:assert/strict'
import { clearQuestionDraft, loadQuestionDraft, saveQuestionDraft } from './questionDraft.ts'

function memory() {
  const m = new Map<string, string>()
  return { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v), removeItem: (k: string) => void m.delete(k) }
}

test('a saved draft comes back for the same request and is gone once cleared', () => {
  const s = memory()
  saveQuestionDraft('r1', [['A'], ['B', 'C']], s)
  assert.deepEqual(loadQuestionDraft('r1', 2, s), [['A'], ['B', 'C']])
  assert.equal(loadQuestionDraft('r2', 2, s), null)
  clearQuestionDraft('r1', s)
  assert.equal(loadQuestionDraft('r1', 2, s), null)
})

test('a draft that no longer fits the question, or is damaged, is ignored', () => {
  const s = memory()
  saveQuestionDraft('r1', [['A']], s)
  assert.equal(loadQuestionDraft('r1', 3, s), null)
  s.setItem('viv.question.r9', '{not json')
  assert.equal(loadQuestionDraft('r9', 1, s), null)
  s.setItem('viv.question.r8', JSON.stringify([['ok', 5, null]]))
  assert.deepEqual(loadQuestionDraft('r8', 1, s), [['ok']])
})
