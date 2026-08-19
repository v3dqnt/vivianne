/**
 * Wire types mirroring the Rust gesture module
 * (`packages/gui/src-tauri/src/gesture/`). Kept as a single typed source
 * rather than `any` at each call site — a backend rename becomes a type
 * error here, not a silently-blank UI somewhere downstream.
 */

export interface Landmark {
  x: number
  y: number
  z: number
}

/** `#[serde(tag = "type", content = "payload", rename_all = "snake_case")]` */
export type GestureEvent =
  | { type: 'hand_detected' }
  | { type: 'hand_lost' }
  | { type: 'pointer_move'; payload: { x: number; y: number } }
  | { type: 'pinch_start'; payload: { x: number; y: number } }
  | { type: 'pinch_end'; payload: { x: number; y: number } }
  | { type: 'click'; payload: { x: number; y: number } }
  | { type: 'adjust'; payload: { delta: number } }
  | { type: 'swipe'; payload: { direction: 'left' | 'right' } }
  | { type: 'open_palm_hold' }

export interface GestureDebugFrame {
  landmarks: Landmark[]
  tracking: boolean
  events: GestureEvent[]
}

/** Mirrors `gesture::config::GestureConfig` — every field a tunable
 * threshold, `#[serde(rename_all = "camelCase")]`. */
export interface GestureConfig {
  pinchCloseDistance: number
  pinchOpenDistance: number
  pinchMinHoldMs: number
  rotationDeadzoneDegPerSec: number
  rotationSensitivity: number
  rotationMaxDegPerSec: number
  swipeMinVelocity: number
  swipeMinDistance: number
  swipeCooldownMs: number
  openPalmHoldMs: number
  fingerExtendedRatio: number
  pointerActiveMargin: number
  smoothingWindow: number
  minDetectionConfidence: number
  handLostFrames: number
}
