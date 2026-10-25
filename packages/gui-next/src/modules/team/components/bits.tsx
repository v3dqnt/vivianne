import { Box, Typography } from '../../../ui'
import { SUBAGENT_COLORS, SUBAGENT_COLOR_HEX } from '../../settings/lib/subagentProfiles'
import type { TeamAgent } from '../store/teamStore'
import { AgentFace, useBotFace, type FaceState } from './AgentFace'
import type { FaceStyle } from '../store/faceStore'

/** An agent's colour: one of the named swatches, or any `#hex`. */
export function colorOf(agent: Pick<TeamAgent, 'color'>): string {
  if (agent.color?.startsWith('#')) return agent.color
  const key = agent.color as (typeof SUBAGENT_COLORS)[number] | null
  return (key && SUBAGENT_COLOR_HEX[key]) || SUBAGENT_COLOR_HEX.blue
}

/** Everything an `Avatar` needs from an agent. */
export function faceOf(agent: Pick<TeamAgent, 'name' | 'color' | 'face'>) {
  return { name: agent.name, color: colorOf(agent), face: agent.face }
}

/** An agent's face: a robot head in the agent's colour (see `AgentFace`).
 * Still unless given a `state` to show, so lists of them cost nothing. */
export function Avatar({
  name,
  color,
  size = 26,
  state,
  animate,
  face,
}: {
  name: string
  color: string
  size?: number
  state?: FaceState
  animate?: boolean
  face?: FaceStyle | null
}) {
  return <AgentFace name={name} color={color} size={size} state={state} animate={animate} face={face} />
}

/** An agent's face that follows what it is doing: thinking, searching,
 * working, waiting on you, asleep. Still by default (it shows the state as a
 * single frame); `animate` keeps it moving, idle included. Only the sidebar
 * row and the conversation's top bar animate, and both read the same live
 * state with the same seed, so they move together. */
export function LiveAvatar({
  root,
  agent,
  size = 26,
  animate = false,
}: {
  root: string
  agent: Pick<TeamAgent, 'id' | 'name' | 'color' | 'face'>
  size?: number
  animate?: boolean
}) {
  const state = useBotFace(root, agent.id)
  return <AgentFace {...faceOf(agent)} size={size} state={state} animate={animate} />
}

export function Tag({ children }: { children: React.ReactNode }) {
  return (
    <Box
      component="span"
      sx={{ px: 0.75, py: 0.125, borderRadius: '6px', bgcolor: 'action.hover', color: 'text.secondary', fontSize: '0.6875rem', whiteSpace: 'nowrap' }}
    >
      {children}
    </Box>
  )
}

export function SectionLabel({ children }: { children: React.ReactNode }) {
  return (
    <Typography variant="caption" color="text.secondary" sx={{ fontWeight: 600, letterSpacing: 0.4, textTransform: 'uppercase', fontSize: '0.6875rem', px: 0.5 }}>
      {children}
    </Typography>
  )
}
