import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import Alert from '@mui/material/Alert'
import { hasTauri } from './ipc'
import { TitleBar } from './components/TitleBar'
import { ChatView } from './modules/ai/components/ChatView'
import { ProjectRail } from './modules/sessions/components/ProjectRail'
import { Sidebar } from './modules/sessions/components/Sidebar'
import { SettingsDialog } from './modules/settings/components/SettingsDialog'
import { SidePanel } from './modules/dock/components/SidePanel'
import { BottomTerminal } from './modules/dock/components/BottomTerminal'
import { ChatDockZone } from './modules/sessions/components/ChatDockZone'
import { ThreadTabBar } from './modules/sessions/components/ThreadTabBar'
import { useThreadsStore } from './modules/sessions/store/threadsStore'
import { GestureCursorOverlay } from './modules/gesture/components/GestureCursorOverlay'

function App() {
  const hasOpenThreads = useThreadsStore((s) => s.openIds.length > 0)

  return (
    <Stack sx={{ height: '100%' }}>
      <TitleBar />

      <Stack direction="row" sx={{ flex: 1, minHeight: 0 }}>
        {/* Projects own everything: the rail picks one, the sidebar lists
            only that project's threads. */}
        <ProjectRail />
        <Sidebar />

        {/* Chat column — the terminal docks under it rather than beside it,
            so it spans the conversation's full width like a real editor's. */}
        <Stack sx={{ flex: 1, minWidth: 0, height: '100%' }}>
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
              <BottomTerminal />
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

        {/* Right panel — opened from the title bar's Panels menu. */}
        <SidePanel />
      </Stack>

      <SettingsDialog />
      {hasTauri() && <GestureCursorOverlay />}
    </Stack>
  )
}

export default App
