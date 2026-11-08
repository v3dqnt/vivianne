/** A run's trace (one JSON event per line, written by the backend's `runtime/trace.rs`) turned into what the Trajectory view draws. */

export interface TraceEvent {
  t: number
  step: number
  kind: string
  [key: string]: unknown
}

export type RowKind = 'system' | 'user' | 'reminder' | 'assistant' | 'call' | 'result' | 'compaction' | 'notice' | 'end'

export interface Row {
  id: number
  kind: RowKind
  step: number
  title: string
  /** What the row says in one line, for the list. */
  summary: string
  t: number
  /** The raw event, and for a system row the prompt and tools, for the detail drawer. */
  event: TraceEvent
  prompt?: string
  tools?: { name: string; description: string }[]
  failed?: boolean
}

export interface Turn {
  step: number
  /** Time from the step starting to the model call (compaction, shaping, building the request). */
  inputMs: number
  modelMs: number
  toolsMs: number
  model: string
  stop: string | null
  inputTokens: number | null
  outputTokens: number | null
  error: string | null
  /** Absolute start of each phase, for the timeline. */
  inputStart: number
  modelStart: number
  toolsStart: number
}

export interface Trajectory {
  rows: Row[]
  turns: Turn[]
  start: number
  end: number
}

const str = (v: unknown): string => (typeof v === 'string' ? v : v == null ? '' : String(v))
const num = (v: unknown): number => (typeof v === 'number' && Number.isFinite(v) ? v : 0)
const line = (s: string, max = 140) => {
  const one = s.replace(/\s+/g, ' ').trim()
  return one.length > max ? `${one.slice(0, max)}…` : one
}

/** A user message that is the loop's own note to the model, not something a person said. */
export const isReminder = (text: string) => /^\s*<(system-reminder|vivianne_[a-z_]+|long_task|team_message|subagent_[a-z_]+|background_result|compacted_history)/.test(text)

export function buildTrajectory(events: TraceEvent[]): Trajectory {
  const rows: Row[] = []
  const turns: Turn[] = []
  const push = (r: Omit<Row, 'id'>) => rows.push({ ...r, id: rows.length })
  const toolsDone = new Map<number, { ms: number; t: number }>()
  for (const e of events) if (e.kind === 'tools_done') toolsDone.set(e.step, { ms: num(e.ms), t: e.t })
  let sawPrompt = false

  for (const e of events) {
    switch (e.kind) {
      case 'request': {
        if (typeof e.system_prompt === 'string') {
          const tools = Array.isArray(e.tools) ? (e.tools as { name: string; description: string }[]) : undefined
          push({ kind: 'system', step: e.step, t: e.t, event: e, title: sawPrompt ? 'System prompt changed' : 'Initial System Prompt', summary: line(e.system_prompt), prompt: e.system_prompt, tools })
          sawPrompt = true
        } else if (Array.isArray(e.tools)) {
          push({ kind: 'system', step: e.step, t: e.t, event: e, title: 'Tools changed', summary: `${(e.tools as unknown[]).length} tools`, tools: e.tools as { name: string; description: string }[] })
        }
        for (const m of (Array.isArray(e.new_messages) ? e.new_messages : []) as Record<string, unknown>[]) {
          const role = str(m.role)
          const text = str(m.text)
          if (role === 'user') push({ kind: isReminder(text) ? 'reminder' : 'user', step: e.step, t: e.t, event: e, title: isReminder(text) ? 'Reminder' : 'User', summary: line(text) })
          else if (role === 'tool') push({ kind: 'result', step: e.step, t: e.t, event: { ...e, ...m }, title: `Result · ${str(m.tool)}`, summary: line(text), failed: m.failed === true })
          // The model's own messages are shown by their turn rows below, with timing and tokens.
        }
        break
      }
      case 'turn': {
        const done = toolsDone.get(e.step)
        const modelMs = num(e.model_ms)
        const inputMs = num(e.input_ms)
        const modelStart = e.t - modelMs
        const turn: Turn = {
          step: e.step,
          inputMs,
          modelMs,
          toolsMs: done?.ms ?? 0,
          model: str(e.answered_by) || str(e.model),
          stop: typeof e.stop_reason === 'string' ? e.stop_reason : null,
          inputTokens: typeof e.input_tokens === 'number' ? e.input_tokens : null,
          outputTokens: typeof e.output_tokens === 'number' ? e.output_tokens : null,
          error: typeof e.error === 'string' ? e.error : null,
          inputStart: modelStart - inputMs,
          modelStart,
          toolsStart: done ? done.t - done.ms : e.t,
        }
        turns.push(turn)
        push({
          kind: 'assistant',
          step: e.step,
          t: e.t,
          event: e,
          failed: !!turn.error,
          title: `Turn ${turns.length} · ${turn.model}${e.fallback ? ' (fallback)' : ''}`,
          summary: turn.error ? line(turn.error) : `${turn.stop ?? 'no stop reason'} · ${turn.inputTokens ?? '?'} in, ${turn.outputTokens ?? '?'} out · ${(modelMs / 1000).toFixed(1)}s`,
        })
        break
      }
      case 'tool':
        push({ kind: 'call', step: e.step, t: e.t, event: e, failed: e.failed === true, title: str(e.tool), summary: line(str(e.args) || str(e.call)) })
        break
      case 'compaction':
        push({ kind: 'compaction', step: e.step, t: e.t, event: e, title: `Compaction (${str(e.when)})`, summary: `${num(e.messages_before) || '?'} → ${num(e.messages_after)} messages` })
        break
      case 'reminder':
        push({ kind: 'notice', step: e.step, t: e.t, event: e, title: `Reminder · ${str(e.reminder)}`, summary: '' })
        break
      case 'stop':
      case 'waiting':
      case 'byte_guard':
      case 'history_shaped':
        push({ kind: 'notice', step: e.step, t: e.t, event: e, title: e.kind.replace(/_/g, ' '), summary: line(str(e.reason) || JSON.stringify(e).slice(0, 120)) })
        break
      case 'run_end':
        push({ kind: 'end', step: e.step, t: e.t, event: e, title: 'Run ended', summary: line(str(e.reason) || str(e.status) || '') })
        break
      default:
    }
  }
  const start = events.length ? (turns[0]?.inputStart ?? events[0].t) : 0
  const end = events.length ? Math.max(...events.map((e) => e.t)) : 0
  return { rows, turns, start: Math.min(start, events[0]?.t ?? start), end }
}
