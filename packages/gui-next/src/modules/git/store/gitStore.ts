import { create } from 'zustand'
import { call } from '../../../ipc'
import { useWorkspaceStore } from '../../ai/store/workspaceStore'

export interface GitCommit {
  hash: string
  author: string
  date: string
  message: string
}

export interface GitBranch {
  name: string
  isCurrent: boolean
  isRemote: boolean
}

export interface GitStatusEntry {
  path: string
  status: string
}

export interface GitRemote {
  url: string
  githubRepo: string | null
  githubUrl: string | null
}

interface GitStoreState {
  root: string | null
  remote: GitRemote | null
  commits: GitCommit[]
  branches: GitBranch[]
  status: GitStatusEntry[]
  loading: boolean
  refresh: (root: string) => Promise<void>
  /** Switch to an existing branch. Throws with git's own message on failure
   * (e.g. uncommitted changes in the way) — callers show it, they don't swallow it. */
  checkoutBranch: (root: string, branch: string) => Promise<void>
  createBranch: (root: string, name: string) => Promise<void>
}

/** Read-only git info for the workspace's primary folder — commit log,
 * branches, working-tree status. Backed by `cmd_git_log`/`cmd_git_branches`/
 * `cmd_git_status`, which all shell out to `git` (matching the existing
 * `cmd_git_current_branch` pattern) and degrade to an empty list rather than
 * erroring when the folder isn't a repo. */
export const useGitStore = create<GitStoreState>((set) => ({
  root: null,
  remote: null,
  commits: [],
  branches: [],
  status: [],
  loading: false,

  refresh: async (root) => {
    set({ loading: true, root })
    try {
      const [remote, commits, branches, status] = await Promise.all([
        call<GitRemote | null>('cmd_git_remote', { path: root }),
        call<GitCommit[]>('cmd_git_log', { path: root, limit: 50 }),
        call<GitBranch[]>('cmd_git_branches', { path: root }),
        call<GitStatusEntry[]>('cmd_git_status', { path: root }),
      ])
      set({ remote: remote ?? null, commits: commits ?? [], branches: branches ?? [], status: status ?? [] })
    } finally {
      set({ loading: false })
    }
  },

  checkoutBranch: async (root, branch) => {
    await call('cmd_git_checkout_branch', { path: root, branch })
    await Promise.all([useGitStore.getState().refresh(root), useWorkspaceStore.getState().refresh()])
  },

  createBranch: async (root, name) => {
    await call('cmd_git_create_branch', { path: root, name })
    await Promise.all([useGitStore.getState().refresh(root), useWorkspaceStore.getState().refresh()])
  },
}))
