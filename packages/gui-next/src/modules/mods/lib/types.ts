/** Mirrors `mods::manifest::Manifest` (camelCase). */
export interface ModManifest {
  apiVersion: number
  id: string
  name: string
  version: string
  description: string
  author?: string | null
  permissions: string[]
  contributes: {
    panels: { id: string; title: string; icon?: string | null; entry?: string | null; url?: string | null }[]
    sidebar: { id: string; title: string; entry: string; height?: number | null }[]
    chat: { fence: string; entry: string }[]
    tools: { name: string; description: string; parameters: Record<string, unknown>; command: string; timeoutSecs?: number | null }[]
    themes: { file: string }[]
    quickAccess?: QuickAccessTile[]
  }
}

/** Mirrors `mods::manifest::QuickAccessContribution`: a tile in the Quick access panel. */
export interface QuickAccessTile {
  id: string
  title: string
  icon?: string | null
  description?: string | null
  action:
    | { type: 'panel'; panel: string }
    | { type: 'open'; target: string }
    | { type: 'compose'; text: string }
    | { type: 'send'; text: string }
}

/** Mirrors `mods::Status`. */
export type ModStatus = 'on' | 'off' | 'needsApproval' | 'permissionsChanged' | 'untrusted' | 'invalid'

/** Mirrors `mods::ModInfo`. */
export interface ModInfo {
  key: string
  scope: 'user' | 'project'
  dir: string
  status: ModStatus
  manifest: ModManifest | null
  error: string | null
  /** [permission, what it means] */
  permissions: [string, string][]
}

/** Where a mod page is shown. */
export type ModSlot = 'panel' | 'sidebar' | 'chat'
