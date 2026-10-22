// Renders the app's own robot head (AgentFace → robot-heads, a canvas
// animation on its own clock) frame by frame into PNGs, with the clock
// stubbed so every frame is exact. ffmpeg then makes the clip.
//
//   PPTR=... CHROME=... node render-head.cjs <outDir> <size> <color> <shape> "<state>:<seconds>,..."
const puppeteer = require(process.env.PPTR)
const fs = require('fs')
const path = require('path')

const [outDir, sizeArg, color, shape, plan] = process.argv.slice(2)
const SIZE = Number(sizeArg)
const FPS = 30
const segments = plan.split(',').map((p) => {
  const [state, secs] = p.split(':')
  return { state, frames: Math.round(Number(secs) * FPS) }
})

;(async () => {
  fs.mkdirSync(outDir, { recursive: true })
  const browser = await puppeteer.launch({ executablePath: process.env.CHROME, headless: true, args: ['--no-sandbox'] })
  const page = await browser.newPage()
  const BOX = Math.round(SIZE * 1.9)
  await page.setViewport({ width: BOX, height: BOX, deviceScaleFactor: 1 })
  // A manual clock: performance.now and requestAnimationFrame answer to us.
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
    }
  })
  await page.goto('http://localhost:1420/', { waitUntil: 'networkidle0' })
  await page.evaluate(async (SIZE, color, shape) => {
    // The same module instances the app uses (Vite adds a version query).
    const url = (name) => performance.getEntriesByType('resource').map((e) => e.name).find((n) => n.includes('/.vite/deps/' + name + '?')) || '/node_modules/.vite/deps/' + name
    const R = await import(url('react.js'))
    const React = R.default || R
    const D = await import(url('react-dom_client.js'))
    const createRoot = D.createRoot || D.default.createRoot
    const { AgentFace } = await import('/src/modules/team/components/AgentFace.tsx')
    document.body.innerHTML = ''
    document.documentElement.style.background = 'transparent'
    document.body.style.cssText = 'margin:0;background:transparent;display:grid;place-items:center;width:100vw;height:100vh;overflow:hidden'
    const host = document.createElement('div')
    document.body.appendChild(host)
    const root = createRoot(host)
    window.__show = (state) =>
      root.render(React.createElement(AgentFace, { name: 'Release', color, size: SIZE, state, animate: true, face: { shape } }))
    window.__show('idle')
  }, SIZE, color, shape)
  // Let the lazy robot-heads module load and draw.
  for (let i = 0; i < 40; i++) {
    await page.evaluate(() => window.__step(1000 / 30))
    await new Promise((r) => setTimeout(r, 50))
  }
  let n = 0
  for (const seg of segments) {
    await page.evaluate((s) => window.__show(s), seg.state)
    await new Promise((r) => setTimeout(r, 60))
    for (let f = 0; f < seg.frames; f++) {
      await page.evaluate(() => window.__step(1000 / 30))
      await page.screenshot({ path: path.join(outDir, `f${String(n).padStart(4, '0')}.png`), omitBackground: true })
      n++
    }
  }
  console.log('frames', n)
  await browser.close()
})().catch((e) => {
  console.error(e)
  process.exit(1)
})
