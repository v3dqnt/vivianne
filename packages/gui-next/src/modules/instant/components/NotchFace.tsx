import { lazy, Suspense, useMemo } from 'react'
import { Box } from '../../../ui'
import type { RobotHeadShape, RobotHeadState } from 'robot-heads'
import { shapeFor } from '../../team/components/AgentFace'

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
}: {
  state: RobotHeadState
  size?: number
  /** Whose head: picks the agent's own outline and offsets its blinks. */
  name?: string
  shape?: RobotHeadShape
}) {
  const seed = useMemo(() => {
    let h = 0
    for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) >>> 0
    return (h % 1000) / 1000
  }, [name])
  const still = state === 'idle' || state === 'sleeping'
  return (
    <Suspense fallback={<Box sx={{ width: size, height: size, flexShrink: 0 }} />}>
      <RobotHead
        shape={shape ?? shapeFor(name)}
        state={state}
        size={size}
        color={SHELL}
        trimColor={TRIM}
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
