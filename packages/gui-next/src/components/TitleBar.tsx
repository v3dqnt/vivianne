import { useEffect, useState } from 'react'
import Box from '@mui/material/Box'
import Typography from '@mui/material/Typography'
import { hasTauri } from '../ipc'

const TRAFFIC_LIGHT_SIZE = 12

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

      {/* Balances the traffic-light cluster so the centered title is actually centered. */}
      <Box sx={{ width: 64, flexShrink: 0 }} />
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
