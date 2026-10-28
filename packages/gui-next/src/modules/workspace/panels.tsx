import { lazy, Suspense, useEffect, useState, type FunctionComponent } from 'react'
import { Box, Stack, Alert, Tooltip, Typography, ButtonBase, alpha, useTheme } from '../../ui'
import type {
  IDockviewHeaderActionsProps,
  IDockviewPanelHeaderProps,
  IDockviewPanelProps,
  IWatermarkPanelProps,
  DockviewGroupPanel,
} from 'dockview-react'
import { CloseLinear, ExpandLinear, ExitFullScreenLinear } from '../../icons'
import { hasTauri } from '../../ipc'
import { ChatView } from '../ai/components/ChatView'
import { useProjectsStore, modeOf, GENERAL_PROJECT_ID } from '../sessions/store/projectsStore'
import { useGeneralAgents } from '../team/store/generalAgentsStore'
import { useBotsStore } from '../team/store/botsStore'
import { LEAD_CHANNEL } from '../team/components/TreeTeam'
import { ADDABLE, PANELS, type PanelKind } from './registry'
import { closeContainer, dockBack, isFloating, openFullScreen, openPanel, zoneOf, ZONE_LABELS } from './layout'

// Every panel body but the chat is loaded when its panel first appears, not
// with the app. The old fixed docks were lazy-loaded as a whole; importing
// their contents eagerly here would put all of them â€” xterm included â€” on the
// startup path of every launch, whether or not the panels are ever opened.
const FilesPanel = lazy(() => import('../panels/files/FilesPanel').then((m) => ({ default: m.FilesPanel })))
const BrowserPanel = lazy(() => import('../panels/browser/BrowserPanel').then((m) => ({ default: m.BrowserPanel })))
// Moved to `modules/panels/terminal` in Task 5; until then it stays where it is.
const TerminalPanel = lazy(() => import('../panels/terminal/TerminalPanel').then((m) => ({ default: m.TerminalPanel })))
const TeamRoom = lazy(() => import('../team/components/TeamRoom').then((m) => ({ default: m.TeamRoom })))

interface Params {
  kind: PanelKind
}

/** Every panel fills its box and scrolls itself; the group never scrolls. */
function Fill({ children }: { children: React.ReactNode }) {
  return (
    // Transparent, so the group's glass surface shows through â€” the same
    // way the sidebar's content sits directly on its card.
    <Box sx={{ height: '100%', width: '100%', minHeight: 0, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
      <Suspense fallback={null}>{children}</Suspense>
    </Box>
  )
}

function Chat() {
  const project = useProjectsStore((s) => s.projects.find((p) => p.id === s.activeProjectId))
  const isGeneral = useProjectsStore((s) => s.activeProjectId === GENERAL_PROJECT_ID)
  const generalRoot = useGeneralAgents((s) => s.root)
  const generalChannel = useBotsStore((s) => (generalRoot ? s.channel[generalRoot] : undefined))
  const treeChannel = useBotsStore((s) => (project?.path ? s.channel[project.path] : undefined))
  if (!hasTauri()) {
    return (
      <Fill>
        <Box sx={{ p: 3 }}>
          <Alert severity="info">
            Running in a plain browser â€” the Tauri backend is not connected, so chat is unavailable. Launch the
            desktop app for the real thing.
          </Alert>
        </Box>
      </Fill>
    )
  }
  // One of General's own agents, picked in the sidebar: its direct messages.
  if (isGeneral && generalRoot && generalChannel && generalChannel !== 'room') {
    return (
      <Fill>
        <TeamRoom key={generalRoot} root={generalRoot} />
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

/** Dockview renders components by name; one per panel kind. */
export const COMPONENTS: Record<PanelKind, FunctionComponent<IDockviewPanelProps<Params>>> = {
  chat: () => <Chat />,
  files: () => (
    <Fill>
      <FilesPanel />
    </Fill>
  ),
  browser: () => (
    <Fill>
      <BrowserPanel />
    </Fill>
  ),
  terminal: () => (
    <Fill>
      <TerminalPanel />
    </Fill>
  ),
}

/** A tab: icon, title, and a close button â€” none for the chat, which can be
 * moved anywhere but never closed. Middle-click closes, like a browser tab. */
export function PanelTab(props: IDockviewPanelHeaderProps<Params>) {
  const kind = props.params.kind
  const def = PANELS[kind]
  const [title, setTitle] = useState(props.api.title ?? def.title)
  useEffect(() => {
    const d = props.api.onDidTitleChange((e) => setTitle(e.title))
    return () => d.dispose()
  }, [props.api])

  return (
    <Stack
      direction="row"
      onAuxClick={(e) => {
        if (e.button === 1 && def.closable) {
          e.preventDefault()
          props.api.close()
        }
      }}
      sx={{ alignItems: 'center', gap: 0.75, px: 1.25, height: '100%', maxWidth: 220, '&:hover .viv-tab-close': { opacity: 1 } }}
    >
      <Box component="span" sx={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', fontSize: '0.75rem', fontWeight: 500 }}>
        {title}
      </Box>
      {def.closable && (
        <Box
          component="button"
          type="button"
          className="viv-tab-close"
          aria-label={`Close ${title}`}
          // Stop the press reaching the tab, or dockview starts a drag.
          onPointerDown={(e) => e.stopPropagation()}
          onClick={(e) => {
            e.stopPropagation()
            props.api.close()
          }}
          sx={{
            border: 0,
            bgcolor: 'transparent',
            color: 'inherit',
            p: 0.25,
            ml: 0.25,
            borderRadius: '4px',
            cursor: 'pointer',
            display: 'grid',
            opacity: props.api.isActive ? 0.7 : 0,
            transition: 'opacity 0.12s',
            '&:hover': { bgcolor: 'action.hover', opacity: 1 },
            '&:focus-visible': { opacity: 1, outline: '2px solid', outlineColor: 'primary.main' },
          }}
        >
          <CloseLinear size={11} color="currentColor" />
        </Box>
      )}
    </Stack>
  )
}

const headerBtnSx = {
  width: 24,
  height: 24,
  border: 0,
  bgcolor: 'transparent',
  color: 'text.secondary',
  borderRadius: '6px',
  cursor: 'pointer',
  display: 'grid',
  placeItems: 'center',
  '&:hover': { bgcolor: 'action.hover', color: 'text.primary' },
} as const

/** The right end of the container's tab strip: lift the tab in front into a
 * dialog, and close the whole container. */
export function GroupActions(props: IDockviewHeaderActionsProps) {
  const group = props.group as DockviewGroupPanel
  const activePanel = props.group.activePanel
  const zone = zoneOf(props.group)
  const floating = isFloating(group)

  return (
    <Stack direction="row" sx={{ alignItems: 'center', gap: 0.25, px: 0.75, height: '100%' }}>
      {floating ? (
        <Tooltip title="Back to the panel">
          <Box component="button" type="button" aria-label="Exit full screen" onClick={() => dockBack(group)} sx={headerBtnSx}>
            <ExitFullScreenLinear size={12} color="currentColor" />
          </Box>
        </Tooltip>
      ) : (
        zone &&
        activePanel && (
          <Tooltip title="Open in a full-screen dialog">
            <Box component="button" type="button" aria-label="Expand panel full screen" onClick={() => openFullScreen(activePanel.id)} sx={headerBtnSx}>
              <ExpandLinear size={13} color="currentColor" />
            </Box>
          </Tooltip>
        )
      )}
      {/* Close the container itself, not the panel inside it: a zone steps
          aside with its panels intact (back from the title bar's Panels
          menu), a dialog closes what it carries. Individual panels still close
          from their own tab. */}
      <Tooltip title={floating ? 'Close' : 'Close this panel container'}>
        <Box component="button" type="button" aria-label="Close panel container" onClick={() => closeContainer(group)} sx={headerBtnSx}>
          <CloseLinear size={12} color="currentColor" />
        </Box>
      </Tooltip>
    </Stack>
  )
}

/** What an empty container shows: the three panels it can hold, as buttons in
 * the middle. Shown exactly when the container has nothing else to show. */
export function ZoneEmptyState(props: IWatermarkPanelProps) {
  const theme = useTheme()
  const group = props.group as DockviewGroupPanel | undefined
  const zone = zoneOf(group)
  if (!group || !zone) return null
  return (
    <Stack sx={{ height: '100%', alignItems: 'center', p: 2, overflowY: 'auto' }}>
      <Stack sx={{ gap: 1.5, width: '100%', maxWidth: 420, alignItems: 'center', my: 'auto' }}>
        <Stack sx={{ alignItems: 'center', gap: 0.25 }}>
          <Typography variant="body2" sx={{ fontWeight: 600 }}>{ZONE_LABELS[zone]} is empty</Typography>
          <Typography variant="caption" color="text.secondary" sx={{ textAlign: 'center' }}>
            Open something here, or drag any tab onto it.
          </Typography>
        </Stack>
        <Box sx={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(150px, 1fr))', gap: 0.75, width: '100%' }}>
          {ADDABLE.map((kind) => {
            const Icon = PANELS[kind].icon
            return (
              <ButtonBase
                key={kind}
                onClick={() => openPanel(kind, { group })}
                sx={{
                  display: 'flex', alignItems: 'center', gap: 1, px: 1.5, py: 1, borderRadius: '10px',
                  justifyContent: 'flex-start', border: 1, borderColor: 'divider', color: 'text.secondary',
                  fontSize: '0.8125rem', transition: 'background-color 0.12s, color 0.12s, border-color 0.12s',
                  '&:hover': { bgcolor: alpha(theme.palette.primary.main, 0.1), borderColor: alpha(theme.palette.primary.main, 0.4), color: 'primary.main' },
                  '&:focus-visible': { outline: `2px solid ${theme.palette.primary.main}`, outlineOffset: 1 },
                }}
              >
                <Icon size={14} color="currentColor" />
                <span>{PANELS[kind].title}</span>
              </ButtonBase>
            )
          })}
        </Box>
      </Stack>
    </Stack>
  )
}
