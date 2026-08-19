import { useEffect, useRef, useState } from 'react'
import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import Button from '@mui/material/Button'
import Switch from '@mui/material/Switch'
import TextField from '@mui/material/TextField'
import CircularProgress from '@mui/material/CircularProgress'
import { CameraLinear, PlayCircleLinear, StopCircleLinear } from '../../../icons'
import { useGestureStore } from '../store/gestureStore'
import type { GestureEvent } from '../lib/types'

// 21-point MediaPipe hand topology — pairs of landmark indices to draw as
// skeleton lines, purely for the calibration overlay.
const HAND_CONNECTIONS: [number, number][] = [
  [0, 1], [1, 2], [2, 3], [3, 4], // thumb
  [0, 5], [5, 6], [6, 7], [7, 8], // index
  [5, 9], [9, 10], [10, 11], [11, 12], // middle
  [9, 13], [13, 14], [14, 15], [15, 16], // ring
  [13, 17], [17, 18], [18, 19], [19, 20], // pinky
  [0, 17], // palm base
]

function describeEvent(e: GestureEvent): string {
  switch (e.type) {
    case 'hand_detected': return 'Hand detected'
    case 'hand_lost': return 'Hand lost'
    case 'pinch_start': return `Pinch start (${e.payload.x.toFixed(2)}, ${e.payload.y.toFixed(2)})`
    case 'pinch_end': return `Pinch end (${e.payload.x.toFixed(2)}, ${e.payload.y.toFixed(2)})`
    case 'click': return `Click (${e.payload.x.toFixed(2)}, ${e.payload.y.toFixed(2)})`
    case 'adjust': return `Adjust ${e.payload.delta > 0 ? '+' : ''}${e.payload.delta.toFixed(3)}`
    case 'swipe': return `Swipe ${e.payload.direction}`
    case 'open_palm_hold': return 'Open-palm hold'
    case 'pointer_move': return '' // excluded from the log — fires at frame rate
  }
}

/** Calibration/debug view: live landmark skeleton overlay + a running log of
 * classified gestures, so thresholds in `GestureConfig` can be tuned against
 * what's actually being detected instead of guessing blind. */
export function GestureCalibrationPanel() {
  const running = useGestureStore((s) => s.running)
  const debug = useGestureStore((s) => s.debug)
  const cameraIndex = useGestureStore((s) => s.cameraIndex)
  const lastDebugFrame = useGestureStore((s) => s.lastDebugFrame)
  const pointer = useGestureStore((s) => s.pointer)
  const pinching = useGestureStore((s) => s.pinching)
  const recentEvents = useGestureStore((s) => s.recentEvents)
  const sidecarLog = useGestureStore((s) => s.sidecarLog)
  const start = useGestureStore((s) => s.start)
  const stop = useGestureStore((s) => s.stop)
  const setDebug = useGestureStore((s) => s.setDebug)
  const init = useGestureStore((s) => s.init)

  const [camIdx, setCamIdx] = useState(cameraIndex)
  const [busy, setBusy] = useState(false)
  const canvasRef = useRef<HTMLCanvasElement>(null)

  useEffect(() => {
    void init()
  }, [init])

  // Redraw the skeleton whenever a new debug frame lands. Landmarks are
  // normalized (0-1) so this scales to whatever the canvas element's actual
  // size is — no camera resolution assumptions.
  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const ctx = canvas.getContext('2d')
    if (!ctx) return
    const { width, height } = canvas
    ctx.clearRect(0, 0, width, height)

    if (!lastDebugFrame?.tracking || lastDebugFrame.landmarks.length === 0) return
    const pts = lastDebugFrame.landmarks.map((l) => ({ x: l.x * width, y: l.y * height }))

    ctx.strokeStyle = 'rgba(74, 158, 255, 0.8)'
    ctx.lineWidth = 2
    for (const [a, b] of HAND_CONNECTIONS) {
      if (!pts[a] || !pts[b]) continue
      ctx.beginPath()
      ctx.moveTo(pts[a].x, pts[a].y)
      ctx.lineTo(pts[b].x, pts[b].y)
      ctx.stroke()
    }
    ctx.fillStyle = '#4a9eff'
    for (const p of pts) {
      ctx.beginPath()
      ctx.arc(p.x, p.y, 3.5, 0, Math.PI * 2)
      ctx.fill()
    }

    // Virtual-cursor circle — the index fingertip position, drawn distinct
    // from the skeleton dots (bigger, glowing, color flips on pinch) so it
    // reads as "the pointer" rather than just another landmark.
    if (pointer) {
      const cx = pointer.x * width
      const cy = pointer.y * height
      const color = pinching ? '#ff6b4a' : '#ffffff'
      ctx.save()
      ctx.shadowColor = color
      ctx.shadowBlur = 14
      ctx.strokeStyle = color
      ctx.lineWidth = 2.5
      ctx.beginPath()
      ctx.arc(cx, cy, pinching ? 9 : 12, 0, Math.PI * 2)
      ctx.stroke()
      if (pinching) {
        ctx.fillStyle = color
        ctx.globalAlpha = 0.35
        ctx.beginPath()
        ctx.arc(cx, cy, 9, 0, Math.PI * 2)
        ctx.fill()
      }
      ctx.restore()
    }
  }, [lastDebugFrame, pointer, pinching])

  const toggle = async () => {
    setBusy(true)
    try {
      if (running) await stop()
      else await start(camIdx)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Stack sx={{ gap: 1.5 }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
        <CameraLinear size={16} color="currentColor" />
        <Typography variant="subtitle2" sx={{ fontWeight: 600, flex: 1 }}>
          Hand-gesture control
        </Typography>
      </Stack>
      <Typography variant="caption" color="text.secondary">
        Pinch to click, rotate a closed fist to adjust a value, swipe to switch threads, hold an open palm to
        move between panels. Runs a local MediaPipe process — the camera feed never leaves your machine.
      </Typography>

      <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
        <TextField
          label="Camera index"
          size="small"
          type="number"
          value={camIdx}
          onChange={(e) => setCamIdx(Math.max(0, Number(e.target.value) || 0))}
          disabled={running}
          sx={{ width: 140 }}
        />
        <Button
          size="small"
          variant={running ? 'outlined' : 'contained'}
          color={running ? 'error' : 'primary'}
          onClick={() => void toggle()}
          disabled={busy}
          startIcon={busy ? <CircularProgress size={14} /> : running ? <StopCircleLinear size={15} color="currentColor" /> : <PlayCircleLinear size={15} color="currentColor" />}
          sx={{ textTransform: 'none' }}
        >
          {running ? 'Stop' : 'Start'}
        </Button>
        <Box sx={{ flex: 1 }} />
        <Typography variant="caption" color="text.secondary">Calibration overlay</Typography>
        <Switch size="small" checked={debug} onChange={(e) => void setDebug(e.target.checked)} disabled={!running && !debug} />
      </Stack>

      {debug && (
        <Box sx={{ position: 'relative', width: '100%', aspectRatio: '4 / 3', bgcolor: '#0d1117', borderRadius: 1.5, overflow: 'hidden', border: 1, borderColor: 'divider' }}>
          <canvas ref={canvasRef} width={640} height={480} style={{ width: '100%', height: '100%' }} />
          {!lastDebugFrame?.tracking && (
            <Typography variant="caption" color="text.secondary" sx={{ position: 'absolute', top: 8, left: 8 }}>
              {running ? 'No hand detected' : 'Not running'}
            </Typography>
          )}
        </Box>
      )}

      {recentEvents.length > 0 && (
        <Stack sx={{ gap: 0.25, maxHeight: 160, overflowY: 'auto' }}>
          <Typography variant="caption" sx={{ fontWeight: 600, color: 'text.secondary', textTransform: 'uppercase', letterSpacing: 0.4, fontSize: '0.625rem' }}>
            Recent gestures
          </Typography>
          {recentEvents.map((e, i) => (
            <Typography key={i} variant="caption" sx={{ fontFamily: '"JetBrains Mono", ui-monospace, monospace', fontSize: '0.6875rem', color: 'text.secondary' }}>
              {describeEvent(e.event)}
            </Typography>
          ))}
        </Stack>
      )}

      {sidecarLog.length > 0 && (
        <Stack sx={{ gap: 0.25, maxHeight: 100, overflowY: 'auto', bgcolor: 'action.hover', borderRadius: 1, p: 1 }}>
          {sidecarLog.map((line, i) => (
            <Typography key={i} variant="caption" sx={{ fontFamily: '"JetBrains Mono", ui-monospace, monospace', fontSize: '0.625rem', color: 'text.disabled' }}>
              {line}
            </Typography>
          ))}
        </Stack>
      )}
    </Stack>
  )
}
