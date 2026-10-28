import { useEffect, useState } from 'react'
import { call } from '../../../ipc'
import { useGitStatus } from '../../git/store/gitStatusStore'
import { mergeChanges, type FileChange, type GitFileChange } from './fileTree'

/**
 * Per-file added and removed line counts for every workspace root, keyed by
 * the path git reports (relative to the repository root — matched to a row by
 * `changeFor`).
 *
 * Refreshed whenever the git status store refreshes: it polls every five
 * seconds and again after every agent turn, so this rides along rather than
 * running a second timer.
 */
export function useFileChanges(roots: string[]): Map<string, FileChange> {
  const [changes, setChanges] = useState<Map<string, FileChange>>(new Map())
  // The store hands back a fresh summary object on every refresh, so its
  // identity is the "git moved" signal.
  const summary = useGitStatus((s) => s.summary)
  const key = roots.join(' ')

  useEffect(() => {
    if (roots.length === 0) {
      setChanges(new Map())
      return
    }
    let cancelled = false
    void Promise.all(
      roots.map(async (root) => {
        const [staged, unstaged] = await Promise.all([
          call<GitFileChange[]>('cmd_git_diff_stat', { path: root, staged: true }).catch(() => []),
          call<GitFileChange[]>('cmd_git_diff_stat', { path: root, staged: false }).catch(() => []),
        ])
        return mergeChanges(staged ?? [], unstaged ?? [])
      }),
    ).then((maps) => {
      if (cancelled) return
      const merged = new Map<string, FileChange>()
      for (const m of maps) {
        for (const [path, c] of m) {
          const prev = merged.get(path)
          merged.set(
            path,
            prev
              ? { additions: prev.additions + c.additions, deletions: prev.deletions + c.deletions, status: c.status || prev.status }
              : c,
          )
        }
      }
      setChanges(merged)
    })
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, summary])

  return changes
}
