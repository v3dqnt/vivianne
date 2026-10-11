import { lazy, Suspense, useEffect, useState, type FunctionComponent } from 'react'
import { Box, Stack, Alert, Menu, MenuItem, Divider, Tooltip, Typography, ButtonBase, alpha, useTheme } from '../../ui'
import type {
  IDockviewHeaderActionsProps,
  IDockviewPanelHeaderProps,
  IDockviewPanelProps,
  IWatermarkPanelProps,
  DockviewGroupPanel,
} from 'dockview-react'
import { AddLinear, CloseLinear, LayerLinear, MaximizeLinear, MinimizeLinear, ArrowDownLinear } from '../../icons'
import { hasTauri } from '../../ipc'
import { ChatView } from '../ai/components/ChatView'
import { useProjectsStore, modeOf, GENERAL_PROJECT_ID } from '../sessions/store/projectsStore'
import { useGeneralAgents } from '../team/store/generalAgentsStore'
import { useBotsStore } from '../team/store/botsStore'
import { useModsStore, modPanels } from '../mods/store/modsStore'
import { ADDABLE, PANELS, type PanelKind } from './registry'
import { openPanel, setZoneVisible, zoneOf, ZONE_LABELS } from './layout'
import { usePanelStore } from '../dock/store/panelStore'

// Every panel body but the chat is loaded when its panel first appears, not
// with the app. The old fixed docks were lazy-loaded as a whole; importing
// their contents eagerly here would put all of them — CodeMirror and xterm
// included — on the startup path of every launch, whether or not the panels
// are ever opened.
const IdeView = lazy(() => import('../ide/components/IdeView').then((m) => ({ default: m.IdeView })))
const TerminalPanel = lazy(() => import('../dock/components/TerminalPanel').then((m) => ({ default: m.TerminalPanel })))
const WorkspaceTree = lazy(() => import('../dock/components/WorkspaceTree').then((m) => ({ default: m.WorkspaceTree })))
const PreviewPanel = lazy(() => import('../dock/components/PreviewPanel').then((m) => ({ default: m.PreviewPanel })))
const BackgroundTasksPanel = lazy(() =>
  import('../dock/components/BackgroundTasksPanel').then((m) => ({ default: m.BackgroundTasksPanel })),
)
const PlanPanel = lazy(() => import('../plan/components/PlanPanel').then((m) => ({ default: m.PlanPanel })))
const TeamRoom = lazy(() => import('../team/components/TeamRoom').then((m) => ({ default: m.TeamRoom })))
const AgentsPanel = lazy(() => import('../team/components/AgentsPanel').then((m) => ({ default: m.AgentsPanel })))
const ModPanel = lazy(() => import('../mods/components/ModPanel').then((m) => ({ default: m.ModPanel })))
const ReviewPanel = lazy(() => import('../git/components/ReviewPanel').then((m) => ({ default: m.ReviewPanel })))
const MultiplayerWorkspacePanel = lazy(() =>
  import('../multiplayer/components/MultiplayerWorkspacePanel').then((m) => ({ default: m.MultiplayerWorkspacePanel })),
)

interface Params {
  kind: PanelKind
  panelName?: string
}

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
  const isGeneral = useProjectsStore((s) => s.activeProjectId === GENERAL_PROJECT_ID)
  const generalRoot = useGeneralAgents((s) => s.root)
  const generalChannel = useBotsStore((s) => (generalRoot ? s.channel[generalRoot] : undefined))
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
  // One of General's own agents, picked in the sidebar: its direct messages.
  if (isGeneral && generalRoot && generalChannel && generalChannel !== 'room') {
    return (
      <Fill>
        <TeamRoom key={generalRoot} root={generalRoot} />
      </Fill>
    )
  }
  // An agent team's conversation is its room: the user and every teammate.
  if (project?.path && modeOf(project) === 'team') {
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

/** Stands in for the retired document panels: sends what they would have
 * shown to Preview's Documents mode, then closes itself. */
function RedirectToPreview({ path, close }: { path?: string | null; close: () => void }) {
  useEffect(() => {
    const panels = usePanelStore.getState()
    if (path) panels.openDocument(path)
    else {
      panels.setPreviewMode('documents')
      openPanel('preview')
    }
    // After this render: dockview doesn't expect a panel to close itself
    // while it is still mounting.
    const t = window.setTimeout(close, 0)
    return () => window.clearTimeout(t)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])
  return null
}

/** Dockview renders components by name; one per panel kind. */
export const COMPONENTS: Record<PanelKind, FunctionComponent<IDockviewPanelProps<Params>>> = {
  chat: () => <Chat />,
  files: () => (
    <Fill>
      <WorkspaceTree />
    </Fill>
  ),
  ide: () => (
    <Fill>
      <Suspense fallback={null}>
        <IdeView />
      </Suspense>
    </Fill>
  ),
  preview: () => (
    <Fill>
      <PreviewPanel />
    </Fill>
  ),
  review: (props) => (
    <Fill>
      <ReviewPanel onClose={() => props.api.close()} />
    </Fill>
  ),
  documents: (props) => <RedirectToPreview close={() => props.api.close()} />,
  tasks: () => (
    <Fill>
      <BackgroundTasksPanel />
    </Fill>
  ),
  multiplayer: () => (
    <Fill>
      <MultiplayerWorkspacePanel />
    </Fill>
  ),
  agents: () => (
    <Fill>
      <AgentsPanel />
    </Fill>
  ),
  plan: () => (
    <Fill>
      <PlanPanel />
    </Fill>
  ),
  // Documents were a panel (and a panel per file) before they became a mode
  // of Preview. A saved layout or the agent's set_panel can still ask for
  // them; both land in Preview.
  document: (props) => <RedirectToPreview path={props.params.panelName} close={() => props.api.close()} />,
  terminal: () => (
    <Fill>
      <Suspense fallback={null}>
        <TerminalPanel />
      </Suspense>
    </Fill>
  ),
  custom: (props) => (
    <Fill>
      <ModPanel name={props.params.panelName ?? null} />
    </Fill>
  ),
}

/** A tab: icon, title, and a close button — none for the chat, which can be
 * moved anywhere but never closed. Middle-click closes, like a browser tab. */
export function PanelTab(props: IDockviewPanelHeaderProps<Params>) {
  const kind = props.params.kind
  const def = PANELS[kind] ?? PANELS.custom
  const Icon = def.icon
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
      <Icon size={12} color="currentColor" />
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

/** The right end of every group's tab strip: add a panel into this group, and
 * maximize / restore it. */
export function GroupActions(props: IDockviewHeaderActionsProps) {
  const [anchor, setAnchor] = useState<HTMLElement | null>(null)
  const [maximized, setMaximized] = useState(props.api.isMaximized())
  const mods = useModsStore((s) => s.mods)
  const customPanels = modPanels(mods)

  useEffect(() => {
    const d = props.containerApi.onDidMaximizedGroupChange(() => setMaximized(props.api.isMaximized()))
    return () => d.dispose()
  }, [props.api, props.containerApi])

  const zone = zoneOf(props.group)

  const add = (kind: PanelKind, panelName?: string) => {
    setAnchor(null)
    openPanel(kind, {
      panelName,
      group: props.group as DockviewGroupPanel,
      // "+" in a group means "put one here" — a terminal always gets a new
      // shell; anything else already open moves here rather than duplicating.
      newInstance: kind === 'terminal',
    })
  }

  return (
    <Stack direction="row" sx={{ alignItems: 'center', gap: 0.25, px: 0.75, height: '100%' }}>
      <Tooltip title="Add a panel here">
        <Box component="button" type="button" aria-label="Add a panel" onClick={(e) => setAnchor(e.currentTarget as HTMLElement)} sx={headerBtnSx}>
          <AddLinear size={13} color="currentColor" />
        </Box>
      </Tooltip>
      {zone ? (
        // A zone is hidden rather than maximized: it belongs to the window
        // edge, and the title bar's panel toggle brings it back.
        <Tooltip title={`Hide ${ZONE_LABELS[zone].toLowerCase()}`}>
          <Box component="button" type="button" aria-label={`Hide ${ZONE_LABELS[zone]}`} onClick={() => setZoneVisible(zone, false)} sx={headerBtnSx}>
            <Box sx={{ display: 'grid', transform: zone === 'right' ? 'rotate(-90deg)' : 'none' }}>
              <ArrowDownLinear size={12} color="currentColor" />
            </Box>
          </Box>
        </Tooltip>
      ) : (
        <Tooltip title={maximized ? 'Restore' : 'Maximize'}>
          <Box
            component="button"
            type="button"
            aria-label={maximized ? 'Restore group size' : 'Maximize group'}
            onClick={() => (maximized ? props.api.exitMaximized() : props.api.maximize())}
            sx={headerBtnSx}
          >
            {maximized ? <MinimizeLinear size={12} color="currentColor" /> : <MaximizeLinear size={12} color="currentColor" />}
          </Box>
        </Tooltip>
      )}
      <Menu anchorEl={anchor} open={!!anchor} onClose={() => setAnchor(null)}>
        {ADDABLE.map((kind) => {
          const Icon = PANELS[kind].icon
          return (
            <MenuItem key={kind} onClick={() => add(kind)} sx={{ gap: 1, fontSize: '0.8125rem' }}>
              <Icon size={14} color="currentColor" />
              {kind === 'terminal' ? 'New terminal' : PANELS[kind].title}
            </MenuItem>
          )
        })}
        {customPanels.length > 0 && <Divider />}
        {customPanels.map((p) => (
          <MenuItem key={p.name} onClick={() => add('custom', p.name)} sx={{ gap: 1, fontSize: '0.8125rem' }}>
            <LayerLinear size={14} color="currentColor" />
            {p.title}
          </MenuItem>
        ))}
      </Menu>
    </Stack>
  )
}

/**
 * What an empty zone shows: the utilities it can hold, as buttons in the
 * middle of the panel. The same list is behind the header's "+"; this is the
 * version nobody has to go looking for, shown exactly when the panel has
 * nothing else to show.
 */
export function ZoneEmptyState(props: IWatermarkPanelProps) {
  const theme = useTheme()
  const mods = useModsStore((s) => s.mods)
  const customPanels = modPanels(mods)
  const group = props.group as DockviewGroupPanel | undefined
  const zone = zoneOf(group)
  if (!group || !zone) return null

  // The bottom panel is where terminals live, so they lead there.
  const kinds = zone === 'bottom' ? ['terminal' as const, ...ADDABLE.filter((k) => k !== 'terminal')] : ADDABLE

  const option = (key: string, label: string, Icon: typeof LayerLinear, onClick: () => void) => (
    <ButtonBase
      key={key}
      onClick={onClick}
      sx={{
        display: 'flex',
        alignItems: 'center',
        gap: 1,
        px: 1.5,
        py: 1,
        borderRadius: '10px',
        justifyContent: 'flex-start',
        border: 1,
        borderColor: 'divider',
        color: 'text.secondary',
        fontSize: '0.8125rem',
        transition: 'background-color 0.12s, color 0.12s, border-color 0.12s',
        '&:hover': {
          bgcolor: alpha(theme.palette.primary.main, 0.1),
          borderColor: alpha(theme.palette.primary.main, 0.4),
          color: 'primary.main',
        },
        '&:focus-visible': { outline: `2px solid ${theme.palette.primary.main}`, outlineOffset: 1 },
      }}
    >
      <Icon size={14} color="currentColor" />
      <span>{label}</span>
    </ButtonBase>
  )

  return (
    // Centred with auto margins, not `justifyContent: center`: a centred flex
    // column that is taller than a short bottom panel overflows at BOTH ends,
    // and the top end — the heading — becomes unreachable by scrolling.
    <Stack sx={{ height: '100%', alignItems: 'center', p: 2, overflowY: 'auto' }}>
      <Stack sx={{ gap: 1.5, width: '100%', maxWidth: 420, alignItems: 'center', my: 'auto' }}>
        <Stack sx={{ alignItems: 'center', gap: 0.25 }}>
          <Typography variant="body2" sx={{ fontWeight: 600 }}>
            {ZONE_LABELS[zone]} is empty
          </Typography>
          <Typography variant="caption" color="text.secondary" sx={{ textAlign: 'center' }}>
            Open something here, or drag any tab onto it.
          </Typography>
        </Stack>
        <Box
          sx={{
            display: 'grid',
            // Two columns when there is room, one when docked narrow.
            gridTemplateColumns: 'repeat(auto-fill, minmax(150px, 1fr))',
            gap: 0.75,
            width: '100%',
          }}
        >
          {kinds.map((kind) =>
            option(kind, kind === 'terminal' ? 'Terminal' : PANELS[kind].title, PANELS[kind].icon, () =>
              openPanel(kind, { group, newInstance: kind === 'terminal' }),
            ),
          )}
          {customPanels.map((p) =>
            option(`custom:${p.name}`, p.title, LayerLinear, () => openPanel('custom', { panelName: p.name, group })),
          )}
        </Box>
      </Stack>
    </Stack>
  )
}
