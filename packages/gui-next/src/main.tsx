import { StrictMode, useMemo } from 'react'
import { createRoot } from 'react-dom/client'
import { CssBaseline, ThemeProvider } from './ui'
import { baselineExtras, buildTheme } from './theme'
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
// Main window ONLY, as an ALLOWlist. Every secondary window (the DIP overlay,
// Viv Instant) has no route of its own here, so Vite's SPA fallback serves each
// of them this same bundle — and without this guard every one of them would
// answer every `provider_stream_request`, firing concurrent API calls per turn
// whose tokens interleave under one stream id (garbled/doubled replies, double
// spend, and a stall whenever they disagree about who finished).
//
// Deliberately `=== 'main'` rather than excluding known labels: a denylist has
// to be updated every time a window is added, and forgetting silently doubles
// every API call rather than failing.
if (hasTauri() && getCurrentWindow().label === 'main') void initProviderDispatcher()

function ThemedApp() {
  const mode = useThemeStore((s) => s.mode)
  const paletteId = useThemeStore((s) => s.paletteId)
  const theme = useMemo(() => buildTheme(mode, paletteId), [mode, paletteId])
  return (
    <ThemeProvider theme={theme}>
      {/* CssBaseline applies the theme's resets — including the no-root-scroll
          rule a desktop window needs. */}
      <CssBaseline extra={baselineExtras(mode)} />
      <App />
    </ThemeProvider>
  )
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <ThemedApp />
  </StrictMode>,
)
