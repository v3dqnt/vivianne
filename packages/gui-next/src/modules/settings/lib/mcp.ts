/** Mirrors `mcp::McpTransportConfig` — `#[serde(tag = "kind", rename_all = "camelCase")]`. */
export type McpTransportConfig =
  | { kind: 'stdio'; command: string; args: string[]; env: Record<string, string> }
  | { kind: 'http'; url: string; headers: Record<string, string> }
  | { kind: 'native'; connector: string }

/** Mirrors `mcp::McpServerConfig` — `#[serde(rename_all = "camelCase")]`. */
export interface McpServerConfig {
  name: string
  transport: McpTransportConfig
  timeoutMs?: number | null
  /** Set for apps connected in Settings → Connections. */
  auth?: string | null
}

export const DEFAULT_MCP_TIMEOUT_MS = 30000

/** Mirrors `mcp::McpToolDef` — `#[serde(rename_all = "camelCase")]`. */
export interface McpToolDef {
  name: string
  description: string
  inputSchema: unknown
}
