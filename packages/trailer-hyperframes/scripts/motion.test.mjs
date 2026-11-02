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
    querySelectorAll: (sel) => (sel === 'style[data-capture]' ? appended : []),
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

test('focusXY centres an app point at the given zoom', () => {
  // app (800, 450) is stage (960, 540): already centred, so no shift.
  assert.deepEqual(globalThis.HF.focusXY({ x: 800, y: 450 }, 2), { x: 960 - 2 * 960, y: 540 - 2 * 540 })
})

test('focusXY never shows past the frame edge', () => {
  const top = globalThis.HF.focusXY({ x: 0, y: 0 }, 1.8)
  assert.deepEqual(top, { x: 0, y: 0 })
  const bottom = globalThis.HF.focusXY({ x: 1600, y: 900 }, 1.8)
  assert.deepEqual(bottom, { x: 1920 - 1920 * 1.8, y: 1080 - 1080 * 1.8 })
})

test('focus at zoom 1 is the full frame', () => {
  assert.deepEqual(globalThis.HF.focusXY({ x: 300, y: 700 }, 1), { x: 0, y: 0 })
})

test('at() uses the rect recorded at capture time (real fonts) when present', () => {
  const el = { getAttribute: (k) => (k === 'data-cap-rect' ? '100,200,40,20' : null) }
  assert.deepEqual(globalThis.HF.at(null, el), { x: 120, y: 210 })
})
