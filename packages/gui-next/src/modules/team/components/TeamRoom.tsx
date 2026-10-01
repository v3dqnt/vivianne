import { useEffect } from 'react'
import { Box, ButtonBase, Stack, Tooltip, Typography, keyframes } from '../../../ui'
import { useTeamStore, type TeamAgent } from '../store/teamStore'
import { useBotsStore, botKey, type BotStatus } from '../store/botsStore'
import { usePanelStore } from '../../dock/store/panelStore'
import { Avatar, colorOf } from './bits'
import { BotChat } from './BotChat'

const EMPTY_AGENTS: TeamAgent[] = []
const EMPTY_MSGS: never[] = []

const pulse = keyframes`
  0%, 100% { opacity: 1; }
  50% { opacity: 0.35; }
`

/** A bot's face with its state: a dot (working pulses, paused is grey), and
 * what it's doing on hover. Clicking opens its page. */
export function BotChip({ root, agent, onOpen }: { root: string; agent: TeamAgent; onOpen: () => void }) {
  const st: BotStatus | undefined = useBotsStore((s) => s.status[botKey(root, agent.id)])
  const state = st?.state ?? 'idle'
  const dot = state === 'working' ? 'success.main' : state === 'paused' ? 'text.disabled' : 'text.secondary'
  const tip = state === 'working' ? `${agent.name} · ${st?.activity ?? 'working'}` : state === 'paused' ? `${agent.name} · paused` : `${agent.name} · ${agent.role || 'idle'}`
  return (
    <Tooltip title={tip}>
      <ButtonBase onClick={onOpen} sx={{ position: 'relative', borderRadius: '50%' }}>
        <Avatar name={agent.name} color={colorOf(agent)} size={30} />
        <Box
          sx={{
            position: 'absolute',
            right: -1,
            bottom: -1,
            width: 10,
            height: 10,
            borderRadius: '50%',
            bgcolor: dot,
            border: 2,
            borderColor: 'background.paper',
            animation: state === 'working' ? `${pulse} 1.2s ease-in-out infinite` : undefined,
          }}
        />
      </ButtonBase>
    </Tooltip>
  )
}

/**
 * The team's room: one conversation between the user and every bot. Say
 * what you need and the right teammate picks it up, or `@mention` one, or
 * `@everyone`. Bots hand work to each other here, post routine results and
 * check-ins, and ask for decisions.
 */
export function TeamRoom({ root }: { root: string }) {
  const team = useTeamStore((s) => (s.byRoot[root] ?? EMPTY_AGENTS)).filter((a) => a.status === 'active')
  const loadTeam = useTeamStore((s) => s.load)
  const focus = useTeamStore((s) => s.focus)
  const messages = useBotsStore((s) => s.room[root] ?? EMPTY_MSGS)
  const approvals = useBotsStore((s) => s.approvals).filter((a) => a.root === root)
  const status = useBotsStore((s) => s.status)
  const { loadRoom, loadStatus, post } = useBotsStore.getState()

  useEffect(() => {
    void loadTeam(root)
    void loadRoom(root)
    void loadStatus(root)
  }, [root, loadTeam, loadRoom, loadStatus])

  const open = (id: string) => {
    focus(root, id)
    usePanelStore.getState().openRight('agents')
  }
  const working = team.filter((a) => status[botKey(root, a.id)]?.state === 'working')

  return (
    <Stack sx={{ flex: 1, minHeight: 0 }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1.5, px: 2, py: 1, borderBottom: 1, borderColor: 'divider', flexShrink: 0 }}>
        <Stack direction="row" sx={{ gap: 0.75 }}>
          {team.map((a) => (
            <BotChip key={a.id} root={root} agent={a} onOpen={() => open(a.id)} />
          ))}
        </Stack>
        <Typography variant="caption" color="text.secondary" noWrap sx={{ flex: 1, minWidth: 0 }}>
          {team.length === 0
            ? 'No teammates yet'
            : working.length > 0
              ? working.map((a) => `${a.name} is ${status[botKey(root, a.id)]?.activity ?? 'working'}`).join(' · ')
              : `${team.length} teammate${team.length === 1 ? '' : 's'} · @mention one, or just say what you need`}
        </Typography>
      </Stack>
      <BotChat
        messages={messages}
        team={team}
        approvals={approvals}
        onSend={(t) => post(root, t)}
        allowEveryone
        placeholder={team.length ? 'Message the team — @ to mention a teammate' : 'Add a teammate in Agents to start'}
        empty={
          <Stack sx={{ alignItems: 'center', textAlign: 'center', gap: 1, py: 8, color: 'text.secondary' }}>
            <Stack direction="row" sx={{ gap: 0.5 }}>
              {team.slice(0, 5).map((a) => (
                <Avatar key={a.id} name={a.name} color={colorOf(a)} size={34} />
              ))}
            </Stack>
            <Typography variant="body2" sx={{ fontWeight: 600, color: 'text.primary' }}>
              Your team's room
            </Typography>
            <Typography variant="caption" sx={{ maxWidth: 380, lineHeight: 1.5 }}>
              Say what you need and the right teammate picks it up. @mention someone to hand it to them, or @everyone for the
              whole team. They post here as they work, hand off to each other, and ask when they need you.
            </Typography>
          </Stack>
        }
      />
    </Stack>
  )
}
