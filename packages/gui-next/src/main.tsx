import { StrictMode, useMemo } from 'react'
import { createRoot } from 'react-dom/client'
import { CssBaseline, ThemeProvider } from './ui'
import { baselineExtras, buildTheme } from './theme'
import { useThemeStore, themeOptions, activePaletteId } from './modules/settings/store/themeStore'
import { AppearanceEffects } from './modules/settings/components/AppearanceEffects'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { hasTauri } from './ipc'
import { invoke } from '@tauri-apps/api/core'
import { initProviderDispatcher } from './modules/ai/providers/dispatcher'
import { initNotchDelegate } from './modules/instant/lib/notchDelegate'
import { initNotificationReply } from './modules/ai/lib/notificationReply'
import { initDiagramCheck } from './modules/ai/lib/diagramCheck'
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
if (hasTauri() && getCurrentWindow().label === 'main') {
  void initProviderDispatcher()
  // Work said to the notch, handed to a project's agent here.
  initNotchDelegate()
  // What's typed into a notification's reply box, sent to the chat.
  initNotificationReply()
  // The agent's check_diagram tool, answered with the same code the chat draws with.
  initDiagramCheck()
  // Only now is it safe to create the notch's window (see `cmd_notch_app_ready`).
  const ready = () => void invoke('cmd_notch_app_ready').catch(() => {})
  if (document.readyState === 'complete') ready()
  else window.addEventListener('load', ready, { once: true })
}

function ThemedApp() {
  const appearance = useThemeStore()
  const mode = appearance.mode
  const paletteId = activePaletteId(appearance)
  const opts = themeOptions(appearance)
  const key = JSON.stringify([mode, paletteId, opts])
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const theme = useMemo(() => buildTheme(mode, paletteId, opts), [key])
  return (
    <ThemeProvider theme={theme}>
      {/* CssBaseline applies the theme's resets — including the no-root-scroll
          rule a desktop window needs. */}
      <CssBaseline extra={baselineExtras(mode)} />
      <AppearanceEffects />
      <App />
    </ThemeProvider>
  )
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <ThemedApp />
  </StrictMode>,
)
