import { Box, Typography, alpha } from '../../../ui'
import { SUBAGENT_COLORS, SUBAGENT_COLOR_HEX } from '../../settings/lib/subagentProfiles'
import type { TeamAgent } from '../store/teamStore'
import { AgentMark } from '../../../ui/brand/AgentMark'

export function colorOf(agent: Pick<TeamAgent, 'color'>): string {
  const key = agent.color as (typeof SUBAGENT_COLORS)[number] | null
  return (key && SUBAGENT_COLOR_HEX[key]) || SUBAGENT_COLOR_HEX.blue
}

/** An agent's face: the agent mark in the agent's colour, on a soft disc
 * of the same colour so it holds its place in a list like any avatar. */
export function Avatar({ name, color, size = 26 }: { name: string; color: string; size?: number }) {
  return (
    <Box
      title={name}
      sx={{
        width: size,
        height: size,
        borderRadius: '50%',
        flexShrink: 0,
        display: 'grid',
        placeItems: 'center',
        bgcolor: alpha(color, 0.14),
      }}
    >
      <AgentMark color={color} size={Math.round(size * 0.78)} title={name} />
    </Box>
  )
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
