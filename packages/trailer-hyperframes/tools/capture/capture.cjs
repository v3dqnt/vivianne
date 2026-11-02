// Captures real Vivianne GUI states as static markup + CSS for the trailer.
//
//   node tools/capture/capture.cjs <scenario>      (gui-next dev server on :5199)
//
// A scenario (tools/capture/scenarios/<name>.cjs) exports
//   { init: [scripts injected before load], states: [{ name, run: async (page) => {} }] }
// Each state is captured after its `run`, in order, into captures/<scenario>.js
// as window.CAPTURES[<state name>] = { css, html }. The CSS is the page's
// stylesheet with html/body/#root rewritten to .viv-app so it cannot leak.
const { chromium } = require('playwright')
const fs = require('fs')
const path = require('path')

const ROOT = path.join(__dirname, '../..')
const APP = process.env.APP_URL || 'http://localhost:5199/'
const W = 1600
const H = 900

async function snapshot(page) {
  return page.evaluate(() => {
    const scope = (sel) =>
      sel
        .split(',')
        .map((s) => s.trim().replace(/^(html|:root)(?=$|[\s>:.[])/, '.viv-app').replace(/^body(?=$|[\s>:.[])/, '.viv-body'))
        .join(', ')
    const out = []
    for (const sheet of document.styleSheets) {
      let rules
      try {
        rules = sheet.cssRules
      } catch {
        continue
      }
      for (const r of rules) {
        if (r.type === CSSRule.FONT_FACE_RULE) out.push(r.cssText.replace(/url\("?[^")]*\/([^/")]+\.woff2)"?\)/g, 'url("assets/fonts/$1")'))
        else if (r.type === CSSRule.STYLE_RULE) out.push(`${scope(r.selectorText)} { ${r.style.cssText} }`)
        else if (r.type === CSSRule.KEYFRAMES_RULE) out.push(r.cssText)
        else if (r.type === CSSRule.MEDIA_RULE && !/prefers-reduced-motion/.test(r.conditionText)) {
          const inner = [...r.cssRules].filter((x) => x.selectorText).map((x) => `${scope(x.selectorText)} { ${x.style.cssText} }`)
          if (inner.length) out.push(`@media ${r.conditionText} { ${inner.join(' ')} }`)
        }
      }
    }
    // A collapsed dock group (dockview keeps it ~0px tall) still holds its
    // panel, overflowing under the next pane; it never shows, so drop it.
    document.querySelectorAll('[data-cap-drop]').forEach((n) => n.removeAttribute('data-cap-drop'))
    for (const el of document.body.querySelectorAll('.dv-groupview')) {
      const r = el.getBoundingClientRect()
      if (r.width < 4 || r.height < 4) el.setAttribute('data-cap-drop', '')
    }
    // Tag chat transcripts (bot chat: max-width 820px; code chat: 880px) and
    // their items, so scenes can find "the newest message" without class hashes.
    document.querySelectorAll('[data-cap-list]').forEach((n) => n.removeAttribute('data-cap-list'))
    for (const el of document.body.querySelectorAll('div')) {
      const mw = getComputedStyle(el).maxWidth
      if ((mw === '820px' || mw === '880px') && el.children.length && el.closest('[class*="dv-"]')) {
        el.setAttribute('data-cap-list', '')
        ;[...el.children].forEach((c, i) => c.setAttribute('data-cap-item', String(i)))
      }
    }
    const body = document.body.cloneNode(true)
    body.querySelectorAll('[data-cap-drop]').forEach((n) => n.replaceChildren())
    body.querySelectorAll('script, noscript, iframe').forEach((n) => n.remove())
    // Inputs keep their typed value only as a property; make it markup.
    document.querySelectorAll('textarea, input').forEach((el, i) => {
      const twin = body.querySelectorAll('textarea, input')[i]
      if (!twin) return
      if (el.tagName === 'TEXTAREA') twin.textContent = el.value
      else twin.setAttribute('value', el.value)
    })
    // The app's root font-size is the rem base; the trailer's page differs, so pin it.
    const base = parseFloat(getComputedStyle(document.documentElement).fontSize)
    const px = (t) => t.replace(/(-?[\d.]+)rem\b/g, (_, n) => `${+(n * base).toFixed(3)}px`)
    return { css: px([...new Set(out)].join('\n')), html: px(body.innerHTML) }
  })
}

;(async () => {
  const name = process.argv[2]
  if (!name) throw new Error('usage: capture.cjs <scenario>')
  // Copy lives in assets/story.js (window.STORY); scenarios receive it.
  const sandbox = { window: {} }
  require('vm').runInNewContext(fs.readFileSync(path.join(ROOT, 'assets/story.js'), 'utf8'), sandbox)
  const mod = require(path.join(__dirname, 'scenarios', `${name}.cjs`))
  const scenario = typeof mod === 'function' ? mod(sandbox.window.STORY) : mod
  const browser = await chromium.launch()
  const page = await browser.newPage({ viewport: { width: W, height: H }, reducedMotion: 'reduce' })
  page.on('pageerror', (e) => console.log('pageerror:', e.message))
  await page.addInitScript({ path: path.join(__dirname, 'mock-ipc.js') })
  await page.addInitScript({ path: path.join(__dirname, 'demo-base.js') })
  for (const s of scenario.init || []) await page.addInitScript(s)
  await page.goto(APP, { waitUntil: 'networkidle' })
  await page.waitForTimeout(1500)
  const captures = {}
  fs.mkdirSync(path.join(ROOT, 'captures/ref'), { recursive: true })
  for (const state of scenario.states) {
    await state.run(page)
    await page.waitForTimeout(400)
    captures[state.name] = await snapshot(page)
    await page.screenshot({ path: path.join(ROOT, `captures/ref/${state.name}.png`) })
    console.log('captured', state.name)
  }
  if (process.env.IPC_LOG) console.log(await page.evaluate(() => [...new Set(window.__IPC_LOG__)].join(' ')))
  const js = `// Generated by tools/capture/capture.cjs (${name}). Do not edit; re-run the capture.\nwindow.CAPTURES = window.CAPTURES || {};\n${Object.entries(
    captures,
  )
    .map(([k, v]) => `window.CAPTURES[${JSON.stringify(k)}] = ${JSON.stringify(v)};`)
    .join('\n')}\n`
  fs.writeFileSync(path.join(ROOT, `captures/${name}.js`), js)
  await browser.close()
})().catch((e) => {
  console.error(e)
  process.exit(1)
})
