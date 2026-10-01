import { useEffect } from 'react'
import { Box, ButtonBase, IconButton, Stack, Tooltip, Typography, alpha, keyframes, useTheme } from '../../../ui'
import { AddLinear, AgentsLinear } from '../../../icons'
import type { TeamAgent } from '../store/teamStore'
import { useBotsStore, botKey, type BotMessage } from '../store/botsStore'
import { Avatar, colorOf } from './bits'

const pulse = keyframes`
  0%, 100% { opacity: 1; }
  50% { opacity: 0.35; }
`

const SECTION_LABEL_SX = { fontWeight: 600, letterSpacing: 0.4, textTransform: 'uppercase', fontSize: '0.6875rem' } as const

function preview(m: BotMessage | undefined, team: TeamAgent[]): string | null {
  if (!m) return null
  const who = m.from === 'user' ? 'You' : m.from === 'system' ? '' : (team.find((a) => a.id === m.from)?.name ?? m.from)
  const text = m.text.replace(/\s+/g, ' ').trim()
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

function Row({
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

/**
 * An agent team's conversations, in the sidebar: the team chat, and a
 * direct message with each teammate. Picking one opens it in the chat
 * panel; each shows its latest message, unread count, and who is working.
 */
export function TeamConversations({ root, team, onManage }: { root: string; team: TeamAgent[]; onManage: () => void }) {
  const room = useBotsStore((s) => s.room[root])
  const dm = useBotsStore((s) => s.dm)
  const status = useBotsStore((s) => s.status)
  const unread = useBotsStore((s) => s.unread)
  const channel = useBotsStore((s) => s.channel[root] ?? 'room')
  const approvals = useBotsStore((s) => s.approvals)
  const { setChannel, loadRoom, loadDm, loadStatus } = useBotsStore.getState()

  useEffect(() => {
    void loadRoom(root)
    void loadStatus(root)
    for (const a of team) void loadDm(root, a.id)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [root, team.map((a) => a.id).join(',')])

  const working = team.filter((a) => status[botKey(root, a.id)]?.state === 'working')
  const roomLast = room?.[room.length - 1]

  return (
    <Stack sx={{ px: 1.5, gap: 1.5 }}>
      <Stack sx={{ gap: 0.25 }}>
        <Typography variant="caption" color="text.secondary" sx={{ ...SECTION_LABEL_SX, px: 0.5, pb: 0.5 }}>
          Team chat
        </Typography>
        <Row
          active={channel === 'room'}
          onClick={() => setChannel(root, 'room')}
          icon={
            <Box sx={{ width: 26, height: 26, borderRadius: '8px', display: 'grid', placeItems: 'center', bgcolor: 'action.hover', color: 'text.secondary', flexShrink: 0 }}>
              <AgentsLinear size={15} color="currentColor" />
            </Box>
          }
          title="Everyone"
          subtitle={working.length ? `${working.map((a) => a.name).join(', ')} working…` : (preview(roomLast, team) ?? 'Say what you need')}
          subtitleTone={working.length ? 'success.main' : undefined}
          unread={unread[botKey(root, 'room')] ?? 0}
        />
      </Stack>

      <Stack sx={{ gap: 0.25 }}>
        <Stack direction="row" sx={{ alignItems: 'center', px: 0.5, pb: 0.5 }}>
          <Typography variant="caption" color="text.secondary" sx={{ ...SECTION_LABEL_SX, flex: 1 }}>
            Direct messages{team.length ? ` · ${team.length}` : ''}
          </Typography>
          <Tooltip title="Add or manage teammates">
            <IconButton size="small" aria-label="Manage teammates" onClick={onManage}>
              <AddLinear size={13} color="currentColor" />
            </IconButton>
          </Tooltip>
        </Stack>
        {team.map((a) => {
          const st = status[botKey(root, a.id)]
          const msgs = dm[botKey(root, a.id)]
          const waiting = approvals.some((p) => p.root === root && p.botId === a.id)
          const state = st?.state ?? 'idle'
          return (
            <Row
              key={a.id}
              active={channel === a.id}
              onClick={() => setChannel(root, a.id)}
              icon={
                <Box sx={{ position: 'relative', flexShrink: 0 }}>
                  <Avatar name={a.name} color={colorOf(a)} size={26} />
                  <Box
                    sx={{
                      position: 'absolute',
                      right: -1,
                      bottom: -1,
                      width: 9,
                      height: 9,
                      borderRadius: '50%',
                      border: 2,
                      borderColor: 'background.paper',
                      bgcolor: waiting ? 'warning.main' : state === 'working' ? 'success.main' : state === 'paused' ? 'text.disabled' : 'transparent',
                      animation: state === 'working' ? `${pulse} 1.2s ease-in-out infinite` : undefined,
                    }}
                  />
                </Box>
              }
              title={a.name}
              subtitle={
                waiting
                  ? 'Waiting for your approval'
                  : state === 'working'
                    ? (st?.activity ?? 'Working…')
                    : state === 'paused'
                      ? 'Paused'
                      : (preview(msgs?.[msgs.length - 1], team) ?? a.role ?? '')
              }
              subtitleTone={waiting ? 'warning.main' : state === 'working' ? 'success.main' : undefined}
              unread={unread[botKey(root, a.id)] ?? 0}
            />
          )
        })}
        {team.length === 0 && (
          <Typography variant="caption" color="text.secondary" sx={{ display: 'block', px: 1, py: 0.5, lineHeight: 1.45 }}>
            No teammates yet. Add them with + — each gets its own job, model and tools, and they work on your tasks together.
          </Typography>
        )}
      </Stack>
    </Stack>
  )
}
