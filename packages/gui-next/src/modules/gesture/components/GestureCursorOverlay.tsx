import { useEffect, useRef, useState } from 'react'
import Box from '@mui/material/Box'
import { on } from '../../../ipc'
import { useGestureStore } from '../store/gestureStore'
import { findNearestTarget, queryInteractiveTargets, type AimTarget } from '../lib/aimAssist'
import type { GestureEvent } from '../lib/types'

/** How close (px) the raw pointer needs to be to a clickable element before
 * the cursor snaps onto it. */
const SNAP_RADIUS_PX = 55
/** How often the interactive-element list is rescanned while tracking is
 * active. Not per-frame — a full querySelectorAll + getBoundingClientRect
 * pass every frame would be wasteful, and layouts don't shift that often
 * relative to hand movement. */
const TARGET_REFRESH_MS = 350

/** App-wide virtual-cursor overlay — tracks the hand's `pointer_move`
 * position across the whole window (not just the settings calibration
 * canvas) and snaps onto nearby clickable elements ("aim assist"), the same
 * way a game reticle locks onto a target instead of requiring pixel-perfect
 * aim. A pinch (`click` gesture event) clicks whatever is currently locked,
 * or whatever is under the raw cursor if nothing is locked. */
export function GestureCursorOverlay() {
  const running = useGestureStore((s) => s.running)
  const pointer = useGestureStore((s) => s.pointer)
  const pinching = useGestureStore((s) => s.pinching)
  const init = useGestureStore((s) => s.init)

  const [locked, setLocked] = useState<AimTarget | null>(null)
  const targetsRef = useRef<AimTarget[]>([])
  const lockedRef = useRef<AimTarget | null>(null)
  const rawPxRef = useRef<{ x: number; y: number } | null>(null)

  useEffect(() => {
    void init()
  }, [init])

  // Keep the interactive-element cache warm only while tracking is active.
  useEffect(() => {
    if (!running) {
      targetsRef.current = []
      return
    }
    const refresh = () => {
      targetsRef.current = queryInteractiveTargets()
    }
    refresh()
    const interval = window.setInterval(refresh, TARGET_REFRESH_MS)
    window.addEventListener('resize', refresh)
    return () => {
      window.clearInterval(interval)
      window.removeEventListener('resize', refresh)
    }
  }, [running])

  // Recompute the snap target whenever the raw pointer moves.
  useEffect(() => {
    if (!pointer) {
      rawPxRef.current = null
      lockedRef.current = null
      setLocked(null)
      return
    }
    const x = pointer.x * window.innerWidth
    const y = pointer.y * window.innerHeight
    rawPxRef.current = { x, y }
    const nearest = findNearestTarget(x, y, targetsRef.current, SNAP_RADIUS_PX)
    lockedRef.current = nearest
    setLocked(nearest)
  }, [pointer])

  // Pinch → click. Dispatches to the locked target if aim-assist snapped
  // onto one, otherwise whatever's directly under the raw cursor position —
  // decoupled from React state via refs so this doesn't need to wait for a
  // render to see the latest lock/position.
  useEffect(() => {
    let unlisten: (() => void) | undefined
    void on<GestureEvent>('gesture_event', (event) => {
      if (event.type !== 'click') return
      const target =
        lockedRef.current?.el ??
        (rawPxRef.current ? (document.elementFromPoint(rawPxRef.current.x, rawPxRef.current.y) as HTMLElement | null) : null)
      if (target instanceof HTMLElement) {
        target.focus({ preventScroll: true })
        target.click()
      }
    }).then((fn) => {
      unlisten = fn
    })
    return () => unlisten?.()
  }, [])

  if (!running || !pointer) return null

  const color = pinching ? '#ff6b4a' : '#ffffff'

  if (locked) {
    const pad = 4
    return (
      <Box
        sx={{
          position: 'fixed',
          left: locked.rect.left - pad,
          top: locked.rect.top - pad,
          width: locked.rect.width + pad * 2,
          height: locked.rect.height + pad * 2,
          borderRadius: 1.5,
          border: `2.5px solid ${color}`,
          bgcolor: pinching ? `${color}33` : `${color}14`,
          boxShadow: `0 0 14px ${color}`,
          pointerEvents: 'none',
          zIndex: 'tooltip',
          transition: 'left 60ms ease, top 60ms ease, width 60ms ease, height 60ms ease, background-color 80ms ease',
        }}
      />
    )
  }

  return (
    <Box
      sx={{
        position: 'fixed',
        left: `${pointer.x * 100}%`,
        top: `${pointer.y * 100}%`,
        transform: 'translate(-50%, -50%)',
        width: pinching ? 18 : 24,
        height: pinching ? 18 : 24,
        borderRadius: '50%',
        border: `2.5px solid ${color}`,
        bgcolor: pinching ? `${color}59` : 'transparent',
        boxShadow: `0 0 14px ${color}`,
        pointerEvents: 'none',
        zIndex: 'tooltip',
        transition: 'width 80ms ease, height 80ms ease, background-color 80ms ease',
      }}
    />
  )
}
