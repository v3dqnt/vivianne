import { lazy, Suspense, useMemo } from 'react'
import { Box, alpha } from '../../../ui'
import { AgentMark } from '../../../ui/brand/AgentMark'
import type { RobotHeadShape, RobotHeadState } from 'robot-heads'
import { useBotsStore, type BotStatus, type BotStep, type LiveRun, type PendingApproval } from '../store/botsStore'

/** An agent's face: a robot head (robot-heads, MIT) in the agent's colour,
 * its LED face showing what the agent is doing. Loaded on first use, so the
 * app's startup doesn't carry it; until then, the agent mark stands in. */
const RobotHead = lazy(() => import('robot-heads').then((m) => ({ default: m.RobotHead })))

export type FaceState = RobotHeadState

const SHAPES: RobotHeadShape[] = ['rectangle', 'square', 'circle', 'hexagon']

/** A stable 0–1 number from a string. */
function hash01(s: string): number {
  let h = 2166136261
  for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 16777619)
  return ((h >>> 0) % 10_000) / 10_000
}

/** A stable shape per agent, so each one keeps its own outline. */
export function shapeFor(key: string): RobotHeadShape {
  return SHAPES[Math.floor(hash01(key) * SHAPES.length) % SHAPES.length]
}

/**
 * A robot head. Performance: a head only joins the shared animation loop
 * while it has something to show (`state` other than idle, or `animate`);
 * otherwise it is paused, which draws one still frame and costs nothing
 * after. Lists of past messages are therefore free; only agents at work move.
 */
export function AgentFace({
  name,
  color,
  size = 26,
  state = 'idle',
  animate,
  interactive = false,
}: {
  name: string
  color: string
  size?: number
  state?: FaceState
  /** Move even when idle (a profile's big head). Default: only when busy. */
  animate?: boolean
  interactive?: boolean
}) {
  const seed = useMemo(() => hash01(name), [name])
  const live = animate ?? state !== 'idle'
  const fallback = (
    <Box sx={{ width: size, height: size, borderRadius: '50%', display: 'grid', placeItems: 'center', bgcolor: alpha(color, 0.14), flexShrink: 0 }}>
      <AgentMark color={color} size={Math.round(size * 0.78)} title={name} />
    </Box>
  )
  return (
    <Suspense fallback={fallback}>
      <RobotHead
        shape={shapeFor(name)}
        state={state}
        size={size}
        color={color}
        seed={seed}
        paused={!live}
        interactive={interactive}
        // Small heads drop the antenna stem (the head fills the room it took)
        // and the floor shadow, which only reads at a larger size.
        antenna={size >= 40}
        floorShadow={size >= 56}
        title={name}
        aria-label={`${name}, ${state}`}
        style={{ flexShrink: 0, display: 'inline-block' }}
      />
    </Suspense>
  )
}

/** Tools whose work is looking things up: the face searches. */
const SEARCH_TOOLS = new Set([
  'web_search', 'web_fetch', 'web_preview', 'read', 'grep', 'glob', 'ls', 'recall', 'guide', 'helium',
  'browser_snapshot', 'browser_navigate', 'inspect_document', 'list_tools', 'semantic_search',
])

/** What an agent's face shows, from its status and the run in flight. */
export function faceState(
  status: BotStatus | undefined,
  run: LiveRun | undefined,
  waiting: PendingApproval | undefined,
  flash: FaceState | undefined,
): FaceState {
  if (flash) return flash
  if (status?.state === 'paused') return 'sleeping'
  if (status?.state !== 'working') return 'idle'
  // Waiting on the user to allow something: attentive.
  if (waiting) return 'listening'
  const current: BotStep | undefined = run ? [...run.steps].reverse().find((s) => s.state === 'running') : undefined
  if (!current) return 'thinking'
  if (SEARCH_TOOLS.has(current.tool) || current.tool.startsWith('browser_') || current.tool.startsWith('mcp__')) return 'searching'
  return 'working'
}

/** An agent's live face state, from the bots store. */
export function useBotFace(root: string | null | undefined, botId: string): FaceState {
  const k = `${root}::${botId}`
  const status = useBotsStore((s) => (root ? s.status[k] : undefined))
  const run = useBotsStore((s) => (root ? s.live[k] : undefined))
  const waiting = useBotsStore((s) => (root ? s.approvals.find((a) => a.root === root && a.botId === botId) : undefined))
  const flash = useBotsStore((s) => (root ? s.flash[k] : undefined))
  return faceState(status, run, waiting, flash)
}
