import { useEffect, useState } from 'react'
import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import IconButton from '@mui/material/IconButton'
import Tooltip from '@mui/material/Tooltip'
import Menu from '@mui/material/Menu'
import MenuItem from '@mui/material/MenuItem'
import { hasTauri } from '../ipc'
import { FolderLinear, CommandSquareLinear } from '../icons'
import { usePanelStore } from '../modules/dock/store/panelStore'
import { usePlaybookStore } from '../modules/settings/store/playbookStore'
import { useProjectsStore } from '../modules/sessions/store/projectsStore'

const TRAFFIC_LIGHT_SIZE = 12

/** Panel toggles + project shortcuts, right-aligned in the title bar. These
 * used to be a permanent vertical icon rail pinned to the window's right
 * edge; moving them here gives that column back to actual content. */
function TitleBarActions() {
  const rightOpen = usePanelStore((s) => s.right.open)
  const bottomOpen = usePanelStore((s) => s.bottom.open)
  const toggleDock = usePanelStore((s) => s.toggleDock)
  // Panel playbooks are listed in each dock's own "add a view" menu now, but
  // this is still where they get loaded — the docks read the same store.
  const refreshCustomPanels = usePlaybookStore((s) => s.refresh)
  const openExternal = useProjectsStore((s) => s.openExternal)
  const activeProjectId = useProjectsStore((s) => s.activeProjectId)
  const projects = useProjectsStore((s) => s.projects)
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

  useEffect(() => {
    void refreshCustomPanels()
  }, [refreshCustomPanels])

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

      {/* Two dock toggles, replacing the old terminal button + Panels menu.
          Choosing WHICH views are open now lives inside each dock's own tab
          strip (see `Dock.tsx`), so the title bar only has to expand and
          collapse them. */}
      <Tooltip title={bottomOpen ? 'Hide bottom bar' : 'Show bottom bar'}>
        <IconButton size="small" onClick={() => toggleDock('bottom')} sx={iconSx(bottomOpen)}>
          <PanelBottomGlyph />
        </IconButton>
      </Tooltip>

      <Tooltip title={rightOpen ? 'Hide right sidebar' : 'Show right sidebar'}>
        <IconButton size="small" onClick={() => toggleDock('right')} sx={iconSx(rightOpen)}>
          <PanelRightGlyph />
        </IconButton>
      </Tooltip>
    </Stack>
  )
}

/** Right-sidebar glyph — a rounded rect with its right column filled. No icon
 * in the set draws a dock-panel affordance. */
function PanelRightGlyph() {
  return (
    <Box component="svg" viewBox="0 0 24 24" sx={{ width: 14, height: 14, fill: 'none', stroke: 'currentColor', strokeWidth: 1.8 }}>
      <rect x="3" y="4" width="18" height="16" rx="2.5" />
      <path d="M15 4v16" />
      <rect x="15" y="4" width="6" height="16" rx="2.5" fill="currentColor" stroke="none" opacity="0.35" />
    </Box>
  )
}

/** Bottom-bar glyph — same rect, bottom row filled. */
function PanelBottomGlyph() {
  return (
    <Box component="svg" viewBox="0 0 24 24" sx={{ width: 14, height: 14, fill: 'none', stroke: 'currentColor', strokeWidth: 1.8 }}>
      <rect x="3" y="4" width="18" height="16" rx="2.5" />
      <path d="M3 14h18" />
      <rect x="3" y="14" width="18" height="6" rx="2.5" fill="currentColor" stroke="none" opacity="0.35" />
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
    let timer: ReturnType<typeof setTimeout> | undefined
    ;(async () => {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      const win = getCurrentWindow()
      setIsMaximized(await win.isMaximized())
      // Resize is the correct signal for maximize/restore — Tauri has no
      // dedicated "maximize changed" event, and polling on a window `resize`
      // listener (rather than the DOM's) fires exactly when it actually
      // could have changed, not on some fixed timer.
      // Dragging a window edge floods that listener with events, so the IPC
      // round-trip is debounced to one call ~150ms after the last tick.
      unlisten = await win.onResized(() => {
        clearTimeout(timer)
        timer = setTimeout(() => {
          void win.isMaximized().then(setIsMaximized)
        }, 150)
      })
    })()
    return () => {
      unlisten?.()
      clearTimeout(timer)
    }
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
        // Same surface as the rail/sidebar below, with no dividing border —
        // together they read as one continuous piece of chrome wrapping the
        // content area, which curves away from it (see App.tsx's chat column
        // and its rounded top-left corner).
        bgcolor: 'background.chrome',
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
