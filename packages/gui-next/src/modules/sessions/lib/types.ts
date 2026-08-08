/**
 * Mirrors `agent::commands::SessionInfo` exactly as the backend serializes
 * it — plain Rust field names, no `rename_all`, so this is genuinely
 * snake_case on the wire (confirmed by reading the struct, not assumed).
 */
export interface SessionInfo {
  id: string
  name: string
  created_at: string
  workspace_folders: string[]
  access_mode: string | null
}
