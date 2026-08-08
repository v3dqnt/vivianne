import { create } from 'zustand'
import { call, on } from '../../../ipc'
import type { AgentEvent, AgentMessage, ImageAttachment, MessageContent } from '../lib/types'
import { useModelStore } from './modelStore'

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
  init: () => Promise<void>
  sendMessage: (text: string, images?: ImageAttachment[]) => Promise<void>
  /** Fold older history into a summary. Returns how many messages were folded (0 = nothing to compact). */
  compactNow: () => Promise<number>
}

export const useAgentStore = create<AgentStoreState>((set, get) => ({
  messages: [],
  isBusy: false,
  streamingIndex: null,
  initialized: false,
  compacting: false,

  init: async () => {
    if (get().initialized) return
    set({ initialized: true })

    await on<AgentEvent>('agent_event', (event) => {
      switch (event.type) {
        case 'message_start': {
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
            if (s.streamingIndex === null) return s
            const messages = [...s.messages]
            messages[s.streamingIndex] = event.payload.message
            return { messages, streamingIndex: null }
          })
          break
        }
        case 'agent_end': {
          set({ isBusy: false, streamingIndex: null })
          void useModelStore.getState().refreshStats()
          break
        }
        case 'tool_execution_start': {
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
          set((s) => {
            const idx = s.messages.findIndex(
              (m) => m.type === 'toolResult' && m.toolCallId === event.payload.tool_call_id,
            )
            if (idx === -1) return s
            const messages = [...s.messages]
            const prev = messages[idx]
            if (prev.type !== 'toolResult') return s
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
        default:
          break
      }
    })
  },

  sendMessage: async (text: string, images) => {
    const trimmed = text.trim()
    if ((!trimmed && !images?.length) || get().isBusy) return
    // No optimistic append here: the backend emits its own `MessageStart`
    // for the user's own prompt at the top of the turn (`loop_core.rs`
    // echoes every message in `prompts`, including the user's), so an
    // optimistic append here duplicated every sent message once that event
    // landed. `isBusy` alone gives instant composer feedback.
    set({ isBusy: true })
    try {
      await call('cmd_agent_prompt', { text: trimmed, images: images?.length ? images : null })
    } catch (err) {
      console.error('[agent] send failed:', err)
      set({ isBusy: false })
    }
    // `isBusy` also clears from the `agent_end` event above — this is a
    // safety net so a dropped event can never leave the composer stuck
    // (the exact class of bug fixed in the old app's equivalent path).
    set({ isBusy: false })
  },

  compactNow: async () => {
    if (get().compacting || get().isBusy) return 0
    set({ compacting: true })
    try {
      const folded = (await call<number>('cmd_compact_now')) ?? 0
      // Compaction rewrites the transcript server-side (drains the folded
      // messages, inserts one summary in their place) but emits no event, so
      // the local copy has to be re-fetched rather than patched — and the
      // context gauge has to be re-read since shrinking it is the whole point.
      const messages = await call<AgentMessage[]>('cmd_agent_messages')
      set({ messages: messages ?? [], streamingIndex: null })
      await useModelStore.getState().refreshStats()
      return folded
    } finally {
      set({ compacting: false })
    }
  },
}))
