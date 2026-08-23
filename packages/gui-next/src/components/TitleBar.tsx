import { useEffect, useState } from 'react'
import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import IconButton from '@mui/material/IconButton'
import Tooltip from '@mui/material/Tooltip'
import Menu from '@mui/material/Menu'
import MenuItem from '@mui/material/MenuItem'
import { hasTauri } from '../ipc'
import { FolderLinear, CommandSquareLinear, LayerLinear, TaskLinear, MonitorLinear, GithubLinear, DocumentTextLinear } from '../icons'
import { usePanelStore, RIGHT_PANEL_LABELS, type RightPanelKind } from '../modules/dock/store/panelStore'
import { useProjectsStore } from '../modules/sessions/store/projectsStore'

const TRAFFIC_LIGHT_SIZE = 12

const RIGHT_PANEL_ICONS: Record<RightPanelKind, typeof FolderLinear> = {
  review: GithubLinear,
  files: DocumentTextLinear,
  preview: MonitorLinear,
  tasks: TaskLinear,
}

/** Panel toggles + project shortcuts, right-aligned in the title bar. These
 * used to be a permanent vertical icon rail pinned to the window's right
 * edge; moving them here gives that column back to actual content. */
function TitleBarActions() {
  const rightOpen = usePanelStore((s) => s.rightOpen)
  const rightKind = usePanelStore((s) => s.rightKind)
  const toggleRight = usePanelStore((s) => s.toggleRight)
  const bottomOpen = usePanelStore((s) => s.bottomOpen)
  const toggleBottom = usePanelStore((s) => s.toggleBottom)
  const openExternal = useProjectsStore((s) => s.openExternal)
  const activeProjectId = useProjectsStore((s) => s.activeProjectId)
  const projects = useProjectsStore((s) => s.projects)
  const [panelAnchor, setPanelAnchor] = useState<HTMLElement | null>(null)
  const [openAnchor, setOpenAnchor] = useState<HTMLElement | null>(null)

  const hasFolder = !!projects.find((p) => p.id === activeProjectId)?.path
  const noFolderHint = hasFolder ? '' : ' (set a folder on this project first)'

  const iconSx = (active: boolean) => ({
    width: 26,
    height: 26,
    borderRadius: 1.5,
    color: active ? 'primary.main' : 'text.secondary',
    bgcolor: active ? 'action.selected' : 'transparent',
    transition: 'background-color 0.15s, color 0.15s',
    '&:hover': { bgcolor: 'action.hover', color: 'text.primary' },
  })

  return (
    <Stack direction="row" sx={{ alignItems: 'center', gap: 0.25, flexShrink: 0, pr: 0.5, zIndex: 1 }}>
      <Tooltip title={`Open project folder${noFolderHint}`}>
        <span>
          <IconButton size="small" disabled={!hasFolder} onClick={(e) => setOpenAnchor(e.currentTarget)} sx={iconSx(false)}>
            <FolderLinear size={14} color="currentColor" />
          </IconButton>
        </span>
      </Tooltip>
      <Menu anchorEl={openAnchor} open={!!openAnchor} onClose={() => setOpenAnchor(null)}>
        <MenuItem
          onClick={() => {
            void openExternal('explorer')
            setOpenAnchor(null)
          }}
          sx={{ gap: 1, fontSize: '0.8125rem' }}
        >
          <FolderLinear size={14} color="currentColor" />
          Open in File Explorer
        </MenuItem>
        <MenuItem
          onClick={() => {
            void openExternal('terminal')
            setOpenAnchor(null)
          }}
          sx={{ gap: 1, fontSize: '0.8125rem' }}
        >
          <CommandSquareLinear size={14} color="currentColor" />
          Open in PowerShell
        </MenuItem>
      </Menu>

      <Tooltip title="Terminal panel">
        <IconButton size="small" onClick={toggleBottom} sx={iconSx(bottomOpen)}>
          <CommandLinearGlyph />
        </IconButton>
      </Tooltip>

      <Tooltip title="Panels">
        <IconButton size="small" onClick={(e) => setPanelAnchor(e.currentTarget)} sx={iconSx(rightOpen)}>
          <LayerLinear size={14} color="currentColor" />
        </IconButton>
      </Tooltip>
      <Menu anchorEl={panelAnchor} open={!!panelAnchor} onClose={() => setPanelAnchor(null)}>
        {(Object.keys(RIGHT_PANEL_LABELS) as RightPanelKind[]).map((kind) => {
          const Icon = RIGHT_PANEL_ICONS[kind]
          return (
            <MenuItem
              key={kind}
              selected={rightOpen && rightKind === kind}
              onClick={() => {
                toggleRight(kind)
                setPanelAnchor(null)
              }}
              sx={{ gap: 1, fontSize: '0.8125rem' }}
            >
              <Icon size={14} color="currentColor" />
              {RIGHT_PANEL_LABELS[kind]}
            </MenuItem>
          )
        })}
      </Menu>
    </Stack>
  )
}

/** Terminal glyph — a `>_` prompt, which no icon in the set draws. */
function CommandLinearGlyph() {
  return (
    <Box component="svg" viewBox="0 0 24 24" sx={{ width: 14, height: 14, fill: 'none', stroke: 'currentColor', strokeWidth: 2, strokeLinecap: 'round', strokeLinejoin: 'round' }}>
      <path d="M4 6l5 6-5 6" />
      <path d="M13 18h7" />
    </Box>
  )
}

/**
 * Custom title bar for the frameless window (`decorations: false` in
 * tauri.conf.json — there is no OS chrome to replace, this IS the chrome).
 * Styled as macOS traffic lights regardless of host OS, since that's the
 * explicit design choice here, not an OS-detection branch.
 */
export function TitleBar() {
  const [isMaximized, setIsMaximized] = useState(false)

  useEffect(() => {
    if (!hasTauri()) return
    let unlisten: (() => void) | undefined
    ;(async () => {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      const win = getCurrentWindow()
      setIsMaximized(await win.isMaximized())
      // Resize is the correct signal for maximize/restore — Tauri has no
      // dedicated "maximize changed" event, and polling on a window `resize`
      // listener (rather than the DOM's) fires exactly when it actually
      // could have changed, not on some fixed timer.
      unlisten = await win.onResized(async () => {
        setIsMaximized(await win.isMaximized())
      })
    })()
    return () => unlisten?.()
  }, [])

  if (!hasTauri()) return null

  const withWindow = (fn: (win: Awaited<ReturnType<typeof import('@tauri-apps/api/window').getCurrentWindow>>) => Promise<void>) =>
    async () => {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      await fn(getCurrentWindow())
    }

  const close = withWindow((win) => win.close())
  const minimize = withWindow((win) => win.minimize())
  const toggleMaximize = withWindow((win) => win.toggleMaximize())

  return (
    <Box
      // Tauri's drag-region attribute — clicking/dragging anywhere in this
      // bar (other than the buttons, which stop propagation via their own
      // click handlers) moves the window, standard frameless-window behavior.
      data-tauri-drag-region=""
      sx={{
        position: 'relative',
        height: 32,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        flexShrink: 0,
        userSelect: 'none',
        px: 1,
        borderBottom: 1,
        borderColor: 'divider',
      }}
    >
      <Box
        className="traffic-lights"
        sx={{
          display: 'flex',
          alignItems: 'center',
          gap: '7px',
          pl: 1,
          flexShrink: 0,
          position: 'relative',
          zIndex: 1,
          '&:hover .traffic-light-glyph': { opacity: 1 },
        }}
      >
        <TrafficLight color="#ff5f56" hoverColor="#e0443e" glyphColor="#4c0002" title="Close" onClick={close}>
          <path d="M1.2 1.2 L4.8 4.8 M4.8 1.2 L1.2 4.8" />
        </TrafficLight>
        <TrafficLight color="#ffbd2e" hoverColor="#dea123" glyphColor="#5c3e00" title="Minimize" onClick={minimize}>
          <path d="M1 3 H5" />
        </TrafficLight>
        <TrafficLight
          color="#27c93f"
          hoverColor="#1aab29"
          glyphColor="#006504"
          title={isMaximized ? 'Restore' : 'Maximize'}
          onClick={toggleMaximize}
        >
          <path d="M1 5 L5 1 M1.2 2.5 V1.2 H2.5 M4.8 3.5 V4.8 H3.5" />
        </TrafficLight>
      </Box>

      <Box sx={{ position: 'absolute', inset: 0, display: 'flex', alignItems: 'center', justifyContent: 'center', pointerEvents: 'none' }}>
        <Typography variant="caption" color="text.secondary" sx={{ fontWeight: 500 }}>
          Vivianne
        </Typography>
      </Box>

      <TitleBarActions />
    </Box>
  )
}

function TrafficLight({
  color,
  hoverColor,
  glyphColor,
  title,
  onClick,
  children,
}: {
  color: string
  hoverColor: string
  glyphColor: string
  title: string
  onClick: () => void
  children: React.ReactNode
}) {
  return (
    <Box
      component="button"
      onClick={onClick}
      title={title}
      sx={{
        width: TRAFFIC_LIGHT_SIZE,
        height: TRAFFIC_LIGHT_SIZE,
        borderRadius: '50%',
        border: 'none',
        p: 0,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        cursor: 'pointer',
        bgcolor: color,
        '&:hover': { bgcolor: hoverColor },
      }}
    >
      <Box
        component="svg"
        className="traffic-light-glyph"
        viewBox="0 0 6 6"
        sx={{ width: 6, height: 6, opacity: 0, transition: 'opacity 0.1s', stroke: glyphColor, strokeWidth: 1.2, strokeLinecap: 'round', fill: 'none' }}
      >
        {children}
      </Box>
    </Box>
  )
}
