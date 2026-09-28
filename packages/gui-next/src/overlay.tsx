/**
 * Entry point for the small always-on-top overlay windows.
 *
 * ## Why these do not share `main.tsx`
 *
 * Every Tauri window gets its own WebView2 renderer process, and every renderer
 * executes its entry module's *entire* static import graph before any component
 * runs. When the overlays pointed at `index.html`, the label check inside
 * `App.tsx` could not save them: by the time it ran, that renderer had already
 * downloaded, parsed and JIT-compiled the whole application — chat, the
 * sidebar, settings, and the CodeMirror IDE — roughly 1.5 MB of JavaScript, of
 * which ~900 KB is the editor alone.
 *
 * Three windows paid that: a 260x44 pill, a 120x120 pointer, and the DIP
 * overlay. This entry exists so they load only what they actually draw.
 *
 * It also deliberately does NOT start the provider dispatcher. That is main's
 * job and main's alone — see the note in `main.tsx` about concurrent windows
 * answering the same request.
 */
import { StrictMode, useMemo } from 'react'
import { createRoot } from 'react-dom/client'
import { CssBaseline, ThemeProvider } from './ui'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { baselineExtras, buildTheme } from './theme'
import { useThemeStore } from './modules/settings/store/themeStore'
import { hasTauri } from './ipc'
import { DipOverlay } from './modules/dip/components/DipOverlay'
import { InstantOverlay } from './modules/instant/components/InstantOverlay'
import { PointerOverlay } from './modules/instant/components/PointerOverlay'
import './tailwind.css'

/**
 * Which overlay this window is.
 *
 * Read once — a window's label is fixed for its lifetime. Opening
 * `overlay.html` outside Tauri (or under an unexpected label) renders nothing
 * rather than throwing, so a stray navigation cannot produce a blank window
 * with a console error behind it.
 */
function Overlay() {
  const label = hasTauri() ? getCurrentWindow().label : ''
  switch (label) {
    case 'viv_instant':
      return <InstantOverlay />
    case 'viv_pointer':
      return <PointerOverlay />
    case 'dip_overlay':
      return <DipOverlay />
    default:
      return null
  }
}

function ThemedOverlay() {
  const mode = useThemeStore((s) => s.mode)
  const paletteId = useThemeStore((s) => s.paletteId)
  const theme = useMemo(() => buildTheme(mode, paletteId), [mode, paletteId])
  return (
    <ThemeProvider theme={theme}>
      {/* Each overlay additionally calls `useTransparentWindowBody()`, which
          undoes CssBaseline's solid body background — without CssBaseline the
          theme's resets are missing, and without that hook the window paints an
          opaque rectangle over the screen. Both are needed. */}
      <CssBaseline extra={baselineExtras(mode)} />
      <Overlay />
    </ThemeProvider>
  )
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <ThemedOverlay />
  </StrictMode>,
)
