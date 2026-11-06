import { Alert, Box, useTheme } from '../../ui'
import { hasTauri } from '../../ipc'
import { SURFACE_RADIUS } from '../../theme'
import { ChatView } from '../ai/components/ChatView'
import { ProjectTools, RAIL_RESERVE } from '../team/components/ProjectTools'

/** Every panel fills its box and scrolls itself; the group never scrolls. */
function Fill({ children }: { children: React.ReactNode }) {
  return (
    // Transparent, so the group's glass surface shows through — the same
    // way the sidebar's content sits directly on its card.
    <Box sx={{ height: '100%', width: '100%', minHeight: 0, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
      {children}
    </Box>
  )
}

function Chat() {
  if (!hasTauri()) {
    return (
      <Fill>
        <Box sx={{ p: 3 }}>
          <Alert severity="info">
            Running in a plain browser — the Tauri backend is not connected, so chat is unavailable. Launch the
            desktop app for the real thing.
          </Alert>
        </Box>
      </Fill>
    )
  }
  // The threads rail lives on the workspace's chrome (see Workspace), for
  // General and for threads projects alike, so the dark surface starts at
  // the conversation itself. Only a project brings tools to its right.
  return (
    <Fill>
      <Box
        sx={{
          position: 'relative',
          flex: 1,
          minHeight: 0,
          display: 'flex',
          flexDirection: 'column',
          // The tools strip sits on the pane's right; the threads are not on
          // the pane at all, so nothing is reserved on the left.
          pr: RAIL_RESERVE,
        }}
      >
        <ProjectTools />
        <ChatView />
      </Box>
    </Fill>
  )
}

/**
 * The main area: the conversation, on one pane. There are no side panels:
 * files, links and documents open in the system's own apps.
 *
 * General and threads projects both put their threads on the workspace's
 * chrome, to the left of the pane: the chat pane's dark surface starts at its
 * own left edge instead of reserving a column beside the chat. A project's
 * right side stays on the dark pane, with the project tools riding it.
 */
export function Workspace() {
  const theme = useTheme()
  const { pane } = theme.palette.background
  return (
    // One rounded card on the frame, inset from the window's edges: the thread
    // rail (a step lighter than the frame) and the chat (lightest) joined
    // inside it with no gap or notch between them.
    <Box sx={{ flex: 1, minWidth: 0, height: '100%', display: 'flex', minHeight: 0, bgcolor: 'background.chrome', pr: 1, pb: 1, pl: 1 }}>
      <Box
        sx={{
          flex: 1,
          minWidth: 0,
          display: 'flex',
          minHeight: 0,
          borderRadius: `${SURFACE_RADIUS}px`,
          overflow: 'hidden',
          border: `1px solid ${theme.palette.divider}`,
          boxShadow: theme.elevation.pane,
        }}
      >
        <Box
          sx={{
            flex: 1,
            minWidth: 0,
            position: 'relative',
            display: 'flex',
            flexDirection: 'column',
            bgcolor: pane,
          }}
        >
          <Chat />
        </Box>
      </Box>
    </Box>
  )
}
