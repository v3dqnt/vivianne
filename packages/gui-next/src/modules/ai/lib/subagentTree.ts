/**
 * The subagent list as a tree: each child under the agent that started it, newest first within a level.
 *
 * A subagent whose parent is gone from the list (or never named one) sits at the top level, so nothing disappears.
 */
export interface TreeNode {
  id: string
  name: string
  parent: string | null
  startedAtEpochMs: number
}

export function subagentTree<T extends TreeNode>(agents: readonly T[]): { agent: T; depth: number }[] {
  const byRef = new Map<string, T>()
  for (const a of agents) {
    byRef.set(a.id, a)
    byRef.set(a.name, a)
  }
  const parentOf = (a: T): T | undefined => {
    const p = a.parent ? byRef.get(a.parent) : undefined
    return p && p !== a ? p : undefined
  }
  const children = new Map<T | undefined, T[]>()
  for (const a of agents) {
    const p = parentOf(a)
    children.set(p, [...(children.get(p) ?? []), a])
  }
  const out: { agent: T; depth: number }[] = []
  const seen = new Set<T>()
  const walk = (parent: T | undefined, depth: number) => {
    for (const a of [...(children.get(parent) ?? [])].sort((x, y) => y.startedAtEpochMs - x.startedAtEpochMs)) {
      if (seen.has(a)) continue // a parent cycle cannot loop forever
      seen.add(a)
      out.push({ agent: a, depth })
      walk(a, depth + 1)
    }
  }
  walk(undefined, 0)
  // Anything only reachable through a cycle still shows up.
  for (const a of agents) if (!seen.has(a)) out.push({ agent: a, depth: 0 })
  return out
}

const STOP_LABEL: Record<string, string> = {
  completed: 'finished',
  aborted: 'stopped',
  error: 'failed',
  'max-tokens': 'ran out of tokens',
  refusal: 'declined',
}

/** Plain words for how a finished child ended; null while it runs or when the backend did not say. */
export function stopReasonLabel(reason: string | null | undefined): string | null {
  return reason ? (STOP_LABEL[reason] ?? reason) : null
}
