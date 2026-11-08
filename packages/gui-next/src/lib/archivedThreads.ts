/**
 * Threads the user has put away. An archived thread is kept as it is and hidden from the list until it is restored.
 *
 * Kept in the browser's storage, per device, because it is a view preference rather than part of the conversation.
 */
import { create } from 'zustand'

const KEY = 'viv.archivedThreads'

type Store = Pick<Storage, 'getItem' | 'setItem'>

export function loadArchived(storage: Store = localStorage): string[] {
  try {
    const parsed: unknown = JSON.parse(storage.getItem(KEY) ?? '[]')
    return Array.isArray(parsed) ? parsed.filter((x): x is string => typeof x === 'string') : []
  } catch {
    return []
  }
}

export function saveArchived(ids: readonly string[], storage: Store = localStorage): void {
  try {
    storage.setItem(KEY, JSON.stringify(ids))
  } catch {
    // Storage unavailable: archiving just lasts until the window closes.
  }
}

/** `ids` with `id` added or removed. */
export function toggled(ids: readonly string[], id: string, archived: boolean): string[] {
  const without = ids.filter((x) => x !== id)
  return archived ? [...without, id] : without
}

interface ArchivedState {
  ids: string[]
  set: (id: string, archived: boolean) => void
}

export const useArchivedThreads = create<ArchivedState>((set, get) => ({
  ids: typeof localStorage === 'undefined' ? [] : loadArchived(),
  set: (id, archived) => {
    const ids = toggled(get().ids, id, archived)
    saveArchived(ids)
    set({ ids })
  },
}))
