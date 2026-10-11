import { useEffect, useState } from 'react'
import { Box, Stack, Typography, IconButton, Tooltip, Menu, MenuItem, Divider, ListItemIcon } from '../ui'
import { call, hasTauri } from '../ipc'
import { fileManagerName, terminalAppName } from '../lib/platform'
import { FolderLinear, CommandSquareLinear, TickCircleLinear, PanelsLinear, LayoutLinear, SidebarLinear } from '../icons'
import { useSidebarStore } from '../modules/sessions/store/sidebarStore'
import { PRESET_LABELS, ZONE_LABELS, applyPreset, resetLayout, toggleZone, useWorkspace, type Preset, type Zone } from '../modules/workspace/layout'
import { useProjectsStore } from '../modules/sessions/store/projectsStore'
import { PairChip } from '../modules/multiplayer/components/PairChip'

const TRAFFIC_LIGHT_SIZE = 12

/** Panel toggles + project shortcuts, right-aligned in the title bar. These
 * used to be a permanent vertical icon rail pinned to the window's right
 * edge; moving them here gives that column back to actual content. */
function TitleBarActions() {
  const zones = useWorkspace((s) => s.zones)
  const [panelsAnchor, setPanelsAnchor] = useState<HTMLElement | null>(null)
  const [layoutAnchor, setLayoutAnchor] = useState<HTMLElement | null>(null)
  const openExternal = useProjectsStore((s) => s.openExternal)
  const activeProjectId = useProjectsStore((s) => s.activeProjectId)
  const projects = useProjectsStore((s) => s.projects)
  const [openAnchor, setOpenAnchor] = useState<HTMLElement | null>(null)

  const hasFolder = !!projects.find((p) => p.id === activeProjectId)?.path
  const noFolderHint = hasFolder ? '' : ' (set a folder on this project first)'

  // Terax header buttons: 28px, rounded-md, muted at rest, ink on hover.
  const iconSx = (active: boolean) => ({
    width: 28,
    height: 28,
    borderRadius: 0.8,
    color: active ? 'text.primary' : 'text.secondary',
    bgcolor: active ? 'action.selected' : 'transparent',
    transition: 'background-color 0.15s, color 0.15s',
    '&:hover': { bgcolor: 'action.hover', color: 'text.primary' },
  })

  return (
    <Stack direction="row" sx={{ alignItems: 'center', gap: 0.25, flexShrink: 0, pr: 0.5, zIndex: 1 }}>
      <PairChip />
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
          Open in {fileManagerName}
        </MenuItem>
        <MenuItem
          onClick={() => {
            void openExternal('terminal')
            setOpenAnchor(null)
          }}
          sx={{ gap: 1, fontSize: '0.8125rem' }}
        >
          <CommandSquareLinear size={14} color="currentColor" />
          Open in {terminalAppName}
        </MenuItem>
      </Menu>

      {/* Panels live wherever the user docks them now, so the title bar no
          longer toggles "the right dock" — it shows or hides panels by name,
          and offers starting arrangements. */}
      <Tooltip title="Panels">
        <IconButton size="small" aria-label="Panels" onClick={(e) => setPanelsAnchor(e.currentTarget)} sx={iconSx(!!panelsAnchor)}>
          <PanelsLinear size={14} color="currentColor" />
        </IconButton>
      </Tooltip>
      {/* Just the two panels. What goes IN them is chosen from each panel's
          own "+" and its empty state, where you are already looking. */}
      <Menu anchorEl={panelsAnchor} open={!!panelsAnchor} onClose={() => setPanelsAnchor(null)}>
        {(['right', 'bottom'] as Zone[]).map((zone) => (
          <MenuItem
            key={zone}
            onClick={() => {
              toggleZone(zone)
              setPanelsAnchor(null)
            }}
            sx={{ gap: 1, fontSize: '0.8125rem', minWidth: 190 }}
          >
            <Box sx={{ flex: 1 }}>{ZONE_LABELS[zone]}</Box>
            {zones[zone] && (
              <ListItemIcon sx={{ minWidth: 0, color: 'primary.main' }}>
                <TickCircleLinear size={13} color="currentColor" />
              </ListItemIcon>
            )}
          </MenuItem>
        ))}
      </Menu>

      <Tooltip title="Layout">
        <IconButton size="small" aria-label="Layout" onClick={(e) => setLayoutAnchor(e.currentTarget)} sx={iconSx(!!layoutAnchor)}>
          <LayoutLinear size={14} color="currentColor" />
        </IconButton>
      </Tooltip>
      <Menu anchorEl={layoutAnchor} open={!!layoutAnchor} onClose={() => setLayoutAnchor(null)}>
        <Typography variant="caption" color="text.secondary" sx={{ display: 'block', px: 2, pt: 0.5, pb: 0.75, maxWidth: 260 }}>
          Start from an arrangement — then drag tabs anywhere to rearrange.
        </Typography>
        {(Object.keys(PRESET_LABELS) as Preset[]).map((preset) => (
          <MenuItem
            key={preset}
            onClick={() => {
              applyPreset(preset)
              setLayoutAnchor(null)
            }}
            sx={{ fontSize: '0.8125rem' }}
          >
            {PRESET_LABELS[preset]}
          </MenuItem>
        ))}
        <Divider />
        <MenuItem
          onClick={() => {
            resetLayout()
            setLayoutAnchor(null)
          }}
          sx={{ fontSize: '0.8125rem', color: 'text.secondary' }}
        >
          Reset to default
        </MenuItem>
      </Menu>
    </Stack>
  )
}

/**
 * Custom title bar for the frameless window (`decorations: false` in
 * tauri.conf.json — there is no OS chrome to replace, this IS the chrome).
 * Styled as macOS traffic lights regardless of host OS, since that's the
 * explicit design choice here, not an OS-detection branch. On macOS this
 * happens to match the platform's own controls, which is why the native
 * ones stay off (`decorations: false`) there too rather than being
 * re-enabled — two sets of traffic lights would be worse than one.
 */
/** Folds the project sidebar away (Ctrl+B), for more room to work. */
function SidebarToggle() {
  const collapsed = useSidebarStore((s) => s.collapsed)
  const toggle = useSidebarStore((s) => s.toggle)
  return (
    <Tooltip title={collapsed ? 'Show sidebar (Ctrl+B)' : 'Hide sidebar (Ctrl+B)'}>
      <IconButton
        size="small"
        aria-label={collapsed ? 'Show sidebar' : 'Hide sidebar'}
        aria-pressed={!collapsed}
        onClick={toggle}
        sx={{ ml: 1.25, width: 28, height: 28, borderRadius: 0.8, color: 'text.secondary', '&:hover': { bgcolor: 'action.hover', color: 'text.primary' } }}
      >
        <SidebarLinear size={16} color="currentColor" />
      </IconButton>
    </Tooltip>
  )
}

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

  // Closing the MAIN window hides it instead of destroying it, so Vivianne
  // keeps running in the tray and Viv Instant's global chord still works —
  // and so the main window's provider dispatcher, which answers every LLM
  // call Viv Instant makes, stays alive. Secondary windows still really close:
  // nothing depends on them surviving.
  const close = withWindow(async (win) => {
    if (win.label === 'main') {
      await call('cmd_hide_to_background', {})
    } else {
      await win.close()
    }
  })
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
        // Terax's header: 40px.
        height: 40,
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
        <SidebarToggle />
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
