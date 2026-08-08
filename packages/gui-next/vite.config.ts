import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

// Tauri-oriented config. In production the desktop shell loads `dist/`
// directly as static files (there is no server), and in dev it points at this
// server's fixed port — so the port must not drift, and the browser must not
// pop open on start since the real window is the Tauri one.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    // This is a workspace monorepo, so React exists at several versions at
    // once (root hoists one for `packages/gui`, this package installed its
    // own, `trailer` has a third). Without pinning, MUI and the app resolve
    // *different* React copies and every hook throws "Invalid hook call" at
    // runtime — while the build still passes clean, which makes it a nasty
    // one to chase. Dedupe forces exactly one copy into the graph.
    dedupe: ['react', 'react-dom'],
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
    target: 'esnext',
    // The bundle ships inside the binary and is never fetched over a network,
    // so keeping sourcemaps costs nothing users feel and makes the port's
    // inevitable debugging far cheaper.
    sourcemap: true,
  },
})
