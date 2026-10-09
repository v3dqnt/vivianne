import { createRequire } from 'node:module'
import { dirname, join, resolve } from 'node:path'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

// Radix keeps some state at module level: the stack of open layers (which
// decides who handles Escape and outside clicks, and owns the
// `pointer-events: none` lock on <body> while a modal is open), the stack of
// focus traps, and the focus-guard count. npm installs a separate copy of
// each under every primitive, and separate copies mean separate stacks — a
// Select inside a Dialog then closes both on one Escape and leaves <body>
// unclickable. Every import is pointed at one copy.
const requireFromRadix = createRequire(createRequire(import.meta.url).resolve('radix-ui/package.json'))
const RADIX_SINGLETONS = ['react-dismissable-layer', 'react-focus-scope', 'react-focus-guards', 'react-portal', 'react-presence']
const radixAliases = RADIX_SINGLETONS.map((name) => ({
  find: new RegExp(`^@radix-ui/${name}$`),
  // `resolve` picks the CommonJS build; the ESM one sits beside it.
  replacement: join(dirname(requireFromRadix.resolve(`@radix-ui/${name}`)), 'index.mjs'),
}))

// Tauri-oriented config. In production the desktop shell loads `dist/`
// directly as static files (there is no server), and in dev it points at this
// server's fixed port — so the port must not drift, and the browser must not
// pop open on start since the real window is the Tauri one.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    // This is a workspace monorepo, so React exists at several versions at
    // once (root hoists one for `packages/gui`, this package installed its
    // own, `trailer` has a third). Without pinning, libraries and the app resolve
    // *different* React copies and every hook throws "Invalid hook call" at
    // runtime — while the build still passes clean, which makes it a nasty
    // one to chase. Dedupe forces exactly one copy into the graph.
    dedupe: ['react', 'react-dom'],
    alias: radixAliases,
  },
  // Relative base: Tauri serves built assets from a non-root origin, where
  // absolute "/assets/..." paths resolve wrong and the app loads blank.
  base: './',
  server: {
    port: 1420,
    strictPort: true,
    open: false,
  },
  build: {
    outDir: 'dist',
    // The oldest web engines Vivianne runs in: macOS 10.15's WKWebView tops
    // out at Safari 15.6's WebKit, and Windows' WebView2 is evergreen
    // Chromium. Anything newer than these is lowered at build time
    // (top-level await, class static blocks, `??=`; nesting and colour
    // functions in CSS).
    target: ['safari15', 'chrome105'],
    cssTarget: ['safari15', 'chrome105'],
    rollupOptions: {
      // Two entry points, because each Tauri window is its own renderer that
      // executes its entry's whole static import graph before anything decides
      // what to draw. The overlays used to share `index.html` and so loaded the
      // entire app — including the CodeMirror IDE — to paint a 260x44 pill.
      input: {
        main: resolve(__dirname, 'index.html'),
        overlay: resolve(__dirname, 'overlay.html'),
      },
    },
    // Sourcemaps in dev builds only.
    //
    // They cost nothing at RUNTIME — nothing loads a `.map` unless DevTools is
    // open — but they are 39.7 MB against 17.2 MB of actual JS, so shipping
    // them roughly triples the frontend payload baked into the installer. In a
    // dev build that trade is clearly worth it; in a release build nobody can
    // open DevTools to use them anyway.
    sourcemap: process.env.NODE_ENV !== 'production',
  },
})
