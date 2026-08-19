import { useWorkspaceStore } from '../ai/store/workspaceStore'
import { useThreadsStore } from '../sessions/store/threadsStore'

/**
 * The workspace the dock panels should be showing: the focused thread pane's
 * own workspace when one is focused, otherwise the app-global workspace (the
 * primary sidebar chat's). One hook so Files/Preview/Git/Terminal can't drift
 * apart on which workspace they think is active.
 */
export function useActiveWorkspace(): string[] {
  const globalFolders = useWorkspaceStore((s) => s.folders)
  const focusedId = useThreadsStore((s) => s.focusedId)
  const workspaces = useThreadsStore((s) => s.workspaces)
  const threadFolders = focusedId ? workspaces[focusedId] : undefined
  return threadFolders && threadFolders.length > 0 ? threadFolders : globalFolders
}

/** Convenience: the primary (first) folder, or null. */
export function useActiveWorkspaceRoot(): string | null {
  return useActiveWorkspace()[0] ?? null
}
