import { useEffect, useState } from 'react'
import type { AgentMessage } from '../../ai/lib/types'
import { usePlanStore } from '../../plan/store/planStore'
import { ButtonBase, Stack, Tooltip, Typography } from '../../../ui'
import { call } from '../../../ipc'
import { ModelPicker } from '../../ai/components/ModelPicker'
import { useTeamStore, type TeamAgent } from '../store/teamStore'
import { useBotsStore, botKey, isGroup, GROUP_PREFIX, type BotStatus } from '../store/botsStore'
import { useBotFace } from './AgentFace'
import { Avatar, faceOf } from './bits'
import { BotChat } from './BotChat'
import { ChatView } from '../../ai/components/ChatView'
import { agentStoreFor, sendThrough, setActiveThread } from '../../ai/store/agentStore'
import { activeThreadId } from '../../ai/store/activeThread'

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
        <Avatar {...faceOf(agent)} size={34} state={face} animate />
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
  const groups = useBotsStore((s) => s.groups[root])
  if (isGroup(channel)) {
    const group = groups?.find((g) => `${GROUP_PREFIX}${g.id}` === channel)
    const members = team.filter((a) => group?.members.includes(a.id))
    return <Room key={channel} root={root} team={members} channel={channel} name={group?.name ?? 'Group'} />
  }
  const bot = channel !== 'room' ? team.find((a) => a.id === channel) : undefined
  if (bot) return <DirectMessage key={bot.id} root={root} bot={bot} team={team} />
  return <Room root={root} team={team} />
}

/** One teammate, alone: the conversation it keeps from day to day. It is
 * the bot's own thread, shown in the chat view with everything the chat has
 * (tool calls, plans, to-dos, checkpoints, approvals, cards); sending goes
 * through the bot. */
function DirectMessage({ root, bot }: { root: string; bot: TeamAgent; team: TeamAgent[] }) {
  const st = useBotsStore((s) => s.status[botKey(root, bot.id)])
  const { loadStatus, sendDm } = useBotsStore.getState()
  const save = useTeamStore((s) => s.save)
  const thread = `bot:${root}:${bot.id}`
  const state = st?.state ?? 'idle'
  // What compaction folded away, and its DM from before bots were threads.
  const [archive, setArchive] = useState<{ messages: AgentMessage[]; start: number }>({ messages: [], start: 0 })
  const [loadingOlder, setLoadingOlder] = useState(false)
  const loadArchive = async (before?: number) => {
    const page = await call<{ messages: AgentMessage[]; start: number }>('cmd_bots_archive', { root, botId: bot.id, before: before ?? null })
    if (page) setArchive((a) => ({ messages: before === undefined ? page.messages : [...page.messages, ...a.messages], start: page.start }))
  }

  useEffect(() => {
    const back = activeThreadId()
    const reload = async () => {
      const messages = (await call<AgentMessage[]>('cmd_bots_thread', { root, botId: bot.id })) ?? []
      // Opened mid-run (work its lead handed it): busy until the run's end
      // arrives, its steps streaming in from here.
      const working = useBotsStore.getState().status[botKey(root, bot.id)]?.state === 'working'
      agentStoreFor(thread).setState({ messages, streamingIndex: null, isBusy: working })
    }
    sendThrough(thread, (text, images) => call('cmd_bots_dm_send', { root, botId: bot.id, text, images }))
    setActiveThread(thread)
    void reload().then(() => loadArchive())
    void loadStatus(root)
    // Its plans, not the open thread's, while this conversation is showing.
    void usePlanStore.getState().setScope({ root, send: (t) => sendDm(root, bot.id, t) })
    return () => {
      void usePlanStore.getState().setScope(null)
      sendThrough(thread, null)
      setActiveThread(back)
    }
  }, [root, bot.id, thread, loadStatus, sendDm])

  const revertTo = async (index: number) => {
    await call('cmd_bots_revert', { root, botId: bot.id, index })
    const messages = (await call<AgentMessage[]>('cmd_bots_thread', { root, botId: bot.id })) ?? []
    agentStoreFor(thread).setState({ messages, streamingIndex: null })
  }

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
      </Stack>
      <ChatView
        own={{
          toolbar: <ModelPicker mode="agent" value={bot.model} onChange={(model) => void save(root, { ...bot, model }, bot.id)} />,
          onRevertTo: revertTo,
          earlier: archive.messages,
          earlierStart: archive.start,
          loadingOlder,
          onLoadOlder: () => {
            setLoadingOlder(true)
            void loadArchive(archive.start).finally(() => setLoadingOlder(false))
          },
        }}
      />
    </Stack>
  )
}

/** Everyone (the room), or a group: the user and some of the bots. */
function Room({ root, team, channel = 'room', name }: { root: string; team: TeamAgent[]; channel?: string; name?: string }) {
  const loadTeam = useTeamStore((s) => s.load)
  const focus = useTeamStore((s) => s.focus)
  const setChannel = useBotsStore((s) => s.setChannel)
  const messages = useBotsStore((s) => (channel === 'room' ? s.room[root] : s.dm[botKey(root, channel)]) ?? EMPTY_MSGS)
  const approvals = useBotsStore((s) => s.approvals).filter((a) => a.root === root)
  const status = useBotsStore((s) => s.status)
  const { loadRoom, loadGroupChat, loadStatus, post } = useBotsStore.getState()

  useEffect(() => {
    void loadTeam(root)
    void (channel === 'room' ? loadRoom(root) : loadGroupChat(root, channel))
    void loadStatus(root)
  }, [root, channel, loadTeam, loadRoom, loadGroupChat, loadStatus])

  const open = (id: string) => {
    focus(root, id)
    setChannel(root, id)
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
        channel={channel}
        messages={messages}
        team={team}
        approvals={approvals}
        onSend={(t) => post(root, t, channel)}
        allowEveryone
        placeholder={
          name
            ? team.length
              ? `Message ${name} — @ to mention someone in it`
              : 'Add people to this group to start'
            : team.length
              ? 'Message everyone — @ to mention a teammate'
              : 'Hire a teammate to start'
        }
        empty={
          <Stack sx={{ alignItems: 'center', textAlign: 'center', gap: 1, py: 8, color: 'text.secondary' }}>
            <Stack direction="row" sx={{ gap: 0.5 }}>
              {team.slice(0, 5).map((a) => (
                <Avatar key={a.id} {...faceOf(a)} size={34} />
              ))}
            </Stack>
            <Typography variant="body2" sx={{ fontWeight: 600, color: 'text.primary' }}>
              {name ?? 'Everyone'}
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
