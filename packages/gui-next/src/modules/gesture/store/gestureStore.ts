import { create } from 'zustand'
import { call, on } from '../../../ipc'
import type { GestureConfig, GestureDebugFrame, GestureEvent } from '../lib/types'

interface GestureStoreState {
  running: boolean
  debug: boolean
  cameraIndex: number
  /** Last debug frame (landmarks + state) — only populated while `debug` is
   * on, since the sidecar only emits it then. */
  lastDebugFrame: GestureDebugFrame | null
  /** Live virtual-cursor position (normalized 0-1, index fingertip), updated
   * every frame — kept separate from `recentEvents` since `pointer_move`
   * fires at frame rate and would otherwise flood that log. */
  pointer: { x: number; y: number } | null
  /** Whether a pinch is currently held — drives the cursor's visual state. */
  pinching: boolean
  /** Recent events, newest first, for the calibration log. Capped so it
   * doesn't grow unbounded over a long tuning session. `pointer_move` is
   * excluded (see `pointer` above). */
  recentEvents: { event: GestureEvent; at: number }[]
  sidecarLog: string[]
  start: (cameraIndex?: number, config?: Partial<GestureConfig>) => Promise<void>
  stop: () => Promise<void>
  setDebug: (debug: boolean) => Promise<void>
  init: () => Promise<void>
}

const MAX_EVENT_LOG = 50
const MAX_SIDECAR_LOG = 100

let initialized = false

export const useGestureStore = create<GestureStoreState>((set, get) => ({
  running: false,
  debug: false,
  cameraIndex: 0,
  lastDebugFrame: null,
  pointer: null,
  pinching: false,
  recentEvents: [],
  sidecarLog: [],

  init: async () => {
    if (initialized) return
    initialized = true

    await on<GestureEvent>('gesture_event', (event) => {
      if (event.type === 'pointer_move') {
        set({ pointer: { x: event.payload.x, y: event.payload.y } })
        return
      }
      if (event.type === 'pinch_start') set({ pinching: true })
      if (event.type === 'pinch_end') set({ pinching: false })
      if (event.type === 'hand_lost') set({ pointer: null, pinching: false })
      set((s) => ({ recentEvents: [{ event, at: Date.now() }, ...s.recentEvents].slice(0, MAX_EVENT_LOG) }))
    })
    await on<GestureDebugFrame>('gesture_debug_frame', (frame) => {
      set({ lastDebugFrame: frame })
    })
    await on<string>('gesture_sidecar_log', (line) => {
      set((s) => ({ sidecarLog: [line, ...s.sidecarLog].slice(0, MAX_SIDECAR_LOG) }))
    })
    await on<void>('gesture_sidecar_stopped', () => {
      set({ running: false, lastDebugFrame: null, pointer: null, pinching: false })
    })

    const running = await call<boolean>('cmd_gesture_status')
    set({ running: running ?? false })
  },

  start: async (cameraIndex, config) => {
    const idx = cameraIndex ?? get().cameraIndex
    await call('cmd_gesture_start', { cameraIndex: idx, config: config ?? null, debug: get().debug })
    set({ running: true, cameraIndex: idx })
  },

  stop: async () => {
    await call('cmd_gesture_stop')
    set({ running: false, lastDebugFrame: null, pointer: null, pinching: false })
  },

  setDebug: async (debug) => {
    set({ debug })
    if (get().running) await call('cmd_gesture_set_debug', { debug })
  },
}))
