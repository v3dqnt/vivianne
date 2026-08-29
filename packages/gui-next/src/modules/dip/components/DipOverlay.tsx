import { useEffect, useState } from 'react'
import Box from '@mui/material/Box'
import Typography from '@mui/material/Typography'
import { on } from '../../../ipc'

interface OverlayBox {
  x: number
  y: number
  w: number
  h: number
  name: string
  role: string
}

/** The `dip_overlay` window's entire UI — a transparent, click-through,
 * always-on-top layer the backend positions/sizes to exactly cover the
 * window `dip_snapshot` just read, and briefly draws a highlight box over
 * every element it found. Self-hides after the given duration, same
 * self-managed-window pattern the voice notch window uses. Purely visual:
 * nothing here is interactive (the window is click-through from the moment
 * it's created — see `lib.rs`'s `set_ignore_cursor_events` call). */
export function DipOverlay() {
  const [boxes, setBoxes] = useState<OverlayBox[]>([])

  useEffect(() => {
    let unlisten: (() => void) | null = null
    let hideTimer: number | null = null

    void on<{ boxes: OverlayBox[]; durationMs: number }>('dip_overlay_show', ({ boxes, durationMs }) => {
      setBoxes(boxes)
      if (hideTimer !== null) window.clearTimeout(hideTimer)
      hideTimer = window.setTimeout(() => {
        setBoxes([])
        void (async () => {
          try {
            const { getCurrentWebviewWindow } = await import('@tauri-apps/api/webviewWindow')
            await getCurrentWebviewWindow().hide()
          } catch (e) {
            console.error('[dip-overlay] hide failed', e)
          }
        })()
      }, durationMs)
    }).then((fn) => {
      unlisten = fn
    })

    return () => {
      unlisten?.()
      if (hideTimer !== null) window.clearTimeout(hideTimer)
    }
  }, [])

  return (
    <Box sx={{ position: 'fixed', inset: 0, bgcolor: 'transparent', overflow: 'hidden' }}>
      {boxes.map((b, i) => (
        <Box
          key={i}
          sx={{
            position: 'absolute',
            left: b.x,
            top: b.y,
            width: Math.max(b.w, 1),
            height: Math.max(b.h, 1),
            border: '2px solid #4a9eff',
            borderRadius: 0.5,
            bgcolor: 'rgba(74,158,255,0.12)',
            boxShadow: '0 0 0 1px rgba(0,0,0,0.4)',
            pointerEvents: 'none',
          }}
        >
          <Typography
            variant="caption"
            sx={{
              position: 'absolute',
              top: -18,
              left: 0,
              px: 0.5,
              py: 0.125,
              bgcolor: '#4a9eff',
              color: '#fff',
              fontSize: '0.625rem',
              fontWeight: 600,
              borderRadius: 0.5,
              whiteSpace: 'nowrap',
              maxWidth: 200,
              overflow: 'hidden',
              textOverflow: 'ellipsis',
            }}
          >
            {b.name || b.role}
          </Typography>
        </Box>
      ))}
    </Box>
  )
}
