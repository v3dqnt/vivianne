import { create } from 'zustand'
import { call } from '../../../ipc'
import { useWorkspaceStore } from '../../ai/store/workspaceStore'
import { onTurnEnd } from '../../ai/store/agentStore'

/** Mirrors `agent::changes::git::GitSummary`. */
export interface GitSummary {
  branch: string
  upstream: string | null
  ahead: number
  behind: number
  staged: number
  unstaged: number
  untracked: number
  conflicted: number
  defaultBranch: string | null
  lastCommit: string | null
}

/** The pull request for the current branch, with its checks rolled up. */
export interface BranchPr {
  number: number
  title: string
  state: 'open' | 'closed' | 'merged'
  draft: boolean
  url: string
  additions: number
  deletions: number
  /** passing, failing, running, or none when the head has no checks. */
  checks: 'passing' | 'failing' | 'running' | 'none'
  failing: string[]
}

interface GitStatusState {
  root: string | null
  summary: GitSummary | null
  repo: string | null
  pr: BranchPr | null
  openPrs: number | null
  openIssues: number | null
  failingRuns: number | null
  refresh: () => Promise<void>
  refreshGithub: () => Promise<void>
}

const changedCount = (s: GitSummary) => s.staged + s.unstaged + s.untracked + s.conflicted

/** What needs attention, for the title bar's Git tab: red for conflicts or
 * failing checks, amber for work not yet committed or pushed. */
export function attention(s: GitSummary | null, pr: BranchPr | null): 'red' | 'amber' | null {
  if (!s) return null
  if (s.conflicted > 0 || pr?.checks === 'failing') return 'red'
  if (changedCount(s) > 0 || s.ahead > 0 || s.behind > 0) return 'amber'
  return null
}

export { changedCount }

/**
 * Where the open project's repository stands, kept fresh: the local state
 * every few seconds and after every agent turn, GitHub (this branch's pull
 * request and its checks, open pull requests and issues, failing runs) less
 * often.
 */
export const useGitStatus = create<GitStatusState>((set, get) => ({
  root: null,
  summary: null,
  repo: null,
  pr: null,
  openPrs: null,
  openIssues: null,
  failingRuns: null,

  refresh: async () => {
    const root = useWorkspaceStore.getState().folders[0] ?? null
    if (!root) return set({ root: null, summary: null, repo: null, pr: null })
    const summary = (await call<GitSummary | null>('cmd_git_summary', { path: root }).catch(() => null)) ?? null
    const moved = root !== get().root || summary?.branch !== get().summary?.branch
    set({ root, summary })
    if (moved) void get().refreshGithub()
  },

  refreshGithub: async () => {
    const { root, summary } = get()
    if (!root || !summary) return
    const gh = <T,>(path: string) => call<T>('cmd_github', { cwd: root, path, method: null, body: null, raw: null })
    try {
      const repo = (await call<string>('cmd_github_repo', { cwd: root })) ?? null
      if (!repo) return set({ repo: null, pr: null })
      const owner = repo.split('/')[0]
      const [prs, open, info, runs] = await Promise.all([
        gh<Array<Record<string, any>>>(`/repos/{repo}/pulls?head=${owner}:${encodeURIComponent(summary.branch)}&state=all&per_page=1`),
        gh<Array<unknown>>('/repos/{repo}/pulls?state=open&per_page=100'),
        gh<Record<string, any>>('/repos/{repo}'),
        gh<Record<string, any>>(`/repos/{repo}/actions/runs?branch=${encodeURIComponent(summary.branch)}&per_page=10`),
      ])
      const p = prs?.[0]
      let pr: BranchPr | null = null
      if (p) {
        const full = await gh<Record<string, any>>(`/repos/{repo}/pulls/${p.number}`)
        const checkRuns = ((await gh<Record<string, any>>(`/repos/{repo}/commits/${p.head.sha}/check-runs?per_page=100`))?.check_runs ?? []) as Array<Record<string, any>>
        const failing = checkRuns.filter((c) => c.conclusion === 'failure' || c.conclusion === 'timed_out').map((c) => c.name as string)
        const running = checkRuns.some((c) => c.status !== 'completed')
        pr = {
          number: p.number,
          title: p.title,
          state: p.merged_at ? 'merged' : p.state,
          draft: !!p.draft,
          url: p.html_url,
          additions: full?.additions ?? 0,
          deletions: full?.deletions ?? 0,
          checks: failing.length ? 'failing' : running ? 'running' : checkRuns.length ? 'passing' : 'none',
          failing,
        }
      }
      const latest = new Map<string, Record<string, any>>()
      for (const r of (runs?.workflow_runs ?? []) as Array<Record<string, any>>) if (!latest.has(r.name)) latest.set(r.name, r)
      const openPrs = open?.length ?? 0
      set({
        repo,
        pr,
        openPrs,
        // GitHub counts pull requests as issues too.
        openIssues: Math.max(0, (info?.open_issues_count ?? 0) - openPrs),
        failingRuns: [...latest.values()].filter((r) => r.conclusion === 'failure').length,
      })
    } catch {
      // No GitHub remote or no credentials: the local state still shows.
      set({ pr: null })
    }
  },
}))

let started = false
/** Start keeping the repository's state fresh (once, at startup). */
export function startGitStatus() {
  if (started) return
  started = true
  const { refresh, refreshGithub } = useGitStatus.getState()
  void refresh()
  window.setInterval(() => document.visibilityState === 'visible' && void refresh(), 5000)
  window.setInterval(() => document.visibilityState === 'visible' && void refreshGithub(), 45000)
  useWorkspaceStore.subscribe((s, prev) => {
    if (s.folders[0] !== prev.folders[0]) void refresh()
  })
  // The agent commits, pushes and opens pull requests: show it at once.
  onTurnEnd(() => {
    void refresh()
    void refreshGithub()
  })
}
