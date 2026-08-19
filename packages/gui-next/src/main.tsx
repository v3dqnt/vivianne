import { StrictMode, useMemo } from 'react'
import { createRoot } from 'react-dom/client'
import { ThemeProvider } from '@mui/material/styles'
import CssBaseline from '@mui/material/CssBaseline'
import { buildTheme } from './theme'
import { useThemeStore } from './modules/settings/store/themeStore'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { hasTauri } from './ipc'
import { initProviderDispatcher } from './modules/ai/providers/dispatcher'
import App from './App.tsx'
import './tailwind.css'

// Registered at app entry, before the first render. This one listener
// services every LLM request in the app, so it must not depend on which
// thread's store happened to initialize first (it used to be a side effect
// of the primary thread's `agentStore.init()`) — if that never ran, or ran
// late, requests went nowhere and every turn died on Rust's 120s idle guard.
//
// Main window ONLY. The `notch` window (voice UI, hidden at startup) has no
// route of its own here, so Vite's SPA fallback serves it this same bundle —
// without this guard both webviews answer every `provider_stream_request`,
// firing two concurrent API calls per turn whose tokens interleave under one
// stream id (garbled/doubled replies, double spend, and a stall whenever the
// two disagree about who finished).
if (hasTauri() && getCurrentWindow().label !== 'notch') void initProviderDispatcher()

function ThemedApp() {
  const mode = useThemeStore((s) => s.mode)
  const paletteId = useThemeStore((s) => s.paletteId)
  const theme = useMemo(() => buildTheme(mode, paletteId), [mode, paletteId])
  return (
    <ThemeProvider theme={theme}>
      {/* CssBaseline applies the theme's resets — including the no-root-scroll
          rule a desktop window needs. */}
      <CssBaseline />
      <App />
    </ThemeProvider>
  )
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <ThemedApp />
  </StrictMode>,
)
