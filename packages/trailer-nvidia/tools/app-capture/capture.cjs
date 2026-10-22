// Render the REAL gui-next app (dev server) in light mode with seeded store
// state, then screenshot and dump its DOM + CSS so the trailer can reuse the
// app's own rendered output.
//
// usage: node capture.cjs <state.js> <outName>
const path = require('path')
const fs = require('fs')
const puppeteer = require(process.env.PPTR)

const CHROME = process.env.CHROME
const [stateFile, outName] = process.argv.slice(2)
const OUT = path.join(__dirname, 'out')
fs.mkdirSync(OUT, { recursive: true })

;(async () => {
  const browser = await puppeteer.launch({ executablePath: CHROME, headless: true, args: ['--no-sandbox'] })
  const page = await browser.newPage()
  await page.setViewport({ width: 1920, height: 1080, deviceScaleFactor: 1 })
  page.on('console', (m) => { if (m.type() === 'error') console.log('[page error]', m.text().slice(0, 200)) })
  page.on('pageerror', (e) => console.log('[pageerror]', String(e).slice(0, 300)))
  await page.evaluateOnNewDocument(() => {
    // Light theme, the default palette; the NVIDIA edition's first-run card and
    // its one-time theme switch are marked done so neither covers the capture.
    localStorage.setItem('viv.appearance', JSON.stringify({ modePref: 'light', paletteId: 'terax' }))
    localStorage.setItem('viv.theme.terax', 'true')
    localStorage.setItem('viv.hackathon.themed', 'true')
    localStorage.setItem('viv.hackathon', JSON.stringify({ done: true }))
    // A stub Tauri backend: every command answers with nothing, so the app
    // takes its real desktop code path (title bar, chat) with no Rust behind it.
    let cb = 0
    window.__TAURI_INTERNALS__ = {
      // A seed can answer particular commands through window.__STUB.
      invoke: async (cmd, args) => {
        const v = window.__STUB && cmd in window.__STUB ? window.__STUB[cmd] : null
        return typeof v === 'function' ? v(args) : v
      },
      transformCallback: () => ++cb,
      unregisterCallback: () => {},
      convertFileSrc: (p) => p,
      metadata: { currentWindow: { label: 'main' }, currentWebview: { windowLabel: 'main', label: 'main' } },
    }
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} }
  })
  await page.goto('http://localhost:1420/', { waitUntil: 'networkidle0', timeout: 60000 })
  await new Promise((r) => setTimeout(r, 800))

  let seed = fs.readFileSync(stateFile, 'utf8')
  // Includes may include others (trailer-common.js includes common.js).
  for (let pass = 0; pass < 5 && /^\/\/@include /m.test(seed); pass++) {
    seed = seed.replace(/^\/\/@include (.+)$/gm, (_, f) => fs.readFileSync(path.join(path.dirname(path.resolve(stateFile)), f.trim()), 'utf8'))
  }
  const result = await page.evaluate(async (seedSrc) => {
    // The app's own module instances (Vite may have stamped ?t= on them).
    const urls = performance.getEntriesByType('resource').map((e) => e.name)
    const mod = async (p) => {
      const hit = urls.filter((u) => u.includes(p)).sort((a, b) => b.length - a.length)[0]
      return import(hit || p)
    }
    const fn = new Function('mod', `return (async () => { ${seedSrc} })()`)
    try {
      return await fn(mod)
    } catch (e) {
      return 'seed failed: ' + (e && e.stack ? e.stack : e)
    }
  }, seed)
  console.log('seed:', result)
  await new Promise((r) => setTimeout(r, 2500))

  // Drop the browser-preview banner: it only exists without the Tauri backend.
  await page.evaluate(() => {
    for (const el of document.querySelectorAll('div, p')) {
      if (el.childElementCount < 4 && /Running in a plain browser/.test(el.textContent || '') && el.textContent.length < 200) {
        const box = el.closest('[class*="ui-Alert"]') || el
        box.remove()
        break
      }
    }
  })

  await page.screenshot({ path: path.join(OUT, outName + '.png') })

  // A <canvas> (three.js, charts) has no DOM to dump: photograph it instead.
  // FLIP=n takes n frames a beat apart so a turning scene becomes a flipbook
  // (`.viv-flip > img[data-f]`); the trailer shows one frame at a time.
  const FLIP = Number(process.env.FLIP || 1)
  // Tag them first: replacing one shifts every later index.
  await page.evaluate(() => document.querySelectorAll('canvas').forEach((c, i) => (c.dataset.cap = String(i))))
  const canvases = await page.$$('canvas')
  for (let ci = 0; ci < canvases.length; ci++) {
    const c = canvases[ci]
    const box = await c.boundingBox()
    if (!box || box.width < 12 || box.height < 12) continue
    const frames = []
    for (let f = 0; f < FLIP; f++) {
      // Clip the page to the canvas's box: the element handle goes stale if React redraws it.
      const r = await page.evaluate((i) => {
        const e = document.querySelector(`canvas[data-cap="${i}"]`)
        const b = e && e.getBoundingClientRect()
        return b ? { x: b.x, y: b.y, width: b.width, height: b.height } : null
      }, ci)
      frames.push((await page.screenshot({ type: 'png', omitBackground: true, encoding: 'base64', clip: r || box })).toString())
      if (f < FLIP - 1) await new Promise((r) => setTimeout(r, 90))
    }
    await page.evaluate(
      (i, frames) => {
        const el = document.querySelector(`canvas[data-cap="${i}"]`)
        if (!el) return
        const w = el.clientWidth
        const h = el.clientHeight
        const box = document.createElement('div')
        box.className = 'viv-flip'
        box.style.cssText = `position:relative;width:${w}px;height:${h}px`
        frames.forEach((b64, f) => {
          const img = document.createElement('img')
          img.src = 'data:image/png;base64,' + b64
          img.dataset.f = String(f)
          img.style.cssText = `position:absolute;inset:0;width:${w}px;height:${h}px;opacity:${f === 0 ? 1 : 0}`
          box.appendChild(img)
        })
        el.replaceWith(box)
      },
      ci,
      frames,
    )
  }
  const dump = await page.evaluate(() => {
    const css = []
    for (const sheet of document.styleSheets) {
      try {
        for (const r of sheet.cssRules) css.push(r.cssText)
      } catch {}
    }
    // Dialogs and popovers render in portals beside #root: fold them in.
    const root = document.getElementById('root')
    const portals = [...document.body.children]
      .filter((el) => el !== root && el.tagName !== 'SCRIPT')
      .map((el) => el.outerHTML)
      .join('')
    return { html: root.outerHTML.replace(/<\/div>$/, portals + '</div>'), css: css.join('\n') }
  })
  fs.writeFileSync(path.join(OUT, outName + '.html'), dump.html)
  fs.writeFileSync(path.join(OUT, outName + '.css'), dump.css)
  console.log('wrote', outName, dump.html.length, 'html chars', dump.css.length, 'css chars')
  await browser.close()
})()
