/**
 * Keyboard shortcuts: one registry of actions, their default keys, and the
 * user's own bindings (Settings → Layout & shortcuts). One keydown listener
 * runs them all (`installShortcuts`), so every shortcut is listed, rebindable
 * and checked for clashes in one place.
 *
 * Keys are written `Mod+Shift+K`: `Mod` is ⌘ on macOS and Ctrl elsewhere.
 */
import { create } from 'zustand'
import { loadPersisted, savePersisted } from './persist'
import { isMac } from './platform'

export interface ShortcutAction {
  id: string
  label: string
  group: 'App' | 'Panels' | 'Chat' | 'Projects' | 'Layouts'
  /** `null`: no default; the user can bind one. */
  defaultKeys: string | null
  run: () => void | Promise<void>
}

const STORAGE_KEY = 'viv.shortcuts'

interface ShortcutState {
  /** Action id → keys, or `''` for "unbound". Missing: the default. */
  overrides: Record<string, string>
  set: (id: string, keys: string) => void
  reset: (id: string) => void
  resetAll: () => void
}

export const useShortcutStore = create<ShortcutState>((set, get) => ({
  overrides: loadPersisted<Record<string, string>>(STORAGE_KEY, {}),
  set: (id, keys) => {
    const overrides = { ...get().overrides, [id]: keys }
    savePersisted(STORAGE_KEY, overrides)
    set({ overrides })
  },
  reset: (id) => {
    const overrides = { ...get().overrides }
    delete overrides[id]
    savePersisted(STORAGE_KEY, overrides)
    set({ overrides })
  },
  resetAll: () => {
    savePersisted(STORAGE_KEY, {})
    set({ overrides: {} })
  },
}))

const MODIFIER_KEYS = new Set(['Control', 'Shift', 'Alt', 'Meta', 'AltGraph', 'OS'])

/** The key part of a combo, normalised (`k` → `K`, ` ` → `Space`). */
function keyName(e: Pick<KeyboardEvent, 'key' | 'code'>): string {
  if (e.code === 'Space') return 'Space'
  if (/^Digit\d$/.test(e.code)) return e.code.slice(5)
  if (/^Key[A-Z]$/.test(e.code)) return e.code.slice(3)
  if (e.key.length === 1) return e.key.toUpperCase()
  return e.key
}

/** The combo a key event is, as `Mod+Shift+K` (`null` for a bare modifier). */
export function comboFromEvent(e: Pick<KeyboardEvent, 'key' | 'code' | 'ctrlKey' | 'metaKey' | 'shiftKey' | 'altKey'>): string | null {
  if (MODIFIER_KEYS.has(e.key)) return null
  const parts: string[] = []
  const mod = isMac ? e.metaKey : e.ctrlKey
  if (mod) parts.push('Mod')
  if (isMac && e.ctrlKey) parts.push('Ctrl')
  if (!isMac && e.metaKey) parts.push('Meta')
  if (e.altKey) parts.push('Alt')
  if (e.shiftKey) parts.push('Shift')
  parts.push(keyName(e))
  return parts.join('+')
}

/** How a combo is shown on this platform. */
export function formatCombo(keys: string): string {
  return keys
    .split('+')
    .map((p) => {
      if (p === 'Mod') return isMac ? '⌘' : 'Ctrl'
      if (isMac && p === 'Shift') return '⇧'
      if (isMac && p === 'Alt') return '⌥'
      if (isMac && p === 'Ctrl') return '⌃'
      return p
    })
    .join(isMac ? '' : '+')
}

/** Whether a combo is safe to bind: it needs a modifier, unless it's a
 * function key (a bare letter would fire while typing). */
export function isBindable(keys: string): boolean {
  const parts = keys.split('+')
  const key = parts[parts.length - 1]
  return parts.length > 1 || /^F\d{1,2}$/.test(key)
}

let registry: ShortcutAction[] = []

/** Give the dispatcher its actions (called once, from the app). */
export function registerShortcuts(actions: ShortcutAction[]) {
  registry = actions
}

export function shortcutActions(): ShortcutAction[] {
  return registry
}

/** The keys an action runs on now (`null`: unbound). */
export function keysFor(id: string, overrides = useShortcutStore.getState().overrides): string | null {
  if (id in overrides) return overrides[id] || null
  return registry.find((a) => a.id === id)?.defaultKeys ?? null
}

/** Actions other than `id` already on `keys`. */
export function clashes(id: string, keys: string): ShortcutAction[] {
  return registry.filter((a) => a.id !== id && keysFor(a.id) === keys)
}

/** Set while a shortcut field is recording, so presses aren't also run. */
let recording = false
export const setRecording = (on: boolean) => {
  recording = on
}

/** The one keydown listener. Returns its remover. */
export function installShortcuts(): () => void {
  const onKey = (e: KeyboardEvent) => {
    if (recording || e.repeat) return
    const combo = comboFromEvent(e)
    if (!combo) return
    // A terminal or code editor needs its plain Ctrl/⌘ keys (Ctrl+L clears a
    // terminal, ⌘L selects a line): there, only chords with Shift or Alt
    // reach the app.
    const target = e.target as Element | null
    if (target?.closest?.('.xterm, .cm-editor') && !/\b(Shift|Alt)\b/.test(combo)) return
    const overrides = useShortcutStore.getState().overrides
    const action = registry.find((a) => keysFor(a.id, overrides) === combo)
    if (!action) return
    e.preventDefault()
    e.stopPropagation()
    void action.run()
  }
  window.addEventListener('keydown', onKey, true)
  return () => window.removeEventListener('keydown', onKey, true)
}
