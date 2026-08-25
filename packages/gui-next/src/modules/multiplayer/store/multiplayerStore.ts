import { create } from 'zustand'
import { call, on } from '../../../ipc'
import type { AgentMessage } from '../../ai/lib/types'

export type ConnectionStatus =
  | { state: 'disconnected' }
  | { state: 'connecting' }
  | { state: 'connected'; peerId: string; peerDisplayName: string | null }
  | { state: 'failed'; reason: string }

export interface ChatEntry {
  fromPeer: boolean
  text: string
  timestamp: number
}

interface MultiplayerStoreState {
  myId: string | null
  displayName: string | null
  status: ConnectionStatus
  peerMessages: AgentMessage[]
  chatLog: ChatEntry[]
  connecting: boolean
  error: string | null
  initialized: boolean
  init: () => Promise<void>
  connect: (peerId: string) => Promise<void>
  disconnect: () => Promise<void>
  setDisplayName: (name: string) => Promise<void>
  sendChat: (text: string) => Promise<void>
}

/** Mirrored history is capped client-side too, matching the backend's own
 * bound (`MAX_MIRRORED_ENTRIES` in `state.rs`) — an append-only array fed by
 * a live event stream is unbounded growth waiting to happen otherwise, and
 * the panel only ever shows a scrollback window anyway. */
const MAX_ENTRIES = 500
function pushBounded<T>(arr: T[], item: T): T[] {
  const next = arr.length >= MAX_ENTRIES ? arr.slice(arr.length - MAX_ENTRIES + 1) : arr
  return [...next, item]
}

/** Phase 1 of the multiplayer update: direct P2P pairing + a mirror of the
 * connected peer's session, updated as each of their turns completes (not
 * token-by-token — see `multiplayer::broadcast_agent_event`'s doc comment
 * on the Rust side). See `docs/multiplayer-update-plan.md`. */
export const useMultiplayerStore = create<MultiplayerStoreState>((set, get) => ({
  myId: null,
  displayName: null,
  status: { state: 'disconnected' },
  peerMessages: [],
  chatLog: [],
  connecting: false,
  error: null,
  initialized: false,

  init: async () => {
    if (get().initialized) return
    set({ initialized: true })

    // Listeners attach *before* the initial fetch resolves — a component
    // that mounts while `init` is still in flight otherwise sees
    // `initialized: true` (set above, synchronously) with no listeners
    // wired up yet, and could miss an event that lands in that window.
    await on<ConnectionStatus>('multiplayer_status', (status) => set({ status }))
    await on<AgentMessage>('multiplayer_peer_message', (message) => {
      set((s) => ({ peerMessages: pushBounded(s.peerMessages, message) }))
    })
    await on<ChatEntry>('multiplayer_chat', (entry) => {
      set((s) => ({ chatLog: pushBounded(s.chatLog, entry) }))
    })

    const [myId, displayName, status, peerMessages, chatLog] = await Promise.all([
      call<string>('cmd_multiplayer_my_id'),
      call<string | null>('cmd_multiplayer_get_display_name'),
      call<ConnectionStatus>('cmd_multiplayer_status'),
      call<AgentMessage[]>('cmd_multiplayer_peer_messages'),
      call<ChatEntry[]>('cmd_multiplayer_chat_log'),
    ])
    set({
      myId: myId ?? null,
      displayName: displayName ?? null,
      status: status ?? { state: 'disconnected' },
      peerMessages: peerMessages ?? [],
      chatLog: chatLog ?? [],
    })
  },

  connect: async (peerId) => {
    set({ connecting: true, error: null })
    try {
      await call('cmd_multiplayer_connect', { peerId })
    } catch (err) {
      set({ error: err instanceof Error ? err.message : String(err) })
      throw err
    } finally {
      set({ connecting: false })
    }
  },

  disconnect: async () => {
    await call('cmd_multiplayer_disconnect')
    set({ status: { state: 'disconnected' }, peerMessages: [], chatLog: [] })
  },

  setDisplayName: async (name) => {
    await call('cmd_multiplayer_set_display_name', { name })
    set({ displayName: name.trim() || null })
  },

  sendChat: async (text) => {
    await call('cmd_multiplayer_send_chat', { text })
    set((s) => ({ chatLog: pushBounded(s.chatLog, { fromPeer: false, text, timestamp: Date.now() }) }))
  },
}))
