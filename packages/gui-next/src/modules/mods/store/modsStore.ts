import { create } from 'zustand'
import { call } from '../../../ipc'
import type { ModInfo } from '../lib/types'

interface ModsState {
  mods: ModInfo[]
  /** The project whose mods are in play (its folder), as last listed. */
  project: string | null
  /** When each mod's files last changed: a change reloads its open pages. */
  fingerprint: Record<string, number>
  loaded: boolean
  refresh: (project?: string | null) => Promise<void>
  setEnabled: (key: string, enabled: boolean) => Promise<void>
  trustProject: (project: string, trusted: boolean) => Promise<void>
  create: (files: { path: string; content: string }[]) => Promise<string>
  remove: (key: string) => Promise<void>
  pollChanges: () => Promise<void>
}

export const useModsStore = create<ModsState>((set, get) => ({
  mods: [],
  project: null,
  fingerprint: {},
  loaded: false,

  refresh: async (project) => {
    const p = project === undefined ? get().project : project
    const mods = (await call<ModInfo[]>('cmd_mods_list', { project: p })) ?? []
    set({ mods, project: p, loaded: true })
  },

  setEnabled: async (key, enabled) => {
    await call('cmd_mods_set_enabled', { key, enabled })
    await get().refresh()
  },

  trustProject: async (project, trusted) => {
    await call('cmd_mods_trust_project', { project, trusted })
    await get().refresh()
  },

  create: async (files) => {
    const key = (await call<string>('cmd_mods_create', { files })) ?? ''
    await get().refresh()
    return key
  },

  remove: async (key) => {
    await call('cmd_mods_remove', { key })
    await get().refresh()
  },

  pollChanges: async () => {
    const next = (await call<Record<string, number>>('cmd_mods_fingerprint')) ?? {}
    const prev = get().fingerprint
    const changed = Object.keys(next).some((k) => next[k] !== prev[k]) || Object.keys(prev).length !== Object.keys(next).length
    if (changed) {
      set({ fingerprint: next })
      // A manifest may have changed too.
      if (Object.keys(prev).length > 0) await get().refresh()
    }
  },
}))

/** Running mods only. */
export const runningMods = (mods: ModInfo[]) => mods.filter((m) => m.status === 'on' && m.manifest)

/** Every panel running mods add, as `<key>/<panel id>` with its title. */
export function modPanels(mods: ModInfo[]) {
  return runningMods(mods).flatMap((m) =>
    m.manifest!.contributes.panels.map((p) => ({ name: `${m.key}/${p.id}`, title: p.title, mod: m, panel: p })),
  )
}

/** A panel by its `<key>/<panel id>` name. */
export function findModPanel(mods: ModInfo[], name: string | null | undefined) {
  return name ? modPanels(mods).find((p) => p.name === name) : undefined
}

/** The running mod that renders a chat fence, if any. */
export function chatRenderer(mods: ModInfo[], fence: string) {
  for (const m of runningMods(mods)) {
    const c = m.manifest!.contributes.chat.find((c) => c.fence === fence)
    if (c) return { mod: m, entry: c.entry }
  }
  return undefined
}

/** Sidebar sections from running mods. */
export function sidebarSections(mods: ModInfo[]) {
  return runningMods(mods).flatMap((m) => m.manifest!.contributes.sidebar.map((s) => ({ mod: m, section: s })))
}

/** Quick access tiles from running mods. */
export function quickTiles(mods: ModInfo[]) {
  return runningMods(mods).flatMap((m) => (m.manifest!.contributes.quickAccess ?? []).map((tile) => ({ mod: m, tile })))
}

/** Mods waiting on the user: new, or asking for more than they were given. */
export const modsNeedingAttention = (mods: ModInfo[]) =>
  mods.filter((m) => m.status === 'needsApproval' || m.status === 'permissionsChanged')
