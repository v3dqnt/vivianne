import { MessageSquareLinear, DocumentTextLinear, MonitorLinear, CommandSquareLinear } from '../../icons'

/** Every kind of panel the workspace can host. The chat is one of them. */
export type PanelKind = 'chat' | 'files' | 'browser' | 'terminal'

export interface PanelDef {
  title: string
  icon: typeof DocumentTextLinear
  /**
   * `always` keeps the panel mounted while it is a hidden tab. Required for
   * anything holding state that dies with its component:
   *
   * - **terminal** — each mount spawns a shell and each unmount kills it.
   * - **browser** — the child webviews are OS-level views; the component is
   *   what parks them off-screen, so it must stay mounted to keep them hidden.
   * - **chat** — a streaming reply and scroll position live in the component.
   */
  renderer: 'always' | 'onlyWhenVisible'
  /** One per workspace; opening it again focuses the existing one. */
  singleton: boolean
  closable: boolean
}

export const PANELS: Record<PanelKind, PanelDef> = {
  chat: { title: 'Chat', icon: MessageSquareLinear, renderer: 'onlyWhenVisible', singleton: true, closable: false },
  files: { title: 'Files', icon: DocumentTextLinear, renderer: 'onlyWhenVisible', singleton: true, closable: true },
  browser: { title: 'Browser', icon: MonitorLinear, renderer: 'always', singleton: true, closable: true },
  terminal: { title: 'Terminal', icon: CommandSquareLinear, renderer: 'always', singleton: true, closable: true },
}

/** Panel ids a saved layout may still name. They are closed on load so an old
 * arrangement comes back as the three-tab container instead of failing. */
export const RETIRED: ReadonlySet<string> = new Set([
  'ide', 'preview', 'review', 'documents', 'document', 'tasks', 'memory',
  'agents', 'plan', 'timeline', 'multiplayer', 'quick', 'stack', 'custom',
])

/** The panels the Panels menu and the empty state offer, in menu order. */
export const ADDABLE: PanelKind[] = ['files', 'browser', 'terminal']
