import { useMemo, useRef, useState } from 'react'
import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import IconButton from '@mui/material/IconButton'
import Tooltip from '@mui/material/Tooltip'
import Menu from '@mui/material/Menu'
import MenuItem from '@mui/material/MenuItem'
import Divider from '@mui/material/Divider'
import { useTheme } from '@mui/material/styles'
import {
  CloseLinear,
  AddLinear,
  DocumentTextLinear,
  MonitorLinear,
  TaskLinear,
  GithubLinear,
  Link2Linear,
  CommandSquareLinear,
  LayerLinear,
} from '../../../icons'
import { surfaceCard } from '../../../theme'
import { ResizeHandle } from '../../../components/ResizeHandle'
import { usePanelStore, VIEW_LABELS, type DockId, type DockTab, type DockViewKind } from '../store/panelStore'
import { usePlaybookStore } from '../../settings/store/playbookStore'
import { isPanel } from '../../settings/lib/playbooks'
import { WorkspaceTree } from './WorkspaceTree'
import { PreviewPanel } from './PreviewPanel'
import { BackgroundTasksPanel } from './BackgroundTasksPanel'
import { CustomPanel } from './CustomPanel'
import { TerminalPanel } from './TerminalPanel'
import { ReviewPanel } from '../../git/components/ReviewPanel'
import { MultiplayerWorkspacePanel } from '../../multiplayer/components/MultiplayerWorkspacePanel'

const VIEW_ICONS: Record<Exclude<DockViewKind, 'custom'>, typeof DocumentTextLinear> = {
  review: GithubLinear,
  files: DocumentTextLinear,
  preview: MonitorLinear,
  tasks: TaskLinear,
  multiplayer: Link2Linear,
  terminal: CommandSquareLinear,
}

/** Every square control in the tab strip — the add button and the dock's
 * hide button — so they read as one row of equal-weight affordances. */
const headerBtnSx = {
  width: 22,
  height: 22,
  flexShrink: 0,
  borderRadius: '6px',
  color: 'text.secondary',
  '&:hover': { color: 'text.primary', bgcolor: 'action.hover' },
} as const

const RIGHT_MIN = 300
const RIGHT_MAX = 900
const BOTTOM_MIN = 120
const BOTTOM_MAX = 720

function tabLabel(tab: DockTab): string {
  return tab.kind === 'custom' ? (tab.panelName ?? 'Panel') : VIEW_LABELS[tab.kind]
}

function TabIcon({ kind, size = 12 }: { kind: DockViewKind; size?: number }) {
  const Icon = kind === 'custom' ? LayerLinear : VIEW_ICONS[kind]
  return <Icon size={size} color="currentColor" />
}

/** Renders one tab's view. Terminals stay mounted for the dock's whole life
 * (see `Dock` below) so their PTY survives switching tabs; everything else is
 * mounted only while active. */
function DockViewBody({ tab, onClose }: { tab: DockTab; onClose: () => void }) {
  switch (tab.kind) {
    case 'review':
      return <ReviewPanel onClose={onClose} />
    case 'files':
      return <WorkspaceTree />
    case 'preview':
      return <PreviewPanel />
    case 'tasks':
      return <BackgroundTasksPanel />
    case 'multiplayer':
      return <MultiplayerWorkspacePanel />
    case 'terminal':
      // Unreachable in practice — `Dock` renders terminals from its
      // always-mounted list instead, so their PTY survives a tab switch.
      return <TerminalPanel />
    case 'custom':
      return <CustomPanel id={tab.panelName ?? null} />
  }
}

/**
 * A dock — the right sidebar or the bottom bar — as a tabbed container.
 *
 * Both docks are the same component and can host the same views, so a
 * terminal can sit in the sidebar and a file tree along the bottom. This
 * replaces the previous model where the right side showed exactly one view at
 * a time (switching discarded whatever was already there) and the bottom was
 * hardcoded to a single terminal.
 *
 * Styled as a floating card on the shared `surfaceCard` treatment — same
 * glass fill and corner radius as the sidebar and Settings dialog, inset so
 * the chrome behind it reads as one continuous surface.
 */
export function Dock({ dock }: { dock: DockId }) {
  const theme = useTheme()
  const state = usePanelStore((s) => s[dock])
  const setDockSize = usePanelStore((s) => s.setDockSize)
  const setActiveTab = usePanelStore((s) => s.setActiveTab)
  const closeTab = usePanelStore((s) => s.closeTab)
  const addTab = usePanelStore((s) => s.addTab)
  const closeDock = usePanelStore((s) => s.closeDock)
  const setPanelResizing = usePanelStore((s) => s.setPanelResizing)
  const playbooks = usePlaybookStore((s) => s.playbooks)
  const panelRef = useRef<HTMLDivElement>(null)
  const [addAnchor, setAddAnchor] = useState<HTMLElement | null>(null)

  const isRight = dock === 'right'
  // Filtered outside the selector — `.filter()` inside one returns a fresh
  // array every read and re-renders on any store change.
  const customPanels = playbooks.filter(isPanel)

  // Terminals are kept mounted even while another tab is active: unmounting
  // one kills its PTY, losing shell state, cwd and scrollback.
  const terminalTabs = useMemo(() => state.tabs.filter((t) => t.kind === 'terminal'), [state.tabs])
  const active = state.tabs.find((t) => t.id === state.activeId) ?? null

  if (!state.open || state.tabs.length === 0) return null

  const addableViews = (Object.keys(VIEW_LABELS) as Exclude<DockViewKind, 'custom'>[]).map((kind) => ({
    kind,
    label: VIEW_LABELS[kind],
  }))

  return (
    <Box
      ref={panelRef}
      sx={{
        position: 'relative',
        flexShrink: 0,
        // Only the docked axis is sized; the cross axis is left to the parent
        // flex container's default `stretch`. An explicit 100% there would be
        // 100% of the parent BEFORE margins, overflowing by exactly the inset.
        ...(isRight ? { width: state.size } : { height: state.size }),
        // Inset so the card floats on the chrome behind it, matching how the
        // Settings dialog sits over the app.
        m: 1,
        ...(isRight ? { ml: 0 } : { mt: 0 }),
      }}
    >
      <ResizeHandle
        edge={isRight ? 'left' : 'top'}
        size={state.size}
        min={isRight ? RIGHT_MIN : BOTTOM_MIN}
        max={isRight ? RIGHT_MAX : BOTTOM_MAX}
        onResize={(n) => setDockSize(dock, n)}
        targetRef={panelRef}
        onDragActiveChange={setPanelResizing}
      />
      <Stack
        sx={{
          height: '100%',
          minHeight: 0,
          overflow: 'hidden',
          ...surfaceCard(theme.palette.mode),
        }}
      >
        {/* Tab strip. Fixed height so both docks' headers line up with each
            other and with the title bar, rather than each sizing to whatever
            its tallest control happens to be. */}
        <Stack
          direction="row"
          sx={{
            alignItems: 'center',
            gap: '2px',
            height: 34,
            flexShrink: 0,
            px: 0.5,
            borderBottom: 1,
            borderColor: 'divider',
          }}
        >
          <Stack
            direction="row"
            sx={{
              alignItems: 'center',
              gap: '2px',
              flex: 1,
              minWidth: 0,
              overflowX: 'auto',
              // A visible scrollbar inside a 34px strip eats half its height.
              scrollbarWidth: 'none',
              '&::-webkit-scrollbar': { display: 'none' },
            }}
          >
            {state.tabs.map((tab) => {
              const isActive = tab.id === state.activeId
              return (
                <Stack
                  key={tab.id}
                  direction="row"
                  onClick={() => setActiveTab(dock, tab.id)}
                  sx={{
                    alignItems: 'center',
                    gap: 0.75,
                    height: 24,
                    pl: 1,
                    pr: 0.5,
                    borderRadius: '7px',
                    cursor: 'pointer',
                    flexShrink: 0,
                    maxWidth: 200,
                    color: isActive ? 'text.primary' : 'text.secondary',
                    bgcolor: isActive ? 'action.selected' : 'transparent',
                    transition: 'background-color 0.12s, color 0.12s',
                    '&:hover': { bgcolor: 'action.hover', color: 'text.primary' },
                    '&:hover .tab-close': { opacity: 0.65 },
                  }}
                >
                  <TabIcon kind={tab.kind} />
                  <Typography
                    variant="caption"
                    noWrap
                    sx={{ fontSize: '0.75rem', fontWeight: isActive ? 600 : 500, lineHeight: 1 }}
                  >
                    {tabLabel(tab)}
                  </Typography>
                  <Box
                    component="span"
                    className="tab-close"
                    role="button"
                    aria-label="Close tab"
                    onClick={(e) => {
                      e.stopPropagation()
                      closeTab(dock, tab.id)
                    }}
                    sx={{
                      display: 'flex',
                      alignItems: 'center',
                      justifyContent: 'center',
                      width: 16,
                      height: 16,
                      borderRadius: '5px',
                      flexShrink: 0,
                      color: 'inherit',
                      // Reserved even when hidden, so the label doesn't shift
                      // sideways the moment the pointer enters the tab.
                      opacity: isActive ? 0.65 : 0,
                      transition: 'opacity 0.12s, background-color 0.12s',
                      '&:hover': { opacity: 1, bgcolor: 'action.selected' },
                    }}
                  >
                    <CloseLinear size={11} color="currentColor" />
                  </Box>
                </Stack>
              )
            })}

            <Tooltip title="Add a view">
              <IconButton size="small" onClick={(e) => setAddAnchor(e.currentTarget)} sx={headerBtnSx}>
                <AddLinear size={13} color="currentColor" />
              </IconButton>
            </Tooltip>
          </Stack>

          <Tooltip title={isRight ? 'Hide sidebar' : 'Hide bottom bar'}>
            <IconButton size="small" onClick={() => closeDock(dock)} sx={{ ...headerBtnSx, ml: 0.5 }}>
              <CloseLinear size={13} color="currentColor" />
            </IconButton>
          </Tooltip>
        </Stack>

        <Menu anchorEl={addAnchor} open={!!addAnchor} onClose={() => setAddAnchor(null)}>
          {addableViews.map((v) => (
            <MenuItem
              key={v.kind}
              onClick={() => {
                addTab(dock, v.kind)
                setAddAnchor(null)
              }}
              sx={{ gap: 1, fontSize: '0.8125rem' }}
            >
              <TabIcon kind={v.kind} size={14} />
              {v.label}
            </MenuItem>
          ))}
          {customPanels.length > 0 && <Divider />}
          {customPanels.map((p) => (
            <MenuItem
              key={p.name}
              onClick={() => {
                addTab(dock, 'custom', p.name)
                setAddAnchor(null)
              }}
              sx={{ gap: 1, fontSize: '0.8125rem' }}
            >
              <LayerLinear size={14} color="currentColor" />
              {p.name}
            </MenuItem>
          ))}
        </Menu>

        {/* Body. Terminals render always (hidden when inactive) so their PTY
            survives; every other view mounts only while it's the active tab. */}
        <Box sx={{ flex: 1, minHeight: 0, minWidth: 0, position: 'relative', display: 'flex' }}>
          {terminalTabs.map((tab) => (
            <Box
              key={tab.id}
              sx={{
                position: 'absolute',
                inset: 0,
                display: 'flex',
                visibility: tab.id === state.activeId ? 'visible' : 'hidden',
                pointerEvents: tab.id === state.activeId ? undefined : 'none',
              }}
            >
              {/* No `attachTabId`: that means "attach to an existing PTY" and
                  skips both the spawn and the close-on-unmount. A dock tab
                  owns its shell, so it lets TerminalPanel generate its own id
                  — keyed by tab id so each tab gets exactly one. */}
              <TerminalPanel key={tab.id} />
            </Box>
          ))}
          {active && active.kind !== 'terminal' && (
            <Box sx={{ position: 'absolute', inset: 0, display: 'flex', minWidth: 0 }}>
              <DockViewBody tab={active} onClose={() => closeTab(dock, active.id)} />
            </Box>
          )}
        </Box>
      </Stack>
    </Box>
  )
}
