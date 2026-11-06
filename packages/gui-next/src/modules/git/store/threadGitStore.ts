import { create } from 'zustand'
import { call } from '../../../ipc'
import { onTurnEnd } from '../../ai/store/agentStore'
import type { GitSummary } from './gitStatusStore'

/** How long a folder's state is trusted before git is asked again. */
const TTL = 20_000

interface ThreadGitState {
  /** Folder → the state of the repository it sits in; null where the folder
   * is not a repository at all. */
  byPath: Record<string, GitSummary | null>
  /** When each folder was last asked, so renders don't stampede git. */
  askedAt: Record<string, number>
  /** Ask about these folders, unless they were asked about moments ago. */
  ensure: (paths: Array<string | undefined>) => void
  /** Make every known folder due again; the next `ensure` refetches it. */
  expire: () => void
}

async function summaryOf(path: string): Promise<GitSummary | null> {
  try {
    return (await call<GitSummary | null>('cmd_git_summary', { path })) ?? null
  } catch {
    // Not a repository, or git isn't installed: the row just has no git line.
    return null
  }
}

/**
 * Where each thread's folder stands in git, for the threads list.
 *
 * The open project's repository is already tracked by `useGitStatus`; this is
 * the same question asked of the other folders a list of threads can span —
 * General's threads, whose folders differ — and answered once per folder
 * rather than once per row.
 */
export const useThreadGit = create<ThreadGitState>((set, get) => ({
  byPath: {},
  askedAt: {},

  ensure: (paths) => {
    const now = Date.now()
    const { askedAt } = get()
    const wanted = [...new Set(paths.filter((p): p is string => !!p))].filter((p) => now - (askedAt[p] ?? 0) > TTL)
    if (!wanted.length) return
    // Count them as asked at once: a burst of renders must not queue a fetch
    // each, and a slow git must not be asked twice for the same folder.
    set((s) => ({ askedAt: { ...s.askedAt, ...Object.fromEntries(wanted.map((p) => [p, now])) } }))
    for (const path of wanted) {
      void summaryOf(path).then((summary) => set((s) => ({ byPath: { ...s.byPath, [path]: summary } })))
    }
  },

  expire: () => set({ askedAt: {} }),
}))

let started = false
/** Keep the threads' git lines fresh (once, at startup): the agent commits
 * and pushes while the list is on screen. */
export function startThreadGit() {
  if (started) return
  started = true
  const refreshKnown = () => {
    if (document.visibilityState !== 'visible') return
    const { byPath, expire, ensure } = useThreadGit.getState()
    const paths = Object.keys(byPath)
    if (!paths.length) return
    expire()
    ensure(paths)
  }
  window.setInterval(refreshKnown, TTL)
  onTurnEnd(refreshKnown)
}
