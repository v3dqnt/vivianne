import type { AgentMessage } from './types'

type ToolResultMessage = Extract<AgentMessage, { type: 'toolResult' }>

export interface TodoItem {
  content: string
  status: 'pending' | 'in_progress' | 'completed' | 'cancelled'
  /** One line on what was done — the agent fills it in on completion. */
  summary?: string
}

/** A point where the agent finished part of its plan. */
export interface Checkpoint {
  /** Index of the `todo_write` result that marked the items done. */
  index: number
  at: number
  /** The items finished at this point, in plan order. */
  items: TodoItem[]
  /** 1-based positions of those items in the plan. */
  positions: number[]
  total: number
  done: number
  /** Files written or edited since the previous checkpoint. */
  files: string[]
  elapsedMs: number
  /** Spend since the previous checkpoint, when the provider reported it. */
  costUsd: number | null
}

const EDIT_TOOLS = new Set(['write', 'edit', 'apply_patch', 'notebook_edit', 'edit_document', 'create_document'])

export function todosOf(m: AgentMessage): TodoItem[] | null {
  if (m.type !== 'toolResult' || m.isError === true) return null
  if (m.toolName !== 'todo_write' && m.toolName !== 'todo_write_rich') return null
  const raw = m.arguments?.todos
  if (!Array.isArray(raw)) return null
  return raw.map((t) => {
    const o = (t ?? {}) as { content?: string; subject?: string; status?: string; summary?: string }
    return {
      content: o.content ?? o.subject ?? '',
      status: (o.status ?? 'pending') as TodoItem['status'],
      summary: typeof o.summary === 'string' && o.summary.trim() ? o.summary.trim() : undefined,
    }
  })
}

function editedPaths(m: ToolResultMessage): string[] {
  if (!EDIT_TOOLS.has(m.toolName) || m.isError !== false) return []
  const a = m.arguments ?? {}
  for (const k of ['path', 'file_path', 'notebook_path']) if (typeof a[k] === 'string') return [a[k] as string]
  const diff = (m.details as { filesDiff?: { path?: unknown }[] } | null)?.filesDiff
  return Array.isArray(diff) ? diff.map((d) => d?.path).filter((p): p is string => typeof p === 'string') : []
}

/**
 * Every checkpoint in a transcript, keyed by the index it happened at.
 *
 * Derived from the plan the agent keeps with `todo_write`: an item that turns
 * `completed` is a checkpoint. Nothing extra is stored — the plan is already
 * in the transcript, so checkpoints survive reloads and show the same in a
 * past conversation as they did live.
 */
export function findCheckpoints(messages: AgentMessage[]): Map<number, Checkpoint> {
  const out = new Map<number, Checkpoint>()
  let completed = new Set<string>()
  let since = messages[0]?.timestamp ?? 0
  let files = new Set<string>()
  let cost = 0
  let costKnown = false

  messages.forEach((m, i) => {
    // A new request from the user starts the clock for its first checkpoint.
    if (m.type === 'user' && out.size === 0 && completed.size === 0) since = m.timestamp
    if (m.type === 'assistant' && m.usage?.cost?.total) {
      cost += m.usage.cost.total
      costKnown = true
    }
    if (m.type === 'toolResult') for (const p of editedPaths(m)) files.add(p)

    const todos = todosOf(m)
    if (!todos) return
    // A plan that shares nothing with the last one is a new plan, and its
    // items start from nothing done.
    if (todos.every((t) => !completed.has(t.content))) completed = new Set()
    const newly = todos.map((t, pos) => ({ t, pos })).filter(({ t }) => t.status === 'completed' && !completed.has(t.content))
    for (const t of todos) if (t.status === 'completed') completed.add(t.content)
    if (newly.length === 0) return
    out.set(i, {
      index: i,
      at: m.timestamp,
      items: newly.map(({ t }) => t),
      positions: newly.map(({ pos }) => pos + 1),
      total: todos.length,
      done: todos.filter((t) => t.status === 'completed').length,
      files: [...files],
      elapsedMs: Math.max(0, m.timestamp - since),
      costUsd: costKnown ? cost : null,
    })
    since = m.timestamp
    files = new Set()
    cost = 0
    costKnown = false
  })
  return out
}
