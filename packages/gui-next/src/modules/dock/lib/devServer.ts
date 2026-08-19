/** Mirrors `agent::dev_server_manager::DevServerStatus` —
 * `#[serde(tag = "kind", rename_all = "camelCase")]`. */
export type DevServerStatus =
  | { kind: 'stopped' }
  | { kind: 'starting' }
  | { kind: 'running'; url: string }
  | { kind: 'crashed'; error: string }

/** Mirrors `agent::dev_server_manager::DevServerConfig`. */
export interface DevServerConfig {
  id: string
  name: string
  cwd: string
  command: string
}

export const DEFAULT_DEV_COMMAND = 'npm run dev'

/** Deterministic per-workspace id — one dev server config per root folder,
 * so switching workspaces naturally shows that workspace's own (or not yet
 * configured) preview instead of carrying another project's server over. */
export function devServerId(workspaceRoot: string): string {
  return `devserver:${workspaceRoot}`
}
