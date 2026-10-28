import { lazy, Suspense } from 'react'
import { Alert, Box, useTheme } from '../../ui'
import { hasTauri } from '../../ipc'
import { SURFACE_RADIUS } from '../../theme'
import { ChatView } from '../ai/components/ChatView'
import { useProjectsStore, modeOf } from '../sessions/store/projectsStore'
import { useBotsStore } from '../team/store/botsStore'
import { LEAD_CHANNEL } from '../team/components/TreeTeam'

const TeamRoom = lazy(() => import('../team/components/TeamRoom').then((m) => ({ default: m.TeamRoom })))

/** Every panel fills its box and scrolls itself; the group never scrolls. */
function Fill({ children }: { children: React.ReactNode }) {
  return (
    // Transparent, so the group's glass surface shows through — the same
    // way the sidebar's content sits directly on its card.
    <Box sx={{ height: '100%', width: '100%', minHeight: 0, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
      <Suspense fallback={null}>{children}</Suspense>
    </Box>
  )
}

function Chat() {
  const project = useProjectsStore((s) => s.projects.find((p) => p.id === s.activeProjectId))
  const treeChannel = useBotsStore((s) => (project?.path ? s.channel[project.path] : undefined))
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
  // A tree shows the circle or DM picked in the sidebar, or its master
  // lead's conversation.
  if (project?.path && modeOf(project) === 'tree' && treeChannel !== LEAD_CHANNEL) {
    return (
      <Fill>
        <TeamRoom key={project.path} root={project.path} />
      </Fill>
    )
  }
  return (
    <Fill>
      <ChatView />
    </Fill>
  )
}

/**
 * The main area: the conversation, on one pane. There are no side panels:
 * files, links and documents open in the system's own apps.
 */
export function Workspace() {
  const theme = useTheme()
  const t = theme.palette.tokens
  return (
    <Box sx={{ flex: 1, minWidth: 0, height: '100%', bgcolor: 'background.chrome', pr: 1, pb: 1, pl: 0.5 }}>
      <Box
        sx={{
          height: '100%',
          display: 'flex',
          flexDirection: 'column',
          borderRadius: `${SURFACE_RADIUS}px`,
          overflow: 'hidden',
          bgcolor: t.background,
          border: `1px solid ${t.border}`,
          boxShadow: theme.elevation.pane,
        }}
      >
        <Chat />
      </Box>
    </Box>
  )
}
