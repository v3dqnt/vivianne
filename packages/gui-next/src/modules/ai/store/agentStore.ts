import { create, type StoreApi, type UseBoundStore } from 'zustand'
import { call, on } from '../../../ipc'
import type { AgentEvent, AgentMessage, ImageAttachment, MessageContent, QuestionSpec, RunLedger } from '../lib/types'
import { useModelStore } from './modelStore'
import { useSubagentStore } from './subagentStore'
import { useShellTaskStore, isShellTool } from '../../dock/store/shellTaskStore'
import { useWebPreviewStore } from './webPreviewStore'
import { usePanelStore, type RightPanelKind } from '../../dock/store/panelStore'
import { notifyFileChanged, resolveAgentPath } from '../../ide/lib/fileChangeBus'
import { useWorkspaceStore } from './workspaceStore'

/** Wall-clock start of the current streaming assistant message — set on
 * `message_start`, consumed on `message_end` to compute tokens/sec. Plain
 * module state rather than store state since no component ever reads it
 * directly, only the derived `tokensPerSec` stamped onto the message.
 * Keyed by thread id since multiple threads can stream concurrently. */
const streamStartedAt = new Map<string, number>()

/** Tool names whose start/end also drives a side-panel store, so agentStore
 * (the one place already subscribed to every tool event) can just forward
 * instead of every panel needing its own duplicate listener. */

/** Computer-use tool names that should light the "Vivianne is driving the
 * screen" indicator. The tool is `dip`; `computer` is still accepted by the
 * backend as an alias for it (see `agent/tool_exec.rs`), so a model that calls
 * it by that name must light the indicator too — otherwise the agent takes
 * over the mouse with nothing on screen saying so. */
const COMPUTER_USE_TOOLS = new Set(['dip', 'computer'])

function isComputerUseTool(name: string): boolean {
  return COMPUTER_USE_TOOLS.has(name)
}

export interface PendingPermission {
  requestId: string
  toolCallId: string
  toolName: string
  args: unknown
  reason: string | null
}

export interface PendingQuestion {
  requestId: string
  toolCallId: string
  questions: QuestionSpec[]
}

export interface PendingConfirmation {
  requestId: string
  toolCallId: string
  message: string
  dangerLevel: string
}

export interface RetryStatus {
  attempt: number
  maxAttempts: number
  reason: string
}

/** Mirror of `agent::commands::QueueSnapshot` — steering is "next up",
 * follow-up runs after the current turn. Both hold full `AgentMessage`s. */
export interface QueuedMessages {
  steering: AgentMessage[]
  followUp: AgentMessage[]
}

export interface ModelFallback {
  fromProvider: string
  fromModel: string
  toProvider: string
  toModel: string
  reason: string
}

export interface CredentialRotated {
  provider: string
  fromLabel: string
  toLabel: string
  reason: string
}

/**
 * Owns exactly one thing: the live chat transcript and whether a turn is
 * running. Deliberately narrow — sessions, models, and workspace state each
 * get their own store as those screens are built, instead of one store
 * accumulating every concern the app has (the old frontend's ~2000-line
 * `AgentProvider` was exactly that, and it's why a single unrelated update
 * anywhere could re-render every subscriber). Zustand selectors
 * (`useAgentStore(s => s.isBusy)`) mean a component only re-renders when the
 * slice it actually reads changes.
 */
interface AgentStoreState {
  messages: AgentMessage[]
  isBusy: boolean
  /** Index into `messages` of the assistant message currently streaming, or
   * null when nothing is in flight. `MessageStart` sets it, `MessageUpdate`
   * appends into it, `MessageEnd`/`AgentEnd` clear it. */
  streamingIndex: number | null
  initialized: boolean
  /** True while a manual compaction is folding history into a summary. */
  compacting: boolean
  /** A gated tool call is paused waiting on approve/deny. */
  pendingPermission: PendingPermission | null
  /** The `question` tool is paused waiting on 1-4 multiple-choice answers. */
  pendingQuestion: PendingQuestion | null
  /** The `confirm_action` tool is paused waiting on a yes/no. */
  pendingConfirmation: PendingConfirmation | null
  /** Transient — clears itself on the next agent event. */
  retryStatus: RetryStatus | null
  /** Sticky for the rest of the turn once set. */
  modelFallback: ModelFallback | null
  /** Transient — clears itself on the next agent event, same rule as `retryStatus`. */
  credentialRotated: CredentialRotated | null
  /** True while the `computer` tool is mid-execution — the agent has control of the screen. */
  computerUseActive: boolean
  /** How many memory entries were silently folded into the current turn's
   * prompt, or null once the turn ends — the per-turn injection is
   * deliberately invisible to the model (it's told not to narrate reading
   * memory), so this is the only signal the user gets that it happened. */
  memoryRecalled: number | null
  /** One-shot channel for putting text INTO the composer from elsewhere —
   * "edit this message" reverts to that point and hands the original text
   * back to be re-sent. The composer consumes and clears it, so this is a
   * transient handoff, not the live editor value (which stays local to the
   * composer so typing doesn't re-render every store subscriber). */
  draft: string
  setDraft: (text: string) => void
  /** What's sitting in the backend steering + follow-up queues. */
  queuedMessages: QueuedMessages
  /** Every model failed and the run is waiting before trying again. */
  runWaiting: { untilMs: number; reason: string } | null
  /** The ledger of this conversation's current task (`agent::run_ledger`):
   * status, spend, checkpoints. Every task has one. */
  longTask: RunLedger | null
  /** Re-read the long task for the open conversation; picks a running one
   * back up after a relaunch. */
  refreshLongTask: () => Promise<void>
  /** Pause, stop, raise the caps of, or resume the long task. */
  controlLongTask: (action: 'pause' | 'stop' | 'extend' | 'resume', add?: { usd?: number; hours?: number }) => Promise<void>
  init: () => Promise<void>
  sendMessage: (text: string, images?: ImageAttachment[]) => Promise<void>
  /** Fold older history into a summary. Returns how many messages were folded (0 = nothing to compact). */
  compactNow: () => Promise<number>
  respondPermission: (approved: boolean) => Promise<void>
  answerQuestion: (answers: string[][]) => Promise<void>
  respondConfirmation: (approved: boolean) => Promise<void>
  /** Re-read the steering/follow-up queues from the backend. */
  refreshQueues: () => Promise<void>
  /** Drop one queued item. `queue` is "steering" or "followUp" (wire-safe). */
  removeQueued: (queue: 'steering' | 'followUp', index: number) => Promise<void>
  /** Empty both queues. */
  clearQueues: () => Promise<void>
  /** Queue a message to run after the current turn finishes. */
  queueFollowUp: (text: string) => Promise<void>
  /** Interrupt the running turn and redirect it with this message now. */
  steer: (text: string) => Promise<void>
  /** Stop the current turn (end the task). */
  abort: () => Promise<void>
  /** Revert the transcript to a message index (checkpoint). */
  revertToIndex: (index: number) => Promise<void>
}

/** Only these four backend commands are thread-scoped today (see
 * `AgentRegistry` in `agent/commands.rs`) — everything else (abort, steer,
 * queues, revert, compaction, permission responses) still only ever touches
 * the backend's single primary `AppState`. Calling them from a secondary
 * thread's store would silently act on the WRONG thread, so those actions
 * are gated to primary-only below instead of quietly misrouting — a real,
 * disclosed gap to close in a follow-up, not a silent correctness bug. */
const PRIMARY_THREAD_ID = ''

function isPrimary(threadId: string): boolean {
  return threadId === PRIMARY_THREAD_ID
}

/** Wraps `agent_event`'s payload — every emit from the backend now carries
 * which thread it belongs to (`AgentRegistry::resolve`'s sentinel: empty
 * string = primary), so a listener can ignore events meant for other panes. */
interface ThreadScopedEvent {
  threadId: string
  event: AgentEvent
}

export function createAgentStore(threadId: string): UseBoundStore<StoreApi<AgentStoreState>> {
  return create<AgentStoreState>((set, get) => {
  /** Resume the long task by sending its briefing as a message, so picking
   * the task back up is visible in the conversation like any other turn. */
  const resumeLongTask = async () => {
    const prompt = await call<string | null>('cmd_long_task_resume_prompt', { threadId })
    if (prompt) await get().sendMessage(prompt)
  }
  return {
    messages: [],
    isBusy: false,
    streamingIndex: null,
    initialized: false,
    compacting: false,
    pendingPermission: null,
    pendingQuestion: null,
    pendingConfirmation: null,
    retryStatus: null,
    modelFallback: null,
    credentialRotated: null,
    computerUseActive: false,
    memoryRecalled: null,
    draft: '',
    setDraft: (text: string) => set({ draft: text }),
    queuedMessages: { steering: [], followUp: [] },
    runWaiting: null,
    longTask: null,

    refreshLongTask: async () => {
      if (!isPrimary(threadId)) return
      const ledger = (await call<RunLedger | null>('cmd_long_task_get', { threadId })) ?? null
      set({ longTask: ledger })
      // A task that was running (or waiting out a rate limit) when the app
      // closed carries on by itself.
      if (ledger && (ledger.status === 'running' || ledger.status === 'waiting') && !get().isBusy) {
        await resumeLongTask()
      }
    },

    controlLongTask: async (action, add) => {
      if (!isPrimary(threadId)) return
      const ledger = await call<RunLedger | null>('cmd_long_task_control', {
        threadId,
        action,
        addUsd: add?.usd ?? null,
        addHours: add?.hours ?? null,
      })
      set({ longTask: action === 'stop' ? null : (ledger ?? null) })
      if (action === 'pause' || action === 'stop') set({ runWaiting: null })
      if (action === 'resume' && ledger && !get().isBusy) await resumeLongTask()
    },

    init: async () => {
      if (get().initialized) return
      set({ initialized: true })

      await on<ThreadScopedEvent>('agent_event', ({ threadId: eventThreadId, event }) => {
        if (eventThreadId !== threadId) return
        // `retryStatus` is explicitly transient — clear it on anything except
        // another retry tick, so a resolved retry doesn't linger forever.
        if (event.type !== 'retry_status') {
          set((s) => (s.retryStatus ? { retryStatus: null } : s))
        }
        if (event.type !== 'credential_rotated') {
          set((s) => (s.credentialRotated ? { credentialRotated: null } : s))
        }
        // A wait is over the moment anything else happens on the run.
        if (event.type !== 'run_waiting' && event.type !== 'long_task') {
          set((s) => (s.runWaiting ? { runWaiting: null } : s))
        }
        switch (event.type) {
          case 'memory_injected': {
            set({ memoryRecalled: event.payload.entry_count })
            break
          }
          case 'message_start': {
            streamStartedAt.set(threadId, Date.now())
            set((s) => ({
              messages: [...s.messages, event.payload.message],
              streamingIndex: s.messages.length,
            }))
            break
          }
          case 'message_update': {
            set((s) => {
              if (s.streamingIndex === null) return s
              const messages = [...s.messages]
              const current = messages[s.streamingIndex]
              if (!current || current.type !== 'assistant') return s
              // Each update carries a single-block delta message tagged with
              // its own kind (thinking vs text) — NOT the full accumulated
              // message. Merge into whichever running block matches that kind
              // and keep the other block (if any) untouched, so reasoning
              // streams live instead of only appearing once the final message
              // lands (the two kinds were previously being mashed into one
              // text block regardless of which the delta actually was).
              const delta = event.payload.message.content[0]
              if (!delta) return s
              const prevText = current.content.find((c): c is Extract<MessageContent, { type: 'text' }> => c.type === 'text')
              const prevThinking = current.content.find(
                (c): c is Extract<MessageContent, { type: 'thinking' }> => c.type === 'thinking',
              )
              let content: MessageContent[]
              if (delta.type === 'thinking') {
                const thinking: MessageContent = { type: 'thinking', thinking: (prevThinking?.thinking ?? '') + delta.thinking }
                content = prevText ? [thinking, prevText] : [thinking]
              } else if (delta.type === 'text') {
                const text: MessageContent = { type: 'text', text: (prevText?.text ?? '') + delta.text }
                content = prevThinking ? [prevThinking, text] : [text]
              } else {
                return s
              }
              messages[s.streamingIndex] = { ...current, content }
              return { messages }
            })
            break
          }
          case 'message_end': {
            set((s) => {
              const messages = [...s.messages]
              let message = event.payload.message
              const startedAt = streamStartedAt.get(threadId)
              if (message.type === 'assistant' && message.usage && startedAt) {
                const elapsedSec = (Date.now() - startedAt) / 1000
                if (elapsedSec > 0 && message.usage.output > 0) {
                  message = { ...message, tokensPerSec: message.usage.output / elapsedSec }
                }
              }
              streamStartedAt.delete(threadId)
              // No in-flight streaming message means no `message_start` ever
              // fired for this one — a provider that errors before its first
              // token (out of credits, rate limit, bad key) does exactly
              // that. Dropping it here is what made those failures look like
              // the agent aborting silently; append instead so the error
              // actually reaches the transcript. Start/end are balanced
              // (end always clears the index), so this can't double-append a
              // message that did stream.
              if (s.streamingIndex === null) {
                return { messages: [...messages, message], streamingIndex: null }
              }
              messages[s.streamingIndex] = message
              return { messages, streamingIndex: null }
            })
            break
          }
          case 'agent_end': {
            set({ isBusy: false, streamingIndex: null, memoryRecalled: null })
            if (isPrimary(threadId)) {
              void useModelStore.getState().refreshStats()
              // QueuePanel only polls while `isBusy`, so without this the
              // panel keeps rendering whatever was queued at the last poll
              // — including messages the run has since drained and answered,
              // which reads as "queued but never sent". This is the one
              // refresh that runs after the drain, so it's what makes the
              // panel reflect reality once a run ends.
              void get().refreshQueues()
            }
            break
          }
          case 'tool_execution_start': {
            if (isComputerUseTool(event.payload.tool_name)) set({ computerUseActive: true })
            if (isPrimary(threadId) && event.payload.tool_name === 'task') void useSubagentStore.getState().refresh()
            // Shell tools run as one-shot processes, not PTY sessions, so the
            // Background Tasks panel (which lists terminal sessions) can only
            // learn about them from here.
            if (isShellTool(event.payload.tool_name)) {
              useShellTaskStore
                .getState()
                .start(
                  event.payload.tool_call_id,
                  event.payload.tool_name,
                  event.payload.args as Record<string, unknown> | undefined,
                )
            }
            // Insert a pending placeholder immediately so the tool card shows
            // a spinner while it runs, instead of only appearing once the
            // (possibly slow) tool finishes. `tool_execution_end` below
            // replaces this same entry in place, matched by toolCallId.
            set((s) => ({
              messages: [
                ...s.messages,
                {
                  type: 'toolResult',
                  toolCallId: event.payload.tool_call_id,
                  toolName: event.payload.tool_name,
                  content: [],
                  details: null,
                  isError: null,
                  timestamp: Date.now(),
                  arguments: event.payload.args as Record<string, unknown> | undefined,
                },
              ],
            }))
            break
          }
          case 'tool_execution_end': {
            if (isComputerUseTool(event.payload.tool_name)) set({ computerUseActive: false })
            if (isShellTool(event.payload.tool_name)) {
              useShellTaskStore.getState().finish(event.payload.tool_call_id, !!event.payload.is_error)
            }
            if (isPrimary(threadId) && event.payload.tool_name === 'task') void useSubagentStore.getState().refresh()
            set((s) => {
              const idx = s.messages.findIndex(
                (m) => m.type === 'toolResult' && m.toolCallId === event.payload.tool_call_id,
              )
              if (idx === -1) return s
              const messages = [...s.messages]
              const prev = messages[idx]
              if (prev.type !== 'toolResult') return s
              // Tell any open IDE tab for this file that it just changed on
              // disk. `write`/`edit`/`apply_patch` all use `path`;
              // `notebook_edit` uses `notebook_path`. Skipped on error — a
              // failed edit didn't touch the file, so there's nothing to warn
              // the editor about.
              if (
                !event.payload.is_error &&
                ['write', 'edit', 'apply_patch', 'notebook_edit'].includes(event.payload.tool_name)
              ) {
                const args = prev.arguments as Record<string, unknown> | undefined
                const rawPath = (args?.path ?? args?.notebook_path) as string | undefined
                if (rawPath) {
                  const root = useWorkspaceStore.getState().folders[0] ?? null
                  notifyFileChanged(resolveAgentPath(rawPath, root))
                }
              }
              messages[idx] = {
                ...prev,
                content: event.payload.result.content,
                details: event.payload.result.details,
                isError: event.payload.is_error,
              }
              return { messages }
            })
            break
          }
          case 'permission_asked': {
            const p = event.payload
            set({
              pendingPermission: {
                requestId: p.request_id,
                toolCallId: p.tool_call_id,
                toolName: p.tool_name,
                args: p.args,
                reason: p.reason,
              },
            })
            break
          }
          case 'permission_resolved': {
            set((s) => (s.pendingPermission?.requestId === event.payload.request_id ? { pendingPermission: null } : s))
            break
          }
          case 'question_asked': {
            const p = event.payload
            set({ pendingQuestion: { requestId: p.request_id, toolCallId: p.tool_call_id, questions: p.questions } })
            break
          }
          case 'question_resolved': {
            set((s) => (s.pendingQuestion?.requestId === event.payload.request_id ? { pendingQuestion: null } : s))
            break
          }
          case 'confirmation_asked': {
            const p = event.payload
            set({
              pendingConfirmation: {
                requestId: p.request_id,
                toolCallId: p.tool_call_id,
                message: p.message,
                dangerLevel: p.danger_level,
              },
            })
            break
          }
          case 'confirmation_resolved': {
            set((s) =>
              s.pendingConfirmation?.requestId === event.payload.request_id ? { pendingConfirmation: null } : s,
            )
            break
          }
          case 'retry_status': {
            const p = event.payload
            set({ retryStatus: { attempt: p.attempt, maxAttempts: p.max_attempts, reason: p.reason } })
            break
          }
          case 'model_fallback': {
            const p = event.payload
            set({
              modelFallback: {
                fromProvider: p.from_provider,
                fromModel: p.from_model,
                toProvider: p.to_provider,
                toModel: p.to_model,
                reason: p.reason,
              },
            })
            break
          }
          case 'run_waiting': {
            set({ runWaiting: { untilMs: event.payload.until_ms, reason: event.payload.reason } })
            break
          }
          case 'long_task': {
            set({ longTask: event.payload.ledger })
            break
          }
          case 'credential_rotated': {
            const p = event.payload
            set({
              credentialRotated: { provider: p.provider, fromLabel: p.from_label, toLabel: p.to_label, reason: p.reason },
            })
            break
          }
          case 'web_preview_requested': {
            if (!isPrimary(threadId)) break
            const p = event.payload
            useWebPreviewStore.getState().requested(p.url, p.screenshot_base64, p.error)
            // Pop the merged Preview panel open in Browser mode so agent-driven
            // browsing (web_preview, or any browser_* interaction tool) is
            // actually visible without the user having to go find it.
            usePanelStore.getState().openRight('preview')
            // 'live', not 'browser'. The agent now drives the embedded webview,
            // so this is the same page and the same session the user can click
            // — not a parallel screenshot feed of a second browser. Sending them
            // to 'browser' here is what kept the live view unreachable in
            // practice, however many times it was built.
            usePanelStore.getState().setPreviewMode('live')
            break
          }
          case 'panel_focus_requested': {
            if (!isPrimary(threadId)) break
            const p = event.payload
            const RIGHT_PANEL_KINDS: RightPanelKind[] = ['review', 'files', 'preview', 'tasks', 'plan']
            if (p.panel.startsWith('custom:')) {
              // Always "open" — set_panel's backend-side validation already
              // confirmed this id exists, and there's no sensible "close the
              // panel showing THIS custom id specifically" distinct from
              // closeRight (the slot is shared across all custom panels).
              if (p.action === 'open') usePanelStore.getState().openCustomPanel(p.panel.slice('custom:'.length))
              else usePanelStore.getState().closeRight()
            } else if (p.panel === 'terminal') {
              if (p.action === 'open') usePanelStore.getState().openBottom()
              else usePanelStore.getState().closeBottom()
            } else if (p.panel === 'documents') {
              // Documents are a mode of the Preview panel.
              if (p.action === 'open') {
                usePanelStore.getState().openRight('preview')
                usePanelStore.getState().setPreviewMode('documents')
              } else {
                usePanelStore.getState().closeRight('preview')
              }
            } else if (p.panel === 'preview_dev' || p.panel === 'preview_browser') {
              if (p.action === 'open') {
                usePanelStore.getState().openRight('preview')
                usePanelStore.getState().setPreviewMode(p.panel === 'preview_dev' ? 'dev' : 'browser')
              } else {
                usePanelStore.getState().closeRight()
              }
            } else if ((RIGHT_PANEL_KINDS as string[]).includes(p.panel)) {
              if (p.action === 'open') usePanelStore.getState().openRight(p.panel as RightPanelKind)
              else usePanelStore.getState().closeRight()
            }
            break
          }
          default:
            break
        }
      })
    },

    sendMessage: async (text: string, images) => {
      const trimmed = text.trim()
      if (!trimmed && !images?.length) return
      // Sending while a turn is already running no longer blocks — the message
      // goes to the steering queue instead, so typing and submitting mid-task
      // doesn't end the current run (mirrors the old app's steer-while-busy).
      // Secondary threads have no steering queue yet (not backend-scoped) —
      // block instead of silently queuing into the wrong thread.
      if (get().isBusy) {
        if (!isPrimary(threadId)) {
          console.warn('[agent] steering queue is not yet thread-scoped — ignoring send while a secondary thread is busy')
          return
        }
        await call('cmd_agent_steering_queue', { text: trimmed })
        await get().refreshQueues()
        return
      }
      // No optimistic append here: the backend emits its own `MessageStart`
      // for the user's own prompt at the top of the turn (`loop_core.rs`
      // echoes every message in `prompts`, including the user's), so an
      // optimistic append here duplicated every sent message once that event
      // landed. `isBusy` alone gives instant composer feedback.
      set({ isBusy: true })
      try {
        await call('cmd_agent_prompt', { text: trimmed, images: images?.length ? images : null, threadId })
      } catch (err) {
        console.error('[agent] send failed:', err)
        set({ isBusy: false })
      }
      // `isBusy` also clears from the `agent_end` event above — this is a
      // safety net so a dropped event can never leave the composer stuck
      // (the exact class of bug fixed in the old app's equivalent path).
      set({ isBusy: false })
    },

    refreshQueues: async () => {
      if (!isPrimary(threadId)) return
      // Old frontend read camelCase `followUp`; Rust serializes `follow_up`
      // unless the struct renames. Read both defensively rather than guessing.
      const snap = await call<{ steering?: AgentMessage[]; followUp?: AgentMessage[]; follow_up?: AgentMessage[] }>('cmd_agent_get_queues')
      if (!snap) return
      set({ queuedMessages: { steering: snap.steering ?? [], followUp: snap.followUp ?? snap.follow_up ?? [] } })
    },

    removeQueued: async (queue, index) => {
      if (!isPrimary(threadId)) return
      // Backend accepts "steering" and either "followUp"/"follow_up" — the
      // old app used "followUp", kept here for wire compatibility.
      const wireQueue = queue === 'steering' ? 'steering' : 'followUp'
      await call('cmd_agent_remove_queued', { queue: wireQueue, index })
      await get().refreshQueues()
    },

    clearQueues: async () => {
      if (!isPrimary(threadId)) return
      await call('cmd_agent_clear_queues')
      set({ queuedMessages: { steering: [], followUp: [] } })
    },

    queueFollowUp: async (text) => {
      if (!isPrimary(threadId)) return
      const trimmed = text.trim()
      if (!trimmed) return
      await call('cmd_agent_follow_up_queue', { text: trimmed })
      await get().refreshQueues()
    },

    steer: async (text) => {
      if (!isPrimary(threadId)) {
        console.warn('[agent] steer is not yet thread-scoped — ignoring on a secondary thread')
        return
      }
      const trimmed = text.trim()
      if (!trimmed) return
      set({ isBusy: true })
      try {
        await call('cmd_agent_steer', { text: trimmed })
      } catch (err) {
        console.error('[agent] steer failed:', err)
        set({ isBusy: false })
      }
    },

    abort: async () => {
      if (!isPrimary(threadId)) {
        console.warn('[agent] abort is not yet thread-scoped — ignoring on a secondary thread')
        return
      }
      try {
        await call('cmd_agent_abort')
      } catch (err) {
        console.error('[agent] abort failed:', err)
      }
      set({ isBusy: false })
    },

    revertToIndex: async (index) => {
      if (!isPrimary(threadId)) return
      await call('cmd_agent_revert_to_index', { index })
      const messages = await call<AgentMessage[]>('cmd_agent_messages', { threadId })
      set({ messages: messages ?? [], streamingIndex: null })
    },

    compactNow: async () => {
      if (!isPrimary(threadId)) return 0
      if (get().compacting || get().isBusy) return 0
      set({ compacting: true })
      try {
        const folded = (await call<number>('cmd_compact_now')) ?? 0
        // Compaction rewrites the transcript server-side (drains the folded
        // messages, inserts one summary in their place) but emits no event, so
        // the local copy has to be re-fetched rather than patched — and the
        // context gauge has to be re-read since shrinking it is the whole point.
        const messages = await call<AgentMessage[]>('cmd_agent_messages', { threadId })
        set({ messages: messages ?? [], streamingIndex: null })
        await useModelStore.getState().refreshStats()
        return folded
      } finally {
        set({ compacting: false })
      }
    },

    // Cleared optimistically here for instant UI feedback rather than waiting
    // on the `*_resolved` event round-trip; that event still fires and is a
    // harmless no-op against the already-cleared state (or the real clear, if
    // the request was resolved from elsewhere — e.g. bypass mode toggled on).
    respondPermission: async (approved) => {
      if (!isPrimary(threadId)) return
      const p = get().pendingPermission
      if (!p) return
      set({ pendingPermission: null })
      await call('cmd_agent_respond_permission', { requestId: p.requestId, approved })
    },

    answerQuestion: async (answers) => {
      if (!isPrimary(threadId)) return
      const p = get().pendingQuestion
      if (!p) return
      set({ pendingQuestion: null })
      await call('cmd_agent_answer_question', { requestId: p.requestId, answers })
    },

    respondConfirmation: async (approved) => {
      if (!isPrimary(threadId)) return
      const p = get().pendingConfirmation
      if (!p) return
      set({ pendingConfirmation: null })
      await call('cmd_agent_respond_confirmation', { requestId: p.requestId, approved })
    },
  }
  })
}

/** The app's one live conversation — the active project's today.
 *
 * `createAgentStore` still takes a thread id because the backend's
 * `AgentRegistry` is keyed that way (Viv Instant runs on its own id), but the
 * frontend no longer opens secondary threads: split panes and tabs went away
 * with daily conversations. */
export const useAgentStore = createAgentStore(PRIMARY_THREAD_ID)
