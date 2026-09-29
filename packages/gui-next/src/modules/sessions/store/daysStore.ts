import { create } from 'zustand'
import { call } from '../../../ipc'
import { usePlanStore } from '../../plan/store/planStore'
import { useAgentStore } from '../../ai/store/agentStore'
import type { AgentMessage } from '../../ai/lib/types'
import type { SessionInfo } from '../lib/types'
import { useProjectsStore, modeOf } from './projectsStore'
import { useWorkspaceStore } from '../../ai/store/workspaceStore'

/** Mirrors `agent::days::GENERAL_SCOPE` and `projectsStore`'s `GENERAL_PROJECT_ID`. */
export const GENERAL_SCOPE = '__general__'

/** Mirrors `agent::conversation::OpenedConversation`. */
interface OpenedConversation {
  id: string
  scope: string
  stitchedDays: number
}

/** An archived pre-migration thread, open read-only. */
export interface Viewing {
  id: string
  label: string
  kind: 'archive'
  messages: AgentMessage[]
}

/**
 * The project's one conversation — the user and its master agent.
 *
 * There are no days and no threads: a project has a single conversation for
 * its whole life (`agent::conversation`). What compaction takes out of the
 * model's context is kept in `earlier`, shown above the live messages, so the
 * user can scroll back through everything they ever said.
 *
 * The file keeps its old name so its many importers didn't all have to move.
 */
interface ConversationState {
  /** Project id, or `GENERAL_SCOPE`. */
  scope: string | null
  /** The live conversation's session id. */
  todayId: string | null
  /** Messages compacted out of the model's context, oldest first — the
   * loaded pages of the archive, not necessarily all of it. */
  earlier: AgentMessage[]
  /** Archive index of `earlier[0]`; above 0, older pages remain. */
  earlierStart: number
  loadingOlder: boolean
  archive: SessionInfo[]
  /** Set while an archived thread is on screen instead of the conversation. */
  viewing: Viewing | null
  /** General's threads, newest first. Empty for a project. */
  threads: SessionInfo[]

  openScope: (scope: string) => Promise<void>
  /** Resolves once the conversation is open — call before sending. */
  ensureToday: () => Promise<void>
  refreshEarlier: () => Promise<void>
  /** Load the next older page of the archive in front of `earlier`. */
  loadOlder: () => Promise<void>
  refreshArchive: () => Promise<void>
  view: (id: string, label: string) => Promise<void>
  backToToday: () => void

  // ── General only: threads, as before projects had one conversation ──
  refreshThreads: () => Promise<void>
  newThread: () => Promise<void>
  openThread: (id: string) => Promise<void>
  renameThread: (id: string, name: string) => Promise<void>
  deleteThread: (id: string) => Promise<void>
}

/** The single conversation General had before it went back to threads. It
 * shows up as an ordinary thread so nothing said in it disappears. */
const LEGACY_GENERAL_CONVERSATION = `conv--${GENERAL_SCOPE}`

/** Whether a scope is organised as threads: General always, and a project
 * created as a threads project. */
export function isThreaded(scope: string | null): boolean {
  if (!scope) return false
  if (scope === GENERAL_SCOPE) return true
  return modeOf(useProjectsStore.getState().projects.find((p) => p.id === scope)) === 'threads'
}

/** A scope's threads, newest first: General's are the ones assigned to no
 * project; a threads project's are the ones assigned to it. */
function threadsOf(scope: string, all: SessionInfo[]): SessionInfo[] {
  const map = useProjectsStore.getState().sessionProjectMap
  return all
    .filter((s) => (scope === GENERAL_SCOPE && s.id === LEGACY_GENERAL_CONVERSATION) || (!s.id.startsWith('conv--') && !s.id.startsWith('day--')))
    .filter((s) => (scope === GENERAL_SCOPE ? !map[s.id] : map[s.id] === scope))
    .sort((a, b) => b.created_at.localeCompare(a.created_at))
}

/** The last thread open in each scope, so returning lands back on it. */
const LAST_THREAD_KEY = 'viv.general.lastThread'
const keyFor = (scope: string | null) => (scope && scope !== GENERAL_SCOPE ? `${LAST_THREAD_KEY}.${scope}` : LAST_THREAD_KEY)
function rememberThread(id: string) {
  try {
    localStorage.setItem(keyFor(useDaysStore.getState().scope), id)
  } catch {
    // storage unavailable — it only costs landing on the newest thread instead
  }
}
function lastThread(): string | null {
  try {
    return localStorage.getItem(keyFor(useDaysStore.getState().scope))
  } catch {
    return null
  }
}

/** Pull the live transcript the backend now holds into the chat view. */
async function syncLiveMessages() {
  const messages = await call<AgentMessage[]>('cmd_agent_messages')
  useAgentStore.setState({ messages: messages ?? [], streamingIndex: null })
}

/** Serialises `openScope`: switching projects quickly would otherwise let an
 * older open land after a newer one and leave the wrong project live. */
/** One page of a conversation's archive (`agent::conversation::ArchivePage`). */
interface ArchivePage {
  messages: AgentMessage[]
  start: number
  total: number
}

let opening: Promise<void> = Promise.resolve()

/** A project switch requested while a turn was running, applied when it ends —
 * swapping the conversation mid-turn would let the turn finish into the other
 * project's transcript. */
let pendingScope: string | null = null

export const useDaysStore = create<ConversationState>((set, get) => ({
  scope: null,
  todayId: null,
  earlier: [],
  earlierStart: 0,
  loadingOlder: false,
  archive: [],
  viewing: null,
  threads: [],

  openScope: (scope) => {
    const run = async () => {
      if (useAgentStore.getState().isBusy) {
        pendingScope = scope
        return
      }
      if (isThreaded(scope)) {
        // Threads: reopen the last one, or the newest, or start one.
        set({ scope, viewing: null, earlier: [], earlierStart: 0 })
        await get().refreshThreads()
        const threads = get().threads
        const target = threads.find((t) => t.id === lastThread()) ?? threads[0]
        if (target) await loadThread(target.id)
        else await createThread()
        return
      }
      const opened = await call<OpenedConversation>('cmd_open_conversation', { scope })
      if (!opened) return
      set({ scope, todayId: opened.id, viewing: null, earlier: [], earlierStart: 0, threads: [] })
      await syncLiveMessages()
      await get().refreshEarlier()
      // After the transcript: resuming a long task sends a message into it.
      void useAgentStore.getState().refreshLongTask()
      void usePlanStore.getState().refreshList()
    }
    opening = opening.then(run, run)
    return opening
  },

  ensureToday: async () => {
    // At launch the first message can otherwise be sent before the
    // conversation is open, and be saved under a stray session id.
    await opening
  },

  refreshEarlier: async () => {
    const id = get().todayId
    // Threads keep no compaction archive; only a project conversation does.
    if (!id || !id.startsWith('conv--') || isThreaded(get().scope)) return
    // Only the most recent page: the archive keeps everything ever said to
    // the master agent, and holding all of it in the view grows without
    // bound. Older pages load on request (`loadOlder`).
    const page = await call<ArchivePage>('cmd_read_conversation_archive', { id })
    if (get().todayId === id) set({ earlier: page?.messages ?? [], earlierStart: page?.start ?? 0 })
  },

  loadOlder: async () => {
    const { todayId: id, earlierStart, loadingOlder } = get()
    if (!id || earlierStart <= 0 || loadingOlder) return
    set({ loadingOlder: true })
    try {
      const page = await call<ArchivePage>('cmd_read_conversation_archive', { id, before: earlierStart })
      if (page && get().todayId === id) set({ earlier: [...page.messages, ...get().earlier], earlierStart: page.start })
    } finally {
      set({ loadingOlder: false })
    }
  },

  refreshArchive: async () => {
    const archive = await call<SessionInfo[]>('cmd_list_archive')
    set({ archive: archive ?? [] })
  },

  view: async (id, label) => {
    if (id === get().todayId) {
      set({ viewing: null })
      return
    }
    const messages = await call<AgentMessage[]>('cmd_read_session_messages', { id })
    set({ viewing: { id, label, kind: 'archive', messages: messages ?? [] } })
  },

  backToToday: () => set({ viewing: null }),

  refreshThreads: async () => {
    const all = await call<SessionInfo[]>('cmd_get_sessions')
    const scope = get().scope ?? GENERAL_SCOPE
    set({ threads: threadsOf(scope, all ?? []) })
  },

  newThread: () => serialized(createThread),
  openThread: (id) => serialized(() => loadThread(id)),

  renameThread: async (id, name) => {
    await call('cmd_rename_session', { id, name })
    await get().refreshThreads()
  },

  deleteThread: async (id) => {
    const wasOpen = get().todayId === id
    await call('cmd_delete_session', { id })
    await get().refreshThreads()
    if (wasOpen) {
      const next = get().threads[0]
      await serialized(() => (next ? loadThread(next.id) : createThread()))
    }
  },
}))

/** Thread switches share `opening` with project switches, so the two can't
 * interleave and leave the wrong transcript live. */
function serialized(run: () => Promise<void>): Promise<void> {
  const guarded = async () => {
    if (useAgentStore.getState().isBusy) return // never swap mid-turn
    await run()
  }
  opening = opening.then(guarded, guarded)
  return opening
}

/** A thread's workspace: General's is the whole computer; a project's is
 * its folder, whatever the thread happened to be saved with. */
async function applyThreadWorkspace() {
  const scope = useDaysStore.getState().scope
  const project = useProjectsStore.getState().projects.find((p) => p.id === scope)
  if (project?.path) {
    const ws = useWorkspaceStore.getState()
    await ws.setWorkspace([project.path], ws.accessMode)
  } else {
    await useWorkspaceStore.getState().applyWholeComputer()
  }
}

async function loadThread(id: string) {
  await call('cmd_load_session', { id })
  await applyThreadWorkspace()
  useDaysStore.setState({ todayId: id, viewing: null, earlier: [], earlierStart: 0 })
  rememberThread(id)
  await syncLiveMessages()
}

async function createThread() {
  await applyThreadWorkspace()
  const id = await call<string>('cmd_new_session')
  if (!id) return
  // A project's thread belongs to it.
  const scope = useDaysStore.getState().scope
  if (scope && scope !== GENERAL_SCOPE) await useProjectsStore.getState().assign(id, scope)
  useDaysStore.setState({ todayId: id, viewing: null, earlier: [], earlierStart: 0 })
  rememberThread(id)
  await syncLiveMessages()
  await useDaysStore.getState().refreshThreads()
}

// A turn just finished: apply a deferred project switch, or pick up whatever
// compaction moved into the archive during it.
let wasBusy = false
useAgentStore.subscribe((s) => {
  if (wasBusy && !s.isBusy) {
    if (pendingScope) {
      const scope = pendingScope
      pendingScope = null
      void useDaysStore.getState().openScope(scope)
    } else if (isThreaded(useDaysStore.getState().scope)) {
      // The autosave may have named the thread after its first message.
      void useDaysStore.getState().refreshThreads()
    } else {
      void useDaysStore.getState().refreshEarlier()
    }
  }
  wasBusy = s.isBusy
})
