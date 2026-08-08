import type { SessionInfo } from './types'

export interface WorkspaceGroup {
  /** The full primary workspace path, or `null` for sessions with none. */
  path: string | null
  /** Last path segment, for display — `path` itself is shown as a tooltip. */
  label: string
  sessions: SessionInfo[]
}

/** Works for both `C:\foo\bar` and `/foo/bar` — sessions can come from either OS. */
function basename(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean)
  return parts[parts.length - 1] ?? path
}

/**
 * Groups sessions by their primary (first) workspace folder, most-recently-
 * active workspace first. Sessions with no recorded workspace (saved before
 * that field existed, or truly run with none) fall into a trailing
 * `path: null` group rather than being silently dropped.
 */
export function groupByWorkspace(sessions: SessionInfo[]): WorkspaceGroup[] {
  const byPath = new Map<string | null, SessionInfo[]>()

  for (const session of sessions) {
    const primary = session.workspace_folders[0] ?? null
    const bucket = byPath.get(primary)
    if (bucket) bucket.push(session)
    else byPath.set(primary, [session])
  }

  const groups: WorkspaceGroup[] = Array.from(byPath.entries()).map(([path, list]) => ({
    path,
    label: path ? basename(path) : 'No workspace',
    sessions: [...list].sort((a, b) => b.created_at.localeCompare(a.created_at)),
  }))

  groups.sort((a, b) => {
    if (a.path === null) return 1
    if (b.path === null) return -1
    return b.sessions[0].created_at.localeCompare(a.sessions[0].created_at)
  })

  return groups
}
