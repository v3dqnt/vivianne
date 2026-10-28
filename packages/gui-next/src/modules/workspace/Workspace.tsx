import { lazy, Suspense } from 'react'
import { Alert, Box, useTheme } from '../../ui'
import { hasTauri } from '../../ipc'
import { mainPane } from '../../theme'
import { ChatView } from '../ai/components/ChatView'
import { HireRequests } from '../team/components/HireRequests'
import { useProjectsStore, modeOf } from '../sessions/store/projectsStore'
import { TeamStrip } from '../team/components/TeamStrip'
import { ProjectTools } from '../team/components/ProjectTools'
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
  const isTeam = !!project?.path && modeOf(project) === 'tree'
  // A team project: a teammate picked on the left shows its work, read
  // only; otherwise the lead's conversation, where you talk to the team.
  const teammate = isTeam && treeChannel && treeChannel !== LEAD_CHANNEL && treeChannel !== 'room' && !treeChannel.startsWith('group:')
  return (
    <Fill>
      {/* The team's people sit on the left; the same space on the right keeps the chat centred. */}
      <Box sx={{ position: 'relative', flex: 1, minHeight: 0, display: 'flex', flexDirection: 'column', px: isTeam ? '212px' : 0 }}>
        {isTeam && project?.path && <TeamStrip root={project.path} projectName={project.name} />}
        {isTeam && project?.path && <ProjectTools root={project.path} />}
        {project?.path && teammate ? (
          <TeamRoom key={project.path} root={project.path} readOnly />
        ) : (
          <>
            {/* A team project's lead asks to hire here, where you talk to it. */}
            {isTeam && project?.path && <HireRequests root={project.path} />}
            <ChatView />
          </>
        )}
      </Box>
    </Fill>
  )
}

/**
 * The main area: the conversation, on one pane. There are no side panels:
 * files, links and documents open in the system's own apps.
 */
export function Workspace() {
  const theme = useTheme()
  return (
    <Box sx={{ flex: 1, minWidth: 0, height: '100%', bgcolor: 'background.chrome', pr: 1, pb: 1, pl: 0.5 }}>
      <Box
        sx={{
          height: '100%',
          display: 'flex',
          flexDirection: 'column',
          ...mainPane(theme),
        }}
      >
        <Chat />
      </Box>
    </Box>
  )
}
