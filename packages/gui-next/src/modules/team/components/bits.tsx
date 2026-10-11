import { Box, Typography } from '../../../ui'
import { SUBAGENT_COLORS, SUBAGENT_COLOR_HEX } from '../../settings/lib/subagentProfiles'
import type { TeamAgent } from '../store/teamStore'
import { AgentFace, type FaceState } from './AgentFace'

export function colorOf(agent: Pick<TeamAgent, 'color'>): string {
  const key = agent.color as (typeof SUBAGENT_COLORS)[number] | null
  return (key && SUBAGENT_COLOR_HEX[key]) || SUBAGENT_COLOR_HEX.blue
}

/** An agent's face: a robot head in the agent's colour (see `AgentFace`).
 * Still unless given a `state` to show, so lists of them cost nothing. */
export function Avatar({
  name,
  color,
  size = 26,
  state,
  animate,
}: {
  name: string
  color: string
  size?: number
  state?: FaceState
  animate?: boolean
}) {
  return <AgentFace name={name} color={color} size={size} state={state} animate={animate} />
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
