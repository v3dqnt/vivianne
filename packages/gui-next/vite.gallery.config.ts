import { mergeConfig } from 'vite'
import base from './vite.config.ts'

// The component gallery: `npm run gallery`, then open http://localhost:5344/gallery.html.
// Same plugins and aliases as the app, on its own port; never part of the release build.
// optimizeDeps.entries: scan every source file up front, or Vite finds new dependencies as components load and reloads the page under you.
export default mergeConfig(base, {
  server: { port: 5344, strictPort: true, open: '/gallery.html' },
  optimizeDeps: { entries: ['gallery.html', 'src/**/*.tsx'] },
})
