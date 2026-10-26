import { create } from 'zustand'
import { call, on } from '../../../ipc'
import type { ImageAttachment, FileAttachment } from '../../ai/lib/types'

/** Mirrors `agent::bots::ChatMessage`. */
export interface BotMessage {
  id: string
  /** `user`, a bot id, or `system`. */
  from: string
  text: string
  ts: number
  kind: 'message' | 'needs_you' | 'routine' | 'checkin' | 'note'
  mentions: string[]
  /** What the agent did to get to this reply (its tool calls). */
  steps?: BotStep[]
  /** What the user attached, by name (the content went to the agent). */
  attachments?: { name: string; kind: 'image' | 'file' }[]
}

/** Files and images going with a DM. */
export interface DmAttachments {
  images: ImageAttachment[]
  files: FileAttachment[]
}

/** Mirrors `agent::bots::BotStep`: one tool call, in a few words. */
export interface BotStep {
  id: string
  tool: string
  label: string
  state: 'running' | 'done' | 'error'
  detail?: string
}

/** A run in flight: where it will reply, and its steps so far. */
export interface LiveRun {
  channel: string
  steps: BotStep[]
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
  /** Runs in flight by `root::botId`, shown live in their conversation. */
  live: Record<string, LiveRun>
  /** A face to show briefly after a run (`happy` done, `error` failed). */
  flash: Record<string, 'happy' | 'error'>
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
  /** Post in Everyone (`room`) or a group (`group:<id>`). */
  post: (root: string, text: string, channel?: string) => Promise<void>
  /** The groups the user made, by root. Everyone isn't one of them. */
  groups: Record<string, BotGroup[]>
  loadGroups: (root: string) => Promise<void>
  saveGroup: (root: string, group: BotGroup) => Promise<BotGroup | null>
  deleteGroup: (root: string, id: string) => Promise<void>
  /** A group's messages, kept beside DMs under `root::group:<id>`. */
  loadGroupChat: (root: string, channel: string) => Promise<void>
  sendDm: (root: string, botId: string, text: string, attachments?: DmAttachments) => Promise<void>
  respond: (requestId: string, approved: boolean) => Promise<void>
}

let listening = false

/** A group chat between the user and some of the bots. */
export interface BotGroup {
  id: string
  name: string
  members: string[]
}

export const GROUP_PREFIX = 'group:'
export const isGroup = (channel: string) => channel.startsWith(GROUP_PREFIX)

function append(list: BotMessage[] | undefined, m: BotMessage): BotMessage[] {
  const cur = list ?? []
  return cur.some((x) => x.id === m.id) ? cur : [...cur, m]
}

export const useBotsStore = create<BotsState>((set, get) => ({
  room: {},
  dm: {},
  status: {},
  live: {},
  flash: {},
  approvals: [],
  channel: {},
  unread: {},
  groups: {},

  setChannel: (root, channel) => {
    set((s) => ({ channel: { ...s.channel, [root]: channel }, unread: { ...s.unread, [key(root, channel)]: 0 } }))
    if (isGroup(channel)) void get().loadGroupChat(root, channel)
    else if (channel !== 'room' && !get().dm[key(root, channel)]) void get().loadDm(root, channel)
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
    void on<BotStatus>('bots:status', (st) =>
      set((s) => {
        const k = key(st.root, st.botId)
        // A run just ended: a moment of happy (or the error face, if the
        // last thing in its conversation says it couldn't finish).
        if (s.status[k]?.state === 'working' && st.state !== 'working') {
          const ch = s.live[k]?.channel ?? st.botId
          const list = ch === 'room' ? s.room[st.root] : s.dm[k]
          const last = list?.[list.length - 1]
          const failed = !!last && last.from === 'system' && /couldn't finish/.test(last.text)
          const face: 'happy' | 'error' = failed ? 'error' : 'happy'
          window.setTimeout(
            () => set((x) => ({ flash: Object.fromEntries(Object.entries(x.flash).filter(([y]) => y !== k)) })),
            failed ? 4000 : 2500,
          )
          s = { ...s, flash: { ...s.flash, [k]: face } }
        }
        // A finished run's steps are on its reply now; drop the live copy.
        const live = st.state === 'working' ? s.live : Object.fromEntries(Object.entries(s.live).filter(([x]) => x !== k))
        return { status: { ...s.status, [k]: st }, live, flash: s.flash }
      }),
    )
    void on<{ root: string; botId: string; channel: string; steps: BotStep[] }>('bots:steps', ({ root, botId, channel, steps }) =>
      set((s) => ({ live: { ...s.live, [key(root, botId)]: { channel, steps } } })),
    )
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

  post: async (root, text, channel) => {
    await call('cmd_bots_post', { root, text, channel: channel ?? null })
  },

  loadGroups: async (root) => {
    const list = (await call<BotGroup[]>('cmd_bots_groups', { root })) ?? []
    set((s) => ({ groups: { ...s.groups, [root]: list } }))
  },

  saveGroup: async (root, group) => {
    const saved = await call<BotGroup>('cmd_bots_save_group', { root, group })
    await get().loadGroups(root)
    return saved
  },

  deleteGroup: async (root, id) => {
    await call('cmd_bots_delete_group', { root, id })
    await get().loadGroups(root)
    if (get().channel[root] === `${GROUP_PREFIX}${id}`) get().setChannel(root, 'room')
  },

  loadGroupChat: async (root, channel) => {
    get().init()
    const msgs = (await call<BotMessage[]>('cmd_bots_room', { root, channel })) ?? []
    set((s) => ({ dm: { ...s.dm, [key(root, channel)]: msgs } }))
  },

  sendDm: async (root, botId, text, attachments) => {
    await call('cmd_bots_dm_send', {
      root,
      botId,
      text,
      files: attachments?.files.length ? attachments.files : null,
      images: attachments?.images.length ? attachments.images : null,
    })
  },

  respond: async (requestId, approved) => {
    await call('cmd_agent_respond_permission', { requestId, approved })
    set((s) => ({ approvals: s.approvals.filter((a) => a.requestId !== requestId) }))
  },
}))

export const botKey = key
