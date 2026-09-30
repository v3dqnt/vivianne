import { useWorkspaceStore } from '../ai/store/workspaceStore'

/**
 * The workspace the dock panels should be showing.
 *
 * One hook so Files/Preview/Git/Terminal can't drift apart on which workspace
 * they think is active. It used to prefer a focused thread pane's own
 * workspace; with one conversation per project there is only ever one, the
 * active project's.
 */
export function useActiveWorkspace(): string[] {
  return useWorkspaceStore((s) => s.folders)
}

/** Convenience: the primary (first) folder, or null. */
export function useActiveWorkspaceRoot(): string | null {
  return useActiveWorkspace()[0] ?? null
}
