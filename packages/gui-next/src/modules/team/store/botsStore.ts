import { create } from 'zustand'
import { call, on } from '../../../ipc'

/** Mirrors `agent::bots::ChatMessage`. */
export interface BotMessage {
  id: string
  /** `user`, a bot id, or `system`. */
  from: string
  text: string
  ts: number
  kind: 'message' | 'needs_you' | 'routine' | 'checkin' | 'note'
  mentions: string[]
}

export interface BotStatus {
  root: string
  botId: string
  state: 'idle' | 'working' | 'paused'
  activity: string | null
}

export type Level = 'auto' | 'if_requested' | 'ask' | 'hand_off'

export interface BotRules {
  files: Level
  commands: Level
  computer: Level
  web: Level
  outside: Level
}

export interface BotSettings {
  rules: BotRules
  paused: boolean
  alwaysOn: boolean
  checkinHours: number
  lastCheckin: number | null
}

export interface Routine {
  id: string
  title: string
  prompt: string
  cron: string
  enabled: boolean
  nextRun: number | null
  runs: { at: number; ok: boolean; summary: string }[]
}

export interface PendingApproval {
  root: string
  botId: string
  requestId: string
  toolName: string
  args: Record<string, unknown>
  reason: string | null
}

const key = (root: string, id: string) => `${root}::${id}`

interface BotsState {
  /** Room messages by project root. */
  room: Record<string, BotMessage[]>
  /** DM messages by `root::botId`. */
  dm: Record<string, BotMessage[]>
  status: Record<string, BotStatus>
  approvals: PendingApproval[]
  /** The conversation open in the chat panel, per project: `room` or a bot id. */
  channel: Record<string, string>
  /** Messages that arrived in a conversation that wasn't open, by `root::channel`. */
  unread: Record<string, number>
  setChannel: (root: string, channel: string) => void
  init: () => void
  loadRoom: (root: string) => Promise<void>
  loadDm: (root: string, botId: string) => Promise<void>
  loadStatus: (root: string) => Promise<void>
  post: (root: string, text: string) => Promise<void>
  sendDm: (root: string, botId: string, text: string) => Promise<void>
  respond: (requestId: string, approved: boolean) => Promise<void>
}

let listening = false

function append(list: BotMessage[] | undefined, m: BotMessage): BotMessage[] {
  const cur = list ?? []
  return cur.some((x) => x.id === m.id) ? cur : [...cur, m]
}

export const useBotsStore = create<BotsState>((set, get) => ({
  room: {},
  dm: {},
  status: {},
  approvals: [],
  channel: {},
  unread: {},

  setChannel: (root, channel) => {
    set((s) => ({ channel: { ...s.channel, [root]: channel }, unread: { ...s.unread, [key(root, channel)]: 0 } }))
    if (channel !== 'room' && !get().dm[key(root, channel)]) void get().loadDm(root, channel)
  },

  init: () => {
    if (listening) return
    listening = true
    void on<{ root: string; channel: string; message: BotMessage }>('bots:message', ({ root, channel, message }) => {
      if (channel === 'room') set((s) => ({ room: { ...s.room, [root]: append(s.room[root], message) } }))
      else set((s) => ({ dm: { ...s.dm, [key(root, channel)]: append(s.dm[key(root, channel)], message) } }))
      // A teammate's message in a conversation that isn't open counts as unread.
      if (message.from !== 'user' && (get().channel[root] ?? 'room') !== channel) {
        set((s) => ({ unread: { ...s.unread, [key(root, channel)]: (s.unread[key(root, channel)] ?? 0) + 1 } }))
      }
    })
    void on<BotStatus>('bots:status', (st) => set((s) => ({ status: { ...s.status, [key(st.root, st.botId)]: st } })))
    void on<PendingApproval>('bots:permission', (p) => set((s) => ({ approvals: [...s.approvals.filter((a) => a.requestId !== p.requestId), p] })))
    void on<{ requestId: string }>('bots:permission_resolved', ({ requestId }) =>
      set((s) => ({ approvals: s.approvals.filter((a) => a.requestId !== requestId) })),
    )
  },

  loadRoom: async (root) => {
    get().init()
    const msgs = (await call<BotMessage[]>('cmd_bots_room', { root })) ?? []
    set((s) => ({ room: { ...s.room, [root]: msgs } }))
  },

  loadDm: async (root, botId) => {
    get().init()
    const msgs = (await call<BotMessage[]>('cmd_bots_dm', { root, botId })) ?? []
    set((s) => ({ dm: { ...s.dm, [key(root, botId)]: msgs } }))
  },

  loadStatus: async (root) => {
    get().init()
    const list = (await call<BotStatus[]>('cmd_bots_status', { root })) ?? []
    set((s) => {
      const status = { ...s.status }
      for (const st of list) status[key(st.root, st.botId)] = st
      return { status }
    })
  },

  post: async (root, text) => {
    await call('cmd_bots_post', { root, text })
  },

  sendDm: async (root, botId, text) => {
    await call('cmd_bots_dm_send', { root, botId, text })
  },

  respond: async (requestId, approved) => {
    await call('cmd_agent_respond_permission', { requestId, approved })
    set((s) => ({ approvals: s.approvals.filter((a) => a.requestId !== requestId) }))
  },
}))

export const botKey = key
