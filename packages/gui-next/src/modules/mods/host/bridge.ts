/**
 * The app's side of a mod page's `vivianne.*` calls (see `sdk/v1.js`).
 *
 * Rules enforced here, for every call:
 * - it comes from the frame it claims to (the caller checks `event.source`
 *   before handing the message over);
 * - the mod is still running and has the permission the method needs;
 * - at most `RATE_LIMIT` calls a second per frame;
 * - file paths stay inside the open project's folder.
 */
import { call as ipc } from '../../../ipc'
import type { AgentMessage } from '../../ai/lib/types'
import type { ModInfo, ModSlot } from '../lib/types'

/** Which permission each method needs (`null`: none). */
export const METHOD_PERMISSION: Record<string, string | null> = {
  'storage.get': null,
  'storage.set': null,
  notify: 'notify',
  'chat.read': 'chat.read',
  'chat.compose': 'chat.compose',
  'chat.send': 'chat.send',
  'agent.status': 'agent.status',
  'agent.subscribe': 'agent.status',
  'files.read': 'files.read',
  'files.list': 'files.read',
  'files.write': 'files.write',
  'open.url': 'open',
  'open.panel': 'open',
}

export const RATE_LIMIT = 100

/** What the bridge needs from the app; injected so it can be tested alone. */
export interface HostApi {
  storageGet(key: string): Promise<unknown>
  storageSet(key: string, value: unknown): Promise<void>
  notify(mod: ModInfo, text: string): void
  messages(): AgentMessage[]
  compose(text: string): void
  send(text: string): Promise<void>
  agentStatus(): Promise<unknown>
  subscribeStatus(push: (status: unknown) => void): () => void
  workspace(): string | null
  readFile(path: string): Promise<string>
  listDir(path: string): Promise<string[]>
  writeFile(path: string, content: string): Promise<void>
  openUrl(key: string, url: string): Promise<void>
  openPanel(name: string): void
}

/** Join a mod-given relative path onto the workspace, refusing anything
 * that would leave it. */
export function workspacePath(root: string, rel: string): string {
  const parts = rel.replace(/\\/g, '/').split('/').filter((p) => p && p !== '.')
  if (/^[a-zA-Z]:/.test(rel) || rel.startsWith('/') || rel.startsWith('\\') || parts.some((p) => p === '..')) {
    throw new Error('paths must be inside the workspace (relative, no ..)')
  }
  const sep = root.includes('\\') ? '\\' : '/'
  return parts.length ? `${root.replace(/[\\/]+$/, '')}${sep}${parts.join(sep)}` : root
}

function messageText(m: AgentMessage): string {
  if (!('content' in m) || !Array.isArray(m.content)) return ''
  return m.content
    .map((c) => (c.type === 'text' ? c.text : ''))
    .filter(Boolean)
    .join('\n')
}

/** One frame's connection: checks and runs its calls. */
export class ModConnection {
  private window = { start: 0, count: 0 }
  private unsubscribers: (() => void)[] = []

  readonly mod: ModInfo
  readonly slot: ModSlot
  private host: HostApi
  private pushEvent: (name: string, payload: unknown) => void

  constructor(mod: ModInfo, slot: ModSlot, host: HostApi, pushEvent: (name: string, payload: unknown) => void) {
    this.mod = mod
    this.slot = slot
    this.host = host
    this.pushEvent = pushEvent
  }

  /** Whether the mod has `permission`. */
  allowed(permission: string | null): boolean {
    return permission === null || (this.mod.manifest?.permissions ?? []).includes(permission)
  }

  async handle(method: string, args: Record<string, unknown> | null): Promise<unknown> {
    const now = Date.now()
    if (now - this.window.start >= 1000) this.window = { start: now, count: 0 }
    if (++this.window.count > RATE_LIMIT) throw new Error('too many calls; slow down')
    if (this.mod.status !== 'on') throw new Error('this mod is off')
    if (!(method in METHOD_PERMISSION)) throw new Error(`unknown method ${method}`)
    const permission = METHOD_PERMISSION[method]
    if (!this.allowed(permission)) {
      throw new Error(`${method} needs the "${permission}" permission in mod.json`)
    }
    const a = args ?? {}
    const str = (k: string) => {
      const v = a[k]
      if (typeof v !== 'string') throw new Error(`${k} must be a string`)
      return v
    }
    const root = () => {
      const r = this.host.workspace()
      if (!r) throw new Error('no project folder is open')
      return r
    }
    switch (method) {
      case 'storage.get':
        return this.host.storageGet(this.mod.key)
      case 'storage.set':
        return this.host.storageSet(this.mod.key, args)
      case 'notify':
        return this.host.notify(this.mod, str('text').slice(0, 300))
      case 'chat.read':
        return this.host
          .messages()
          .filter((m) => m.type === 'user' || m.type === 'assistant')
          .map((m) => ({ role: m.type, text: messageText(m) }))
      case 'chat.compose':
        return this.host.compose(str('text'))
      case 'chat.send':
        return this.host.send(str('text'))
      case 'agent.status':
        return this.host.agentStatus()
      case 'agent.subscribe':
        if (this.unsubscribers.length === 0) {
          this.unsubscribers.push(this.host.subscribeStatus((s) => this.pushEvent('agent.status', s)))
        }
        return null
      case 'files.read':
        return this.host.readFile(workspacePath(root(), str('path')))
      case 'files.list':
        return this.host.listDir(workspacePath(root(), str('path')))
      case 'files.write':
        return this.host.writeFile(workspacePath(root(), str('path')), str('content'))
      case 'open.url':
        return this.host.openUrl(this.mod.key, str('url'))
      case 'open.panel': {
        const id = str('id')
        if (!this.mod.manifest?.contributes.panels.some((p) => p.id === id)) throw new Error(`this mod has no panel "${id}"`)
        return this.host.openPanel(`${this.mod.key}/${id}`)
      }
    }
    throw new Error(`unknown method ${method}`)
  }

  dispose() {
    this.unsubscribers.forEach((u) => u())
    this.unsubscribers = []
  }
}

/** The real app behind the bridge. Imported lazily by `ModFrame` so tests
 * of the bridge don't pull in the stores. */
export async function appHost(): Promise<HostApi> {
  const [{ useAgentStore }, { useProjectsStore }, { usePanelStore }, { toast }, { on }] = await Promise.all([
    import('../../ai/store/agentStore'),
    import('../../sessions/store/projectsStore'),
    import('../../dock/store/panelStore'),
    import('../store/toastStore'),
    import('../../../ipc'),
  ])
  return {
    storageGet: (key) => ipc('cmd_mods_storage_get', { key }),
    storageSet: async (key, value) => {
      await ipc('cmd_mods_storage_set', { key, value })
    },
    notify: (mod, text) => toast(`${mod.manifest?.name ?? mod.key}: ${text}`),
    messages: () => useAgentStore.getState().messages,
    compose: (text) => useAgentStore.getState().setDraft(text),
    send: (text) => useAgentStore.getState().sendMessage(text),
    agentStatus: () => ipc('cmd_notch_status'),
    subscribeStatus: (push) => {
      const off = on<unknown>('notch:status', push)
      return () => void off.then((f) => f())
    },
    workspace: () => {
      const s = useProjectsStore.getState()
      return s.projects.find((p) => p.id === s.activeProjectId)?.path ?? null
    },
    readFile: async (path) => (await ipc<string>('cmd_read_file_raw', { path })) ?? '',
    listDir: async (path) => {
      const tree = await ipc<{ name: string }[] | { children?: { name: string }[] }>('cmd_list_dir_tree', {
        root: path,
        maxDepth: 1,
        maxEntries: 500,
      })
      const list = Array.isArray(tree) ? tree : (tree?.children ?? [])
      return list.map((n) => n.name)
    },
    writeFile: async (path, content) => {
      await ipc('cmd_write_text_file', { path, content })
    },
    openUrl: async (key, url) => {
      await ipc('cmd_mods_open_url', { key, url })
    },
    openPanel: (name) => usePanelStore.getState().openCustomPanel(name),
  }
}
