/**
 * Right-click menus, contributed by whoever knows about the thing clicked.
 *
 * A provider is registered for a kind of target (`file`, `thread`) and returns the items it wants for one particular target. Providers may be
 * asynchronous (listing installed apps takes a moment); the menu waits a bounded time for each, drops one that fails or is late, and keeps
 * the rest, so a slow or broken contributor never blocks the menu.
 */

export interface MenuItem {
  /** Stable within one menu; a later item with the same id is dropped. */
  id: string
  label: string
  run: () => void | Promise<void>
  danger?: boolean
  /** Items with the same group sit together, groups separated by a rule, in the order they first appear. */
  group?: string
}

export type MenuProvider<T> = (target: T) => MenuItem[] | Promise<MenuItem[]>

const registry = new Map<string, MenuProvider<never>[]>()

/** Contribute to the menu for `kind`. Returns a function that removes the contribution. */
export function registerMenu<T>(kind: string, provider: MenuProvider<T>): () => void {
  const list = registry.get(kind) ?? []
  list.push(provider as MenuProvider<never>)
  registry.set(kind, list)
  return () => registry.set(kind, (registry.get(kind) ?? []).filter((p) => p !== provider))
}

const PROVIDER_TIMEOUT_MS = 1000

/** Every provider's items for `target`, in registration order, without duplicates by id. */
export async function resolveMenu<T>(kind: string, target: T, timeoutMs: number = PROVIDER_TIMEOUT_MS): Promise<MenuItem[]> {
  const providers = (registry.get(kind) ?? []) as MenuProvider<T>[]
  const settled = await Promise.all(
    providers.map(async (p) => {
      try {
        return await Promise.race([Promise.resolve(p(target)), new Promise<MenuItem[]>((resolve) => setTimeout(() => resolve([]), timeoutMs))])
      } catch {
        return []
      }
    }),
  )
  const seen = new Set<string>()
  return settled.flat().filter((item) => (seen.has(item.id) ? false : (seen.add(item.id), true)))
}

/** The items cut into groups for display, in the order each group first appears. */
export function groupItems(items: readonly MenuItem[]): MenuItem[][] {
  const order: string[] = []
  const groups = new Map<string, MenuItem[]>()
  for (const item of items) {
    const g = item.group ?? ''
    if (!groups.has(g)) {
      groups.set(g, [])
      order.push(g)
    }
    groups.get(g)!.push(item)
  }
  return order.map((g) => groups.get(g)!)
}
