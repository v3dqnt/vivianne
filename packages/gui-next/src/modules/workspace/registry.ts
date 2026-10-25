import {
  MessageSquareLinear,
  DocumentTextLinear,
  MonitorLinear,
  TaskLinear,
  GithubLinear,
  Link2Linear,
  CommandSquareLinear,
  LayerLinear,
  CodeLinear,
  AgentsLinear,
  ClipboardTextLinear,
  ClockLinear,
  BrainLinear,
} from '../../icons'
import type { DockViewKind } from '../dock/store/panelStore'

/** Every kind of panel the workspace can host. The chat is one of them now —
 * it docks, tabs and resizes like the rest. */
export type PanelKind = 'chat' | 'stack' | 'quick' | DockViewKind

export interface PanelDef {
  title: string
  icon: typeof DocumentTextLinear
  /**
   * `always` keeps the panel mounted while it's a hidden tab, and while it is
   * dragged to another group. Required for anything holding state that dies
   * with its component:
   *
   * - **terminal** — each mount spawns a shell and each unmount kills it, so
   *   the default renderer would silently restart your shell every time you
   *   switched tabs or moved the panel.
   * - **preview** — a native webview laid over the page, which must not be
   *   torn down and recreated on a tab switch.
   * - **chat** and **ide** — a streaming reply, scroll position, and the
   *   editor's undo history all live in the component.
   *
   * Everything else re-renders cheaply from its store, and staying unmounted
   * while hidden is what keeps a crowded layout light.
   */
  renderer: 'always' | 'onlyWhenVisible'
  /** One per workspace; opening it again focuses the existing one. */
  singleton: boolean
  closable: boolean
}

export const PANELS: Record<PanelKind, PanelDef> = {
  chat: { title: 'Chat', icon: MessageSquareLinear, renderer: 'always', singleton: true, closable: false },
  files: { title: 'All files', icon: DocumentTextLinear, renderer: 'onlyWhenVisible', singleton: true, closable: true },
  ide: { title: 'Editor', icon: CodeLinear, renderer: 'always', singleton: true, closable: true },
  preview: { title: 'Preview', icon: MonitorLinear, renderer: 'always', singleton: true, closable: true },
  review: { title: 'Git review', icon: GithubLinear, renderer: 'onlyWhenVisible', singleton: true, closable: true },
  documents: { title: 'Documents', icon: DocumentTextLinear, renderer: 'onlyWhenVisible', singleton: true, closable: true },
  tasks: { title: 'Background tasks', icon: TaskLinear, renderer: 'onlyWhenVisible', singleton: true, closable: true },
  multiplayer: { title: 'Multiplayer', icon: Link2Linear, renderer: 'onlyWhenVisible', singleton: true, closable: true },
  // One per open file; `panelName` is the file's path.
  document: { title: 'Document', icon: DocumentTextLinear, renderer: 'onlyWhenVisible', singleton: true, closable: true },
  agents: { title: 'Agents', icon: AgentsLinear, renderer: 'onlyWhenVisible', singleton: true, closable: true },
  plan: { title: 'Plan', icon: ClipboardTextLinear, renderer: 'onlyWhenVisible', singleton: true, closable: true },
  timeline: { title: 'Timeline', icon: ClockLinear, renderer: 'onlyWhenVisible', singleton: true, closable: true },
  memory: { title: 'Memory', icon: BrainLinear, renderer: 'onlyWhenVisible', singleton: true, closable: true },
  terminal: { title: 'Terminal', icon: CommandSquareLinear, renderer: 'always', singleton: false, closable: true },
  custom: { title: 'Panel', icon: LayerLinear, renderer: 'onlyWhenVisible', singleton: true, closable: true },
  // One-press shortcuts, with tiles mods can add (see `QuickAccess.tsx`).
  quick: { title: 'Quick access', icon: LayerLinear, renderer: 'onlyWhenVisible', singleton: true, closable: true },
  // Several panels shown one above another in a single slot (see `Stack.tsx`).
  stack: { title: 'Panels', icon: LayerLinear, renderer: 'onlyWhenVisible', singleton: true, closable: true },
}

/** The panels offered in "add panel" menus, in menu order. Chat is left out:
 * it always exists and can't be closed, so there is never a second one to add.
 * The editor is left out because it was removed from the app; its component
 * stays registered only so an older saved layout can still be restored. */
export const ADDABLE: Exclude<PanelKind, 'chat' | 'stack' | 'custom' | 'ide' | 'document' | 'documents'>[] = [
  'quick',
  'files',
  'agents',
  'plan',
  'timeline',
  'memory',
  'preview',
  'terminal',
  'review',
  'tasks',
  'multiplayer',
]
