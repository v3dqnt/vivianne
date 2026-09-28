/**
 * Notifies open IDE editors when a file they have open was written by
 * something other than the editor itself — almost always the agent.
 *
 * There's no filesystem-watcher plumbing from Rust to the frontend, so this
 * doesn't learn about changes made outside the app (another editor, git).
 * What it DOES catch is the case that actually matters here: the agent edits
 * a file you have open in the IDE view while you're looking at it. Without
 * this, saving your own unrelated edit a moment later would silently
 * overwrite whatever the agent just wrote, and you'd have no idea it had
 * changed underneath you at all.
 *
 * Wired from `agentStore`'s `tool_execution_end` handler for
 * write/edit/apply_patch/notebook_edit — see the call site there.
 */
type Listener = () => void

const listeners = new Map<string, Set<Listener>>()

/** Normalizes path separators so a listener registered with one slash style
 * still matches a notification that used the other — Windows tool output and
 * a path typed/selected in the UI don't always agree. */
function norm(path: string): string {
  return path.replace(/\\/g, '/').toLowerCase()
}

/**
 * `write`/`edit`/`apply_patch`/`notebook_edit` report the RAW path argument
 * the model passed — which is very often relative to the workspace root, not
 * absolute — while every IDE tab is keyed by the absolute path
 * `cmd_list_dir_tree` handed it. Comparing those directly would silently
 * never match for a relative path, which is most of them; a data-safety
 * feature that only works some of the time is worse than an obviously-broken
 * one, since it teaches you to trust a warning that quietly won't always
 * come.
 *
 * `resolve_path` on the Rust side already collapses this the same way — join
 * onto the workspace root unless the string already looks absolute — so this
 * mirrors that rule rather than reimplementing path resolution generally.
 */
export function resolveAgentPath(raw: string, workspaceRoot: string | null): string {
  const looksAbsolute = /^[a-zA-Z]:[\\/]/.test(raw) || raw.startsWith('/') || raw.startsWith('\\\\')
  if (looksAbsolute || !workspaceRoot) return raw
  const sep = workspaceRoot.includes('\\') ? '\\' : '/'
  return `${workspaceRoot.replace(/[\\/]+$/, '')}${sep}${raw.replace(/^[\\/]+/, '')}`
}

export function notifyFileChanged(path: string): void {
  const set = listeners.get(norm(path))
  if (!set) return
  for (const fn of set) fn()
}

/** Call in a `useEffect`; returns the unsubscribe function. */
export function subscribeFileChanged(path: string, fn: Listener): () => void {
  const key = norm(path)
  let set = listeners.get(key)
  if (!set) {
    set = new Set()
    listeners.set(key, set)
  }
  set.add(fn)
  return () => {
    set!.delete(fn)
    if (set!.size === 0) listeners.delete(key)
  }
}
