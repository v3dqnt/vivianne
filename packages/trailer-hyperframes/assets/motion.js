// Deterministic motion helpers shared by every scene. Loaded as a classic
// <script> in compositions (and imported by the node tests); attaches to
// globalThis.HF. No Math.random / Date.now: renders must be repeatable.
;(() => {
  /** mulberry32: a seeded PRNG in [0, 1). */
  function rng(seed) {
    let a = seed >>> 0
    return () => {
      a = (a + 0x6d2b79f5) >>> 0
      let t = a
      t = Math.imul(t ^ (t >>> 15), t | 1)
      t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296
    }
  }

  /** Types `text` into `el` from `at`, `cps` characters a second. Returns the end time. */
  function typeText(tl, el, text, at, cps = 40) {
    const step = 1 / cps
    for (let i = 1; i <= text.length; i++) tl.set(el, { textContent: text.slice(0, i) }, +(at + i * step).toFixed(4))
    return at + text.length / cps
  }

  /** Centre of `el` relative to `stage`, in the stage's unscaled px. */
  function centerOf(el, stage) {
    const r = el.getBoundingClientRect()
    const s = stage.getBoundingClientRect()
    const k = stage.offsetWidth ? s.width / stage.offsetWidth : 1
    return { x: (r.left - s.left + r.width / 2) / k, y: (r.top - s.top + r.height / 2) / k }
  }

  /** Glides the cursor to the centre of `toEl` (both inside `stage`). Returns the end time. */
  function moveCursor(tl, cursor, toEl, at, dur = 0.6, stage = cursor.parentElement) {
    const { x, y } = centerOf(toEl, stage)
    tl.to(cursor, { x, y, duration: dur, ease: 'power2.inOut' }, at)
    return at + dur
  }

  /** A press: the cursor dips and a ring pulses. Returns the end time. */
  function click(tl, cursor, at) {
    tl.to(cursor, { scale: 0.85, duration: 0.08, ease: 'power1.out' }, at)
    tl.to(cursor, { scale: 1, duration: 0.12, ease: 'power1.out' }, at + 0.08)
    const ring = cursor.querySelector('.cursor-ring')
    if (ring) tl.fromTo(ring, { scale: 0.4, opacity: 0.6 }, { scale: 1.6, opacity: 0, duration: 0.4, ease: 'power2.out' }, at)
    return at + 0.2
  }

  /** Puts captured app state `name` (captures/*.js) into `box`; its CSS goes in once. */
  function mount(box, name) {
    const cap = (globalThis.CAPTURES || {})[name]
    if (!cap) throw new Error(`capture not loaded: ${name}`)
    if (!document.querySelector(`style[data-capture="${name}"]`)) {
      const style = document.createElement('style')
      style.dataset.capture = name
      style.textContent = cap.css
      document.head.appendChild(style)
    }
    box.innerHTML = `<div class="viv-app"><div class="viv-body">${cap.html}</div></div>`
    return box
  }

  globalThis.HF = { rng, typeText, moveCursor, click, centerOf, mount }
})()
