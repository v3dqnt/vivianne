import { create } from 'zustand'
import { call } from '../../../ipc'
import type { McpServerConfig, McpToolDef } from '../lib/mcp'

interface McpStoreState {
  servers: McpServerConfig[]
  /** `cmd_mcp_list_tools` returns `Vec<(String, McpToolDef)>` — server name paired with each tool. */
  tools: [string, McpToolDef][]
  loading: boolean
  error: string | null
  refresh: () => Promise<void>
  addServer: (config: McpServerConfig) => Promise<void>
  removeServer: (name: string) => Promise<void>
  authorize: (serverName: string, resourceUrl: string, clientId: string) => Promise<boolean>
}

export const useMcpStore = create<McpStoreState>((set, get) => ({
  servers: [],
  tools: [],
  loading: false,
  error: null,

  refresh: async () => {
    set({ loading: true, error: null })
    try {
      const [servers, tools] = await Promise.all([
        call<McpServerConfig[]>('cmd_mcp_list_servers'),
        call<[string, McpToolDef][]>('cmd_mcp_list_tools'),
      ])
      set({ servers: servers ?? [], tools: tools ?? [] })
    } finally {
      set({ loading: false })
    }
  },

  addServer: async (config) => {
    set({ error: null })
    try {
      await call('cmd_mcp_add_server', { config })
      await get().refresh()
    } catch (err) {
      set({ error: err instanceof Error ? err.message : String(err) })
      throw err
    }
  },

  removeServer: async (name) => {
    await call('cmd_mcp_remove_server', { name })
    set((s) => ({ servers: s.servers.filter((sv) => sv.name !== name), tools: s.tools.filter(([n]) => n !== name) }))
  },

  authorize: async (serverName, resourceUrl, clientId) => {
    const ok = await call<boolean>('cmd_mcp_authorize', { serverName, resourceUrl, clientId })
    return !!ok
  },
}))
