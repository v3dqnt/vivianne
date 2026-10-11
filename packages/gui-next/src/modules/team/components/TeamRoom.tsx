import { useEffect } from 'react'
import { Button, ButtonBase, Stack, Tooltip, Typography } from '../../../ui'
import { call } from '../../../ipc'
import { ModelPicker } from './ModelPicker'
import { useTeamStore, type TeamAgent } from '../store/teamStore'
import { useBotsStore, botKey, type BotStatus } from '../store/botsStore'
import { usePanelStore } from '../../dock/store/panelStore'
import { useBotFace } from './AgentFace'
import { Avatar, colorOf } from './bits'
import { BotChat } from './BotChat'

const EMPTY_AGENTS: TeamAgent[] = []
const EMPTY_MSGS: never[] = []


/** A bot's face, showing its state (thinking, searching, working, waiting
 * on you, asleep when paused), and what it's doing on hover. Clicking opens
 * its page. */
export function BotChip({ root, agent, onOpen }: { root: string; agent: TeamAgent; onOpen: () => void }) {
  const st: BotStatus | undefined = useBotsStore((s) => s.status[botKey(root, agent.id)])
  const face = useBotFace(root, agent.id)
  const state = st?.state ?? 'idle'
  const tip = state === 'working' ? `${agent.name} · ${st?.activity ?? 'working'}` : state === 'paused' ? `${agent.name} · paused` : `${agent.name} · ${agent.role || 'idle'}`
  return (
    <Tooltip title={tip}>
      <ButtonBase onClick={onOpen} sx={{ position: 'relative', borderRadius: '50%' }}>
        <Avatar name={agent.name} color={colorOf(agent)} size={34} state={face} />
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
  const channel = useBotsStore((s) => s.channel[root] ?? 'room')
  const team = useTeamStore((s) => (s.byRoot[root] ?? EMPTY_AGENTS)).filter((a) => a.status === 'active')
  const bot = channel !== 'room' ? team.find((a) => a.id === channel) : undefined
  if (bot) return <DirectMessage key={bot.id} root={root} bot={bot} team={team} />
  return <Room root={root} team={team} />
}

/** One teammate, alone: the conversation it keeps from day to day. */
function DirectMessage({ root, bot, team }: { root: string; bot: TeamAgent; team: TeamAgent[] }) {
  const messages = useBotsStore((s) => s.dm[botKey(root, bot.id)] ?? EMPTY_MSGS)
  const approvals = useBotsStore((s) => s.approvals).filter((a) => a.root === root && a.botId === bot.id)
  const st = useBotsStore((s) => s.status[botKey(root, bot.id)])
  const { loadDm, loadStatus, sendDm } = useBotsStore.getState()
  const focus = useTeamStore((s) => s.focus)
  const save = useTeamStore((s) => s.save)
  useEffect(() => {
    void loadDm(root, bot.id)
    void loadStatus(root)
  }, [root, bot.id, loadDm, loadStatus])

  const state = st?.state ?? 'idle'
  return (
    <Stack sx={{ flex: 1, minHeight: 0 }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1.25, px: 2, py: 1, borderBottom: 1, borderColor: 'divider', flexShrink: 0 }}>
        <BotChip root={root} agent={bot} onOpen={() => {}} />
        <Stack sx={{ minWidth: 0, flex: 1 }}>
          <Stack direction="row" sx={{ alignItems: 'baseline', gap: 0.75 }}>
            <Typography variant="body2" sx={{ fontWeight: 700 }} noWrap>
              {bot.name}
            </Typography>
            <Typography variant="caption" color="text.secondary">
              @{bot.id}
            </Typography>
          </Stack>
          <Typography variant="caption" color={state === 'working' ? 'success.main' : 'text.secondary'} noWrap sx={{ fontSize: '0.6875rem' }}>
            {state === 'working' ? (st?.activity ?? 'Working…') : state === 'paused' ? 'Paused' : bot.role}
          </Typography>
        </Stack>
        {state === 'working' && (
          <Button size="small" variant="outlined" color="error" onClick={() => void call('cmd_bots_stop', { root, botId: bot.id })} sx={{ borderRadius: '8px', flexShrink: 0 }}>
            Stop
          </Button>
        )}
        <Tooltip title="Routines, rules, memory">
          <Button
            size="small"
            variant="outlined"
            onClick={() => {
              focus(root, bot.id)
              usePanelStore.getState().openRight('agents')
            }}
            sx={{ borderRadius: '8px', flexShrink: 0 }}
          >
            Profile
          </Button>
        </Tooltip>
      </Stack>
      <BotChat
        root={root}
        channel={bot.id}
        messages={messages}
        team={team}
        approvals={approvals}
        onSend={(t, att) => sendDm(root, bot.id, t, att)}
        attachments
        toolbar={<ModelPicker compact value={bot.model} onChange={(model) => void save(root, { ...bot, model }, bot.id)} />}
        allowEveryone={false}
        placeholder={`Message ${bot.name}`}
        empty={
          <Stack sx={{ alignItems: 'center', textAlign: 'center', gap: 1, py: 8, color: 'text.secondary' }}>
            <Avatar name={bot.name} color={colorOf(bot)} size={72} animate />
            <Typography variant="body2" sx={{ fontWeight: 600, color: 'text.primary' }}>
              {bot.name}
            </Typography>
            <Typography variant="caption" sx={{ maxWidth: 360, lineHeight: 1.5 }}>
              {bot.role ? `${bot.role}. ` : ''}Talk to {bot.name} on its own here. It keeps this conversation, and what it learns,
              from one day to the next.
            </Typography>
          </Stack>
        }
      />
    </Stack>
  )
}

function Room({ root, team }: { root: string; team: TeamAgent[] }) {
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
        root={root}
        channel="room"
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
