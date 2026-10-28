import { lazy, Suspense } from 'react'
import { Alert, Box, useTheme } from '../../ui'
import { hasTauri } from '../../ipc'
import { SURFACE_RADIUS } from '../../theme'
import { ChatView } from '../ai/components/ChatView'
import { HireRequests } from '../team/components/HireRequests'
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
  // A team project: a teammate picked in the sidebar shows its work, read
  // only; otherwise the lead's conversation, where you talk to the team.
  const teammate = project?.path && modeOf(project) === 'tree' && treeChannel && treeChannel !== LEAD_CHANNEL && treeChannel !== 'room' && !treeChannel.startsWith('group:')
  if (project?.path && teammate) {
    return (
      <Fill>
        <TeamRoom key={project.path} root={project.path} readOnly />
      </Fill>
    )
  }
  return (
    <Fill>
      {/* A team project's lead asks to hire here, where you talk to it. */}
      {project?.path && modeOf(project) === 'tree' && <HireRequests root={project.path} />}
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
