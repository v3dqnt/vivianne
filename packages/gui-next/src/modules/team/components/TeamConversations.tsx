import { previewText } from '../../ai/lib/previewText'
import { Box, ButtonBase, Stack, Typography, alpha, useTheme } from '../../../ui'
import type { TeamAgent } from '../store/teamStore'
import type { BotMessage } from '../store/botsStore'


export const SECTION_LABEL_SX = { fontWeight: 600, letterSpacing: 0.4, textTransform: 'uppercase', fontSize: '0.6875rem' } as const

export function preview(m: BotMessage | undefined, team: TeamAgent[]): string | null {
  if (!m) return null
  const who = m.from === 'user' ? 'You' : m.from === 'system' ? '' : (team.find((a) => a.id === m.from)?.name ?? m.from)
  const text = previewText(m.text)
  return who ? `${who}: ${text}` : text
}

function Unread({ n }: { n: number }) {
  if (!n) return null
  return (
    <Box
      sx={{
        minWidth: 18,
        height: 18,
        px: 0.5,
        borderRadius: '9px',
        bgcolor: 'primary.main',
        color: 'primary.contrastText',
        fontSize: '0.625rem',
        fontWeight: 700,
        display: 'grid',
        placeItems: 'center',
        flexShrink: 0,
      }}
    >
      {n > 99 ? '99+' : n}
    </Box>
  )
}

/** One conversation in the sidebar: a circle or a DM. */
export function ChannelRow({
  active,
  onClick,
  icon,
  title,
  subtitle,
  subtitleTone,
  unread,
}: {
  active: boolean
  onClick: () => void
  icon: React.ReactNode
  title: string
  subtitle: string
  subtitleTone?: string
  unread: number
}) {
  const theme = useTheme()
  return (
    <ButtonBase
      onClick={onClick}
      aria-current={active ? 'true' : undefined}
      sx={{
        display: 'flex',
        justifyContent: 'flex-start',
        alignItems: 'center',
        gap: 1,
        width: '100%',
        textAlign: 'left',
        px: 1,
        py: 0.75,
        borderRadius: '10px',
        bgcolor: active ? alpha(theme.palette.primary.main, 0.12) : 'transparent',
        '&:hover': { bgcolor: active ? alpha(theme.palette.primary.main, 0.16) : 'action.hover' },
      }}
    >
      {icon}
      <Stack sx={{ minWidth: 0, flex: 1 }}>
        <Typography variant="body2" sx={{ fontWeight: active || unread ? 600 : 500, fontSize: '0.8125rem' }} noWrap>
          {title}
        </Typography>
        <Typography variant="caption" color={subtitleTone ?? 'text.secondary'} sx={{ fontSize: '0.6875rem' }} noWrap>
          {subtitle}
        </Typography>
      </Stack>
      <Unread n={unread} />
    </ButtonBase>
  )
}
