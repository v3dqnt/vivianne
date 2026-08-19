import type { StoreApi, UseBoundStore } from 'zustand'
import { createAgentStore } from './agentStore'

type AgentStore = UseBoundStore<StoreApi<ReturnType<typeof createAgentStore> extends UseBoundStore<StoreApi<infer S>> ? S : never>>

const registry = new Map<string, AgentStore>()

/** Get-or-create the `agentStore` instance for a secondary thread (a pane
 * opened via drag-to-dock, not the sidebar's primary/default chat). Reusing
 * the same instance across re-renders is what makes a pane's messages
 * survive being scrolled out of view instead of resetting on remount. */
export function getThreadStore(threadId: string): AgentStore {
  let store = registry.get(threadId)
  if (!store) {
    store = createAgentStore(threadId)
    registry.set(threadId, store)
  }
  return store
}

/** Drop a thread's store instance when its pane closes — the backend's
 * `AgentRegistry` entry (and any in-flight turn) is deliberately left alone;
 * this only frees the frontend-side subscription/state. */
export function disposeThreadStore(threadId: string): void {
  registry.delete(threadId)
}
