import { Box, Typography, alpha } from '../../../ui'
import { SUBAGENT_COLORS, SUBAGENT_COLOR_HEX } from '../../settings/lib/subagentProfiles'
import type { TeamAgent } from '../store/teamStore'

export function colorOf(agent: Pick<TeamAgent, 'color'>): string {
  const key = agent.color as (typeof SUBAGENT_COLORS)[number] | null
  return (key && SUBAGENT_COLOR_HEX[key]) || SUBAGENT_COLOR_HEX.blue
}

export function Avatar({ name, color, size = 26 }: { name: string; color: string; size?: number }) {
  return (
    <Box
      sx={{
        width: size,
        height: size,
        borderRadius: '50%',
        flexShrink: 0,
        display: 'grid',
        placeItems: 'center',
        bgcolor: alpha(color, 0.18),
        color,
        fontSize: size * 0.42,
        fontWeight: 700,
      }}
    >
      {name.trim()[0]?.toUpperCase() ?? '?'}
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
    <Typography variant="caption" color="text.secondary" sx={{ fontWeight: 600, letterSpacing: 0.4, textTransform: 'uppercase', fontSize: '0.625rem', px: 0.5 }}>
      {children}
    </Typography>
  )
}
