import { test } from 'node:test'
import assert from 'node:assert/strict'
import '../assets/motion.js'
const { rng, typeText } = globalThis.HF

const fakeTl = () => {
  const calls = []
  return { calls, set: (el, vars, at) => (calls.push({ el, vars, at }), fakeTl) }
}

test('rng is deterministic per seed', () => {
  const a = rng(42), b = rng(42), c = rng(7)
  const xs = [a(), a(), a()]
  assert.deepEqual(xs, [b(), b(), b()])
  assert.notDeepEqual(xs, [c(), c(), c()])
  for (const x of xs) assert.ok(x >= 0 && x < 1)
})

test('typeText sets one frame per character and returns the end time', () => {
  const tl = fakeTl()
  const el = {}
  const end = typeText(tl, el, 'hello', 2, 10)
  assert.equal(end, 2.5)
  assert.equal(tl.calls.length, 5)
  assert.deepEqual(tl.calls.map((c) => c.vars.textContent), ['h', 'he', 'hel', 'hell', 'hello'])
  assert.equal(tl.calls[0].at, 2.1)
})

test('mount injects a capture once and wraps it in the app scope', () => {
  const appended = []
  globalThis.document = {
    head: { appendChild: (n) => appended.push(n) },
    createElement: (tag) => ({ tag, dataset: {}, textContent: '' }),
    querySelector: (sel) => appended.find((n) => sel === `style[data-capture="${n.dataset.capture}"]`) ?? null,
  }
  globalThis.CAPTURES = { home: { css: '.a{}', html: '<p>hi</p>' } }
  const box = { innerHTML: '' }
  globalThis.HF.mount(box, 'home')
  globalThis.HF.mount({ innerHTML: '' }, 'home')
  assert.equal(appended.length, 1)
  assert.equal(appended[0].textContent, '.a{}')
  assert.equal(box.innerHTML, '<div class="viv-app"><div class="viv-body"><p>hi</p></div></div>')
  assert.throws(() => globalThis.HF.mount(box, 'nope'), /nope/)
})
