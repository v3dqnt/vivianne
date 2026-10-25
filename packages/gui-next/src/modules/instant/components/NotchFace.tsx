import { lazy, Suspense, useMemo } from 'react'
import { Box } from '../../../ui'
import type { RobotHeadShape, RobotHeadState } from 'robot-heads'
import { hash01, shapeFor } from '../../team/components/AgentFace'
import { useFacePrefs } from '../../team/store/faceStore'

/** The robot head (robot-heads, MIT), loaded on first use. */
const RobotHead = lazy(() => import('robot-heads').then((m) => ({ default: m.RobotHead })))

/** A shell that reads on the notch's hardware black in both themes. */
const SHELL = '#5b5b66'
const TRIM = '#a1a1aa'

/**
 * A small robot head for the notch. Performance: only a head with something
 * to show joins the shared animation loop; idle, sleeping and finished heads
 * are paused (one still frame, no work after). The notch never takes clicks
 * on the face, so pointer tracking is off.
 */
export function NotchFace({
  state,
  size = 20,
  name = 'Vivianne',
  shape,
  vivianne = name === 'Vivianne',
}: {
  state: RobotHeadState
  size?: number
  /** Whose head: picks the agent's own outline and offsets its blinks. */
  name?: string
  shape?: RobotHeadShape
  /** Wears Vivianne's customized head (Settings → Appearance). */
  vivianne?: boolean
}) {
  const seed = useMemo(() => hash01(name), [name])
  const viv = useFacePrefs((s) => s.viv)
  const motion = useFacePrefs((s) => s.motion)
  // Vivianne's own head is the one Settings → Appearance customizes.
  const own = vivianne ? viv : {}
  const still = motion === 'never' || (motion !== 'always' && (state === 'idle' || state === 'sleeping'))
  return (
    <Suspense fallback={<Box sx={{ width: size, height: size, flexShrink: 0 }} />}>
      <RobotHead
        shape={shape ?? own.shape ?? shapeFor(name)}
        state={state}
        size={size}
        color={own.color ?? SHELL}
        trimColor={own.trim ?? TRIM}
        screenColor={own.screen}
        seed={seed}
        paused={still}
        interactive={false}
        antenna={false}
        floorShadow={false}
        aria-label={`${name}, ${state}`}
        style={{ flexShrink: 0, display: 'inline-block' }}
      />
    </Suspense>
  )
}
