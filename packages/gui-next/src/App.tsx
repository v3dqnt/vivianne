import { lazy, Suspense } from 'react'
import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import Alert from '@mui/material/Alert'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { hasTauri } from './ipc'
import { TitleBar } from './components/TitleBar'
import { VoiceSession } from './modules/voice/components/VoiceSession'
import { DipOverlay } from './modules/dip/components/DipOverlay'
import { ChatView } from './modules/ai/components/ChatView'
import { ProjectRail } from './modules/sessions/components/ProjectRail'
import { Sidebar } from './modules/sessions/components/Sidebar'
import { SettingsDialog } from './modules/settings/components/SettingsDialog'
import { OptionalComponentsModal } from './modules/settings/components/OptionalComponentsModal'
import { UpdateSurfaces } from './modules/updates/components/UpdateSurfaces'
import { ChatDockZone } from './modules/sessions/components/ChatDockZone'
import { ThreadTabBar } from './modules/sessions/components/ThreadTabBar'
import { useThreadsStore } from './modules/sessions/store/threadsStore'

// Lazy: @xterm is one of the heaviest deps in the app, and the terminal dock
// is often never opened in a session — keep it out of the eager startup
// bundle and let Vite split it into its own chunk.
const Dock = lazy(() => import('./modules/dock/components/Dock').then((m) => ({ default: m.Dock })))

function App() {
  const hasOpenThreads = useThreadsStore((s) => s.openIds.length > 0)

  // The `notch` window (voice mode) has no route of its own — Vite's SPA
  // fallback serves it this same bundle — so it renders standalone here
  // instead of the full shell. Checked once per mount, not reactively: a
  // window's label is fixed for its lifetime.
  if (hasTauri() && getCurrentWindow().label === 'notch') {
    return <VoiceSession />
  }
  if (hasTauri() && getCurrentWindow().label === 'dip_overlay') {
    return <DipOverlay />
  }

  return (
    <Stack sx={{ height: '100%' }}>
      <TitleBar />

      {/* `bgcolor` here is the chrome colour the title bar and rail/sidebar
          share, so the curve cut out of the chat column below reveals chrome
          rather than a third colour. */}
      <Stack direction="row" sx={{ flex: 1, minHeight: 0, bgcolor: 'background.chrome' }}>
        {/* Projects own everything: the rail picks one, the sidebar lists
            only that project's threads. */}
        <ProjectRail />
        <Sidebar />

        {/* Chat column — the terminal docks under it rather than beside it,
            so it spans the conversation's full width like a real editor's.
            Rounded top-left only: that single corner is what makes the title
            bar and the sidebar read as one continuous L of chrome the content
            is inset into, instead of two separate bars meeting at a seam. */}
        <Stack
          sx={{
            flex: 1,
            minWidth: 0,
            height: '100%',
            bgcolor: 'background.default',
            borderTopLeftRadius: 12,
            overflow: 'hidden',
          }}
        >
          {!hasTauri() ? (
            <Box sx={{ p: 3 }}>
              <Alert severity="info">
                Running in a plain browser — the Tauri backend is not connected,
                so chat is unavailable. Launch the desktop app for the real
                thing.
              </Alert>
            </Box>
          ) : (
            <>
              <ThreadTabBar />
              <ChatView />
              <Suspense fallback={null}>
                <Dock dock="bottom" />
              </Suspense>
            </>
          )}
        </Stack>

        {/* Secondary chat threads, split in via right-click "Open in split
            view" on a thread row (or dragged in) — only takes up space once
            a thread is actually open there; no empty placeholder otherwise. */}
        {hasTauri() && hasOpenThreads && (
          <Box sx={{ flex: 2, minWidth: 320, display: 'flex' }}>
            <ChatDockZone />
          </Box>
        )}

        {/* Right sidebar — toggled from the title bar, contents chosen in its
            own tab strip. */}
        <Suspense fallback={null}>
          <Dock dock="right" />
        </Suspense>
      </Stack>

      <SettingsDialog />
      <OptionalComponentsModal />
      <UpdateSurfaces />
    </Stack>
  )
}

export default App
