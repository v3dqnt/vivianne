import { contentImages, contentToText, type AgentMessage } from '../../ai/lib/types.ts'
import { isAppText, isPlanNoteText, isSummaryText } from '../../ai/lib/appText.ts'

/**
 * The rules of a 24/7 agent's messenger chat, kept apart from its drawing so
 * they can be read and tested on their own.
 */

/** What the composer's round button does. */
export type ComposerButton = 'send' | 'send-disabled' | 'stop'

/**
 * Typed text always sends (mid-run it is queued for the agent). With nothing
 * typed, the button stops the agent if it is working at all: started from
 * this chat (`chatBusy`) or by a routine, a check-in or another agent, which
 * only its status (`agentState`) knows about.
 */
export function composerButton(s: { chatBusy: boolean; agentState: string; typed: string }): ComposerButton {
  if (s.typed.trim()) return 'send'
  if (s.chatBusy || s.agentState === 'working') return 'stop'
  return 'send-disabled'
}

export type ToolResult = Extract<AgentMessage, { type: 'toolResult' }>
export type Image = ReturnType<typeof contentImages>[number]

/** One thing drawn in the conversation, in order. */
export type Item =
  | { kind: 'bubble'; from: 'you' | 'agent'; key: string; text: string; images: Image[]; ts: number; by?: string }
  | { kind: 'activity'; key: string; tools: ToolResult[] }
  | { kind: 'note'; key: string; text: string }
  /** Something it offers to do, for your yes (`bots::proactive`). */
  | { kind: 'suggestion'; key: string; text: string }
  | { kind: 'day'; key: string; label: string }

function dayLabel(ts: number): string {
  const d = new Date(ts)
  const today = new Date()
  const yesterday = new Date(Date.now() - 86_400_000)
  if (d.toDateString() === today.toDateString()) return 'Today'
  if (d.toDateString() === yesterday.toDateString()) return 'Yesterday'
  return d.toLocaleDateString(undefined, { weekday: 'short', month: 'short', day: 'numeric' })
}

/** Another agent's words, as the app delivers them into this thread: an
 * answer (`<agent_message from=…>`), a question it asked, or the user
 * mentioning this agent elsewhere. */
const AGENT_MESSAGE = /^\s*<agent_message from="([^"]+)"[^>]*>\n?([\s\S]*?)\n?<\/agent_message>/
const ASKED = /^(.+?) \(@([\w-]+)\), another agent, asks you:\n\n([\s\S]*?)\n\nYour answer is sent back to/
const MENTIONED = /^The user mentioned you in (.+?)'s chat:\n\n([\s\S]*?)\n\nYour answer goes into that chat/
/** What the app asks it on its own schedule (`bots::CHECKIN_PROMPT`, `MORNING_PROMPT`). */
const PROACTIVE = /^(Check-in|Morning brief)\. /
/** The line it ends a reply with to offer something (`proactive::suggestion_in`). */
const SUGGEST = /^\s*SUGGEST:[ \t]*(.+)$/m

/** The transcript as a messenger reads it: what each side said, the agent's
 * tool work folded into one line between messages, reasoning left out. */
export function toItems(messages: AgentMessage[]): Item[] {
  const items: Item[] = []
  let tools: ToolResult[] = []
  let lastDay = ''
  const flushTools = () => {
    if (tools.length) items.push({ kind: 'activity', key: `a${items.length}`, tools })
    tools = []
  }
  const day = (ts: number) => {
    const label = dayLabel(ts)
    if (ts && label !== lastDay) {
      lastDay = label
      items.push({ kind: 'day', key: `d${items.length}`, label })
    }
  }
  messages.forEach((m, i) => {
    if (m.type === 'toolResult') {
      tools.push(m)
      return
    }
    const text = contentToText(m.content).trim()
    const images = contentImages(m.content)
    if (m.type === 'user') {
      flushTools()
      day(m.timestamp)
      // Another agent speaking here: its bubble, with its face.
      const answer = AGENT_MESSAGE.exec(text)
      const asked = ASKED.exec(text)
      const mentioned = MENTIONED.exec(text)
      if (answer) items.push({ kind: 'bubble', from: 'agent', by: answer[1], key: `m${i}`, text: answer[2].trim(), images: [], ts: m.timestamp })
      else if (asked) items.push({ kind: 'bubble', from: 'agent', by: asked[2], key: `m${i}`, text: asked[3].trim(), images: [], ts: m.timestamp })
      else if (mentioned) {
        items.push({ kind: 'note', key: `n${i}`, text: `In ${mentioned[1]}'s chat` })
        items.push({ kind: 'bubble', from: 'you', key: `m${i}`, text: mentioned[2].trim(), images: [], ts: m.timestamp })
      }
      // The app's own notes ride in user messages: shown as a quiet line, never as your bubble.
      else if (PROACTIVE.test(text)) items.push({ kind: 'note', key: `n${i}`, text: PROACTIVE.exec(text)![1] })
      else if (isPlanNoteText(text)) items.push({ kind: 'note', key: `n${i}`, text: 'You answered the plan' })
      else if (isSummaryText(text)) items.push({ kind: 'note', key: `n${i}`, text: 'Earlier messages were summarised' })
      else if (isAppText(text) || text.startsWith('<system-reminder>')) return
      else items.push({ kind: 'bubble', from: 'you', key: `m${i}`, text, images, ts: m.timestamp })
      return
    }
    // Failed turns are listed behind the errors icon by the composer.
    if (m.stopReason === 'error') return
    if (!text && images.length === 0) return
    flushTools()
    day(m.timestamp)
    const suggestion = SUGGEST.exec(text)
    const said = suggestion ? text.replace(SUGGEST, '').trim() : text
    if (said || images.length) items.push({ kind: 'bubble', from: 'agent', key: `m${i}`, text: said, images, ts: m.timestamp })
    if (suggestion) items.push({ kind: 'suggestion', key: `s${i}`, text: suggestion[1].trim() })
  })
  flushTools()
  return items
}
