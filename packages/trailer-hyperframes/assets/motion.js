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
    const loaded = [...document.querySelectorAll('style[data-capture]')].some((n) => n.dataset.capture === name)
    if (!loaded) {
      const style = document.createElement('style')
      style.dataset.capture = name
      style.textContent = cap.css
      document.head.appendChild(style)
    }
    box.innerHTML = `<div class="viv-app"><div class="viv-body">${cap.html}</div></div>`
    return box
  }

  /** Camera offset (transform-origin 0 0 on a 1920x1080 frame) that centres
   * app point `p` (1600x900 app px) at `scale`, clamped so the frame edge
   * never shows. */
  function focusXY(p, scale) {
    const clamp = (v, lo) => Math.min(0, Math.max(lo, v))
    return {
      x: clamp(960 - scale * p.x * 1.2, 1920 - 1920 * scale),
      y: clamp(540 - scale * p.y * 1.2, 1080 - 1080 * scale),
    }
  }

  /** Camera move to app point `p` at `scale` (scale 1 = the whole frame). */
  function focus(tl, cam, p, at, scale = 1.6, dur = 0.6, ease = 'power3.inOut') {
    tl.to(cam, { ...focusXY(p, scale), scale, duration: dur, ease }, at)
    return at + dur
  }

  /** Centre of `el` in app px (1600x900). Prefers the rect the capture tool
   * recorded with the app's real fonts (data-cap-rect="x,y,w,h"); otherwise
   * measures with `layer` (a mounted capture) briefly shown. */
  function at(layer, el) {
    const rec = el.getAttribute && el.getAttribute('data-cap-rect')
    if (rec) {
      const [x, y, w, h] = rec.split(',').map(Number)
      return { x: x + w / 2, y: y + h / 2 }
    }
    const was = layer.style.display
    layer.style.display = 'block'
    const r = el.getBoundingClientRect(), s = layer.getBoundingClientRect(), k = s.width / 1600
    layer.style.display = was
    return { x: (r.left - s.left + r.width / 2) / k, y: (r.top - s.top + r.height / 2) / k }
  }

  globalThis.HF = { rng, typeText, moveCursor, click, centerOf, mount, focusXY, focus, at }
})()
