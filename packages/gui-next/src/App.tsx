import { useState } from 'react'
import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import Alert from '@mui/material/Alert'
import { hasTauri } from './ipc'
import { TitleBar } from './components/TitleBar'
import { ChatView } from './modules/ai/components/ChatView'
import { Sidebar } from './modules/sessions/components/Sidebar'
import { SettingsDialog } from './modules/settings/components/SettingsDialog'

function App() {
  const [settingsOpen, setSettingsOpen] = useState(false)

  return (
    <Stack sx={{ height: '100%' }}>
      <TitleBar />

      <Stack direction="row" sx={{ flex: 1, minHeight: 0 }}>
        <Sidebar onOpenSettings={() => setSettingsOpen(true)} />

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
            <ChatView />
          )}
        </Stack>
      </Stack>

      <SettingsDialog open={settingsOpen} onClose={() => setSettingsOpen(false)} />
    </Stack>
  )
}

export default App
