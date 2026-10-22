// Renders the app's own Viv Instant notch (Notch + NotchFace) frame by frame
// into transparent PNGs. The clock is stubbed: requestAnimationFrame (the
// robot head) and every CSS animation and transition (the pill easing to a
// new size, the working wave) are seeked to the frame's time, so each frame
// is exact.
//
//   PPTR=... CHROME=... node render-notch.cjs <outDir>
const puppeteer = require(process.env.PPTR)
const fs = require('fs')
const path = require('path')

const [outDir] = process.argv.slice(2)
const FPS = 30
const SECONDS = Number(process.env.SECS || 5.6)
const DSF = 3

;(async () => {
  fs.mkdirSync(outDir, { recursive: true })
  const browser = await puppeteer.launch({ executablePath: process.env.CHROME, headless: true, args: ['--no-sandbox'] })
  const page = await browser.newPage()
  await page.setViewport({ width: 520, height: 190, deviceScaleFactor: DSF })
  await page.evaluateOnNewDocument(() => {
    window.__t = 0
    window.__raf = []
    performance.now = () => window.__t
    window.requestAnimationFrame = (cb) => {
      window.__raf.push(cb)
      return window.__raf.length
    }
    window.cancelAnimationFrame = () => {}
    window.__step = (ms) => {
      window.__t += ms
      const q = window.__raf
      window.__raf = []
      q.forEach((cb) => cb(window.__t))
      // CSS animations and transitions: each one runs from the frame it appeared on.
      for (const a of document.getAnimations()) {
        if (a.__s === undefined) a.__s = window.__t - ms
        a.pause()
        a.currentTime = window.__t - a.__s
      }
    }
  })
  page.on('console', (m) => m.type() === 'error' && console.log('page:', m.text().slice(0, 300)))
  page.on('pageerror', (e) => console.log('pageerror:', String(e).slice(0, 300)))
  await page.goto('http://localhost:1420/', { waitUntil: 'networkidle0' })
  await page.evaluate(async () => {
    const url = (name) => performance.getEntriesByType('resource').map((e) => e.name).find((n) => n.includes('/.vite/deps/' + name + '?')) || '/node_modules/.vite/deps/' + name
    const R = await import(url('react.js'))
    const React = R.default || R
    const D = await import(url('react-dom_client.js'))
    const createRoot = D.createRoot || D.default.createRoot
    const { Notch } = await import('/src/modules/instant/components/Notch.tsx')
    const { ThemeProvider } = await import('/src/ui/index.ts')
    const { buildTheme } = await import('/src/theme.ts')
    const TS = await import('/src/modules/settings/store/themeStore.ts')
    const ap = TS.useThemeStore.getState()
    const theme = buildTheme('dark', TS.activePaletteId(ap), TS.themeOptions(ap))
    document.body.innerHTML = ''
    document.documentElement.style.background = 'transparent'
    document.body.style.cssText = 'margin:0;background:transparent;width:100vw;height:100vh;overflow:hidden;font-family:Geist,system-ui,sans-serif'
    const host = document.createElement('div')
    document.body.appendChild(host)
    const root = createRoot(host)
    // The reply as the overlay shows it while speaking: the words said so far
    // (InstantOverlay's RevealedText, same rule: whole words, spaces kept).
    const revealed = (text, n) => {
      let seen = 0
      let out = ''
      for (const w of text.split(/(\s+)/)) {
        if (/\S/.test(w)) {
          if (seen >= n) break
          seen++
        }
        out += w
      }
      return out
    }
    const ASK = 'What are my agents doing?'
    const ANSWER = 'Three agents are working. On Vivianne, the lead is fixing the build.'
    window.__show = (t) => {
      let props
      if (t < 1.7) {
        // Listening: what it hears, word by word, and the mic level.
        const heard = revealed(ASK, Math.floor(Math.max(0, t - 0.35) * 5))
        props = { tone: 'listening', face: 'listening', title: 'Listening', detail: heard || 'Speak while holding, release to send', level: 0.07 + 0.05 * Math.abs(Math.sin(t * 9)) }
      } else if (t < 2.5) {
        props = { tone: 'working', face: 'thinking', title: 'Checking', detail: 'Reading the status of 3 projects' }
      } else {
        const n = Math.floor((t - 2.5) * 4.2)
        const done = n >= ANSWER.split(/\s+/).length + 2
        props = { tone: 'done', face: done ? 'happy' : 'speaking', title: 'Done', body: React.createElement('div', { style: { whiteSpace: 'pre-wrap' } }, revealed(ANSWER, n) || ' ') }
      }
      root.render(React.createElement(ThemeProvider, { theme }, React.createElement(Notch, { ...props, resizeCommand: 'cmd_noop' })))
    }
    window.__show(0)
  })
  // Let fonts and the lazy robot-heads module load.
  for (let i = 0; i < 30; i++) {
    await page.evaluate(() => window.__step(0.001))
    await new Promise((r) => setTimeout(r, 60))
  }
  await page.evaluate(() => {
    window.__t = 0
    for (const a of document.getAnimations()) a.__s = 0
  })
  const N = Math.round(SECONDS * FPS)
  for (let n = 0; n < N; n++) {
    await page.evaluate((t) => window.__show(t), n / FPS)
    // React commits, then the ResizeObserver sets the pill's new size.
    await new Promise((r) => setTimeout(r, 30))
    await page.evaluate(() => window.__step(1000 / 30))
    await page.screenshot({ path: path.join(outDir, `n${String(n).padStart(4, '0')}.png`), omitBackground: true })
  }
  console.log('frames', N, await page.evaluate(() => document.body.innerHTML.length + ' ' + document.body.innerHTML.slice(0, 300)))
  await browser.close()
})().catch((e) => {
  console.error(e)
  process.exit(1)
})
