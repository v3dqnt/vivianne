/**
 * The workspace layout: the chat in the centre, a right panel and a bottom
 * panel around it, and every panel — the chat included — movable between them
 * by dragging its tab.
 *
 * ## Zones
 *
 * The right and bottom panels are dockview *edge groups*: they belong to the
 * window's edges rather than the grid, they can be shown and hidden as a unit
 * from the title bar, and — unlike ordinary groups, which dockview deletes the
 * moment their last tab leaves — they persist when empty. An empty zone shows
 * a chooser of utilities to open in it, alongside the "+" in its header.
 *
 * Everything else in the centre is an ordinary grid: drag a tab onto the
 * chat's edge to split beside it, onto another group to join it.
 *
 * ## Who calls this
 *
 * The UI (title bar, panel headers, empty states, settings) and the agent's
 * `set_panel` tool via `panelStore`. Both ask for a panel by name — "open the
 * preview" — and placement is decided here.
 */
import { create } from 'zustand'
import type { DockviewApi, DockviewGroupPanel, IDockviewPanel } from 'dockview-react'
import { loadPersisted, savePersisted } from '../../lib/persist'
import { useModsStore, findModPanel } from '../mods/store/modsStore'
import { PANELS, type PanelKind } from './registry'

export type Zone = 'right' | 'bottom'
export type Preset = 'default' | 'full' | 'focus' | 'coding' | 'research'

export const ZONE_LABELS: Record<Zone, string> = { right: 'Right panel', bottom: 'Bottom panel' }

export const PRESET_LABELS: Record<Preset, string> = {
  default: 'Chat with right panel',
  full: 'Chat with right panel and quick access',
  focus: 'Chat only',
  coding: 'Coding: chat, files and a terminal',
  research: 'Research: chat and a browser',
}

/** A layout the user saved by name (Settings → Layout & shortcuts). */
export interface SavedLayout {
  id: string
  name: string
  layout: unknown
}

const SAVED_KEY = 'viv.layouts.saved'
/** What opens at launch: `last` (where you left off), a preset, or
 * `saved:<id>`. */
const STARTUP_KEY = 'viv.layouts.startup'

export const useSavedLayouts = create<{ saved: SavedLayout[]; startup: string }>(() => ({
  saved: loadPersisted<SavedLayout[]>(SAVED_KEY, []),
  startup: loadPersisted<string>(STARTUP_KEY, 'last'),
}))

function storeSaved(saved: SavedLayout[]) {
  savePersisted(SAVED_KEY, saved)
  useSavedLayouts.setState({ saved })
}

/** Save the arrangement on screen under a name (replacing one of that name). */
export function saveCurrentLayout(name: string): SavedLayout | null {
  if (!api || !name.trim()) return null
  const entry: SavedLayout = { id: crypto.randomUUID(), name: name.trim(), layout: snapshot() }
  const rest = useSavedLayouts.getState().saved.filter((l) => l.name !== entry.name)
  storeSaved([...rest, entry])
  return entry
}

export function deleteSavedLayout(id: string) {
  storeSaved(useSavedLayouts.getState().saved.filter((l) => l.id !== id))
  if (useSavedLayouts.getState().startup === `saved:${id}`) setStartupLayout('last')
}

export function renameSavedLayout(id: string, name: string) {
  if (!name.trim()) return
  storeSaved(useSavedLayouts.getState().saved.map((l) => (l.id === id ? { ...l, name: name.trim() } : l)))
}

export function setStartupLayout(startup: string) {
  savePersisted(STARTUP_KEY, startup)
  useSavedLayouts.setState({ startup })
}

/** Restore a saved layout (a panel it names that no longer exists is
 * dropped by dockview; the chat always comes back). */
export function applySavedLayout(id: string): boolean {
  const entry = useSavedLayouts.getState().saved.find((l) => l.id === id)
  if (!api || !entry) return false
  try {
    api.fromJSON(withZoneMinimums(structuredClone(entry.layout)) as Parameters<DockviewApi['fromJSON']>[0])
  } catch (e) {
    console.warn('[workspace] saved layout could not be applied:', e)
    return false
  }
  ensureZones()
  if (!find('chat')) api.addPanel(panelOptions('chat', undefined, 'chat'))
  keepEmptyZonesOpen()
  syncState()
  scheduleSave()
  return true
}

const ZONES: Zone[] = ['right', 'bottom']
const ZONE_IDS: Record<Zone, string> = { right: 'zone-right', bottom: 'zone-bottom' }
/** How big each zone starts, and the smallest it can be dragged to. Below
 * these a panel stops being usable: the right panel's file tree and preview
 * need room for a path and a readable page, the bottom panel for a few lines
 * of terminal output under its tabs. */
const ZONE_SIZE: Record<Zone, { initialSize: number; minimumSize: number }> = {
  right: { initialSize: 400, minimumSize: 320 },
  bottom: { initialSize: 260, minimumSize: 160 },
}

/** The least room the chat keeps, however the zones are sized or the window
 * shrinks. Below it the chat stops being a place to type and read. */
const CHAT_MIN = { right: 440, bottom: 240 } as const
/** A hidden zone comes back only once it has this much room to spare, so a
 * window resized across the line doesn't flicker it in and out. */
const REVEAL_SLACK = 40

/** Zones hidden because the window got too small for them (not by the user),
 * to be brought back when it grows. */
const autoHidden: Record<Zone, boolean> = { right: false, bottom: false }
/** Zones the user (or the agent) just asked for: kept even when the chat is
 * tight, rather than snatched away again by the fit below. */
const requested: Record<Zone, boolean> = { right: false, bottom: false }

/** How big a zone opens for a window of this size: a share of it, within
 * the zone's own bounds. A fixed 400px was a third of a laptop screen and
 * most of a small window. */
function openingSize(zone: Zone): number {
  const total = zone === 'right' ? api?.width : api?.height
  const { initialSize, minimumSize } = ZONE_SIZE[zone]
  if (!total) return initialSize
  const share = zone === 'right' ? 0.34 : 0.32
  return Math.round(Math.min(Math.max(total * share, minimumSize), initialSize * 1.2))
}

/** Serialized dockview layout. Bumped from v1 when the zones arrived: a v1
 * layout has no edge groups, so it's rebuilt rather than restored. */
const STORAGE_KEY = 'viv.workspace.v2'

/** One panel inside a stack. */
export interface StackItem {
  kind: PanelKind
  panelName?: string
}

interface PanelParams {
  kind: PanelKind
  panelName?: string
  /** For a `stack` panel: what it holds, top to bottom, and the relative
   * height of each. */
  items?: StackItem[]
  sizes?: number[]
}

interface WorkspaceState {
  /** Whether the right panel is showing its panels stacked rather than as tabs. */
  stacked: boolean
  /** One entry per open panel: `kind`, or `custom:<mod key>/<panel id>` for mod panels. */
  open: string[]
  zones: Record<Zone, boolean>
  ready: boolean
}

export const useWorkspace = create<WorkspaceState>(() => ({
  stacked: false,
  open: [],
  zones: { right: false, bottom: false },
  ready: false,
}))

let api: DockviewApi | null = null
let saveTimer: number | undefined

/** Kinds with one panel per `panelName` rather than one per kind: custom
 * panels by name, documents by file path. */
function isKeyed(kind: PanelKind): boolean {
  return kind === 'custom' || kind === 'document'
}

function kindKey(kind: PanelKind, panelName?: string): string {
  return isKeyed(kind) ? `${kind}:${panelName ?? ''}` : kind
}

export function itemKey(item: StackItem): string {
  return kindKey(item.kind, item.panelName)
}

/** The name a panel goes by in a tab or a card header. */
export function titleFor(kind: PanelKind, panelName?: string): string {
  if (kind === 'custom') return findModPanel(useModsStore.getState().mods, panelName)?.title ?? panelName ?? 'Panel'
  if (kind === 'document') return panelName?.split(/[\\/]/).filter(Boolean).pop() ?? 'Document'
  return PANELS[kind].title
}

function paramsOf(panel: IDockviewPanel): PanelParams | undefined {
  return panel.params as PanelParams | undefined
}

export function zoneOf(group: { id: string } | undefined): Zone | null {
  if (!group) return null
  return group.id === ZONE_IDS.right ? 'right' : group.id === ZONE_IDS.bottom ? 'bottom' : null
}

function zoneGroup(zone: Zone): DockviewGroupPanel | undefined {
  return api?.groups.find((g) => g.id === ZONE_IDS[zone])
}

/** The chat's own floor, so no drag of a sash can take it below the least
 * it needs: set on whichever group holds it (it can be moved), and cleared
 * from any group it has left. */
const pinned = new WeakMap<object, boolean>()
function pinChatMinimum() {
  if (!api) return
  const holder = find('chat')?.group
  for (const g of api.groups) {
    if (zoneOf(g)) continue
    const isChat = g === holder
    // Only on a change: setting a constraint raises a layout event, and a
    // layout event runs this again.
    if (pinned.get(g) === isChat) continue
    pinned.set(g, isChat)
    try {
      g.api.setConstraints(isChat ? { minimumWidth: CHAT_MIN.right, minimumHeight: CHAT_MIN.bottom } : { minimumWidth: 0, minimumHeight: 0 })
    } catch (e) {
      console.warn('[workspace] could not set the chat minimum:', e)
    }
  }
}

function syncState() {
  if (!api) return
  pinChatMinimum()
  const open = api.panels.flatMap((p) => {
    const params = paramsOf(p)
    if (params?.kind === 'stack') return (params.items ?? []).map(itemKey)
    return params ? [kindKey(params.kind, params.panelName)] : [p.id]
  })
  // A group holding only a stack has no tabs worth showing: each card in it
  // has its own header.
  for (const g of api.groups) g.header.hidden = g.panels.length === 1 && paramsOf(g.panels[0])?.kind === 'stack'
  useWorkspace.setState({
    stacked: !!stackPanel(),
    open,
    zones: {
      right: api.isEdgeGroupVisible('right'),
      bottom: api.isEdgeGroupVisible('bottom'),
    },
  })
}

/** The layout as it should be remembered: a zone that stepped aside only
 * because the window was small is kept as showing, so it is there again when
 * the window is big enough (a choice the user made is saved as made). */
function snapshot(): ReturnType<DockviewApi['toJSON']> {
  const json = api!.toJSON()
  const edges = (json as { edgeGroups?: Record<string, { visible?: boolean }> }).edgeGroups
  if (edges) for (const zone of ZONES) if (autoHidden[zone] && edges[zone]) edges[zone].visible = true
  return json
}

function scheduleSave() {
  if (!api) return
  window.clearTimeout(saveTimer)
  // Debounced: a drag fires a layout change per frame.
  saveTimer = window.setTimeout(() => {
    if (api) savePersisted(STORAGE_KEY, snapshot())
  }, 400)
}

/** Create any zone that doesn't exist yet. */
function ensureZones() {
  if (!api) return
  for (const zone of ZONES) {
    if (!api.getEdgeGroup(zone)) api.addEdgeGroup(zone, { id: ZONE_IDS[zone], ...ZONE_SIZE[zone], initialSize: openingSize(zone) })
    // An edge group at the bottom puts its tabs along the bottom edge by
    // default. Tabs sit at the top everywhere else in the app.
    zoneGroup(zone)?.api.setHeaderPosition('top')
  }
}

/**
 * A zone emptied of its last tab collapses to a thin strip — dockview's
 * default for edge groups. That strip would hide the empty state, whose whole
 * point is to be there when the zone is empty, so an emptied zone that's meant
 * to be showing is opened back up.
 */
function keepEmptyZonesOpen() {
  if (!api) return
  for (const zone of ZONES) {
    const edge = api.getEdgeGroup(zone)
    const group = zoneGroup(zone)
    if (edge && group && group.panels.length === 0 && api.isEdgeGroupVisible(zone) && edge.isCollapsed()) edge.expand()
  }
}

function showZone(zone: Zone) {
  if (!api) return
  requested[zone] = true
  autoHidden[zone] = false
  api.setEdgeGroupVisible(zone, true)
  const edge = api.getEdgeGroup(zone)
  if (edge?.isCollapsed()) edge.expand()
}

export function setZoneVisible(zone: Zone, visible: boolean) {
  if (!api) return
  if (visible) showZone(zone)
  else {
    requested[zone] = false
    autoHidden[zone] = false
    api.setEdgeGroupVisible(zone, false)
  }
  syncState()
  scheduleSave()
}

export function toggleZone(zone: Zone) {
  if (!api) return
  setZoneVisible(zone, !api.isEdgeGroupVisible(zone))
}

// A concrete shape rather than `AddPanelOptions`: that type is a union over
// the ways to position a panel, and spreading a union loses which member it
// is, so `{ ...panelOptions(), position }` wouldn't type-check.
function panelOptions(
  kind: PanelKind,
  panelName: string | undefined,
  id: string,
): { id: string; component: string; title: string; params: PanelParams; renderer: 'always' | 'onlyWhenVisible' } {
  const def = PANELS[kind]
  return {
    id,
    component: kind,
    title: titleFor(kind, panelName),
    params: { kind, panelName },
    renderer: def.renderer,
  }
}

function find(kind: PanelKind, panelName?: string): IDockviewPanel | undefined {
  return api?.panels.find((p) => {
    const params = paramsOf(p)
    if (params?.kind === 'stack') return (params.items ?? []).some((i) => i.kind === kind && (!isKeyed(kind) || i.panelName === panelName))
    return params?.kind === kind && (!isKeyed(kind) || params.panelName === panelName)
  })
}

function stackPanel(): IDockviewPanel | undefined {
  return api?.panels.find((p) => paramsOf(p)?.kind === 'stack')
}

/** Replace a stack's contents (and heights). An empty stack is closed. */
export function saveStack(items: StackItem[], sizes?: number[]) {
  const stack = stackPanel()
  if (!stack) return
  if (!items.length) stack.api.close()
  else stack.api.updateParameters({ items, sizes })
  syncState()
  scheduleSave()
}

/** Show the right panel's tabs one above another instead. */
export function stackZone() {
  if (!api) return
  const group = zoneGroup('right')
  if (!group || stackPanel()) return
  const tabs = group.panels.filter((p) => paramsOf(p)?.kind !== 'stack')
  if (tabs.length < 2) return
  const items = tabs.flatMap((p) => {
    const params = paramsOf(p)
    return params ? [{ kind: params.kind, panelName: params.panelName }] : []
  })
  for (const p of tabs) p.api.close()
  api.addPanel({
    id: 'stack',
    component: 'stack',
    title: PANELS.stack.title,
    params: { kind: 'stack', items },
    renderer: PANELS.stack.renderer,
    position: { referenceGroup: group },
  })
  syncState()
  scheduleSave()
}

/** Put a stack's panels back as tabs. */
export function unstackZone() {
  const stack = stackPanel()
  const group = zoneGroup('right')
  if (!api || !stack || !group) return
  const items = paramsOf(stack)?.items ?? []
  stack.api.close()
  for (const item of items) {
    api.addPanel({
      ...panelOptions(item.kind, item.panelName, newId(item.kind, item.panelName)),
      position: { referenceGroup: group },
    })
  }
  syncState()
  scheduleSave()
}

function newId(kind: PanelKind, panelName?: string): string {
  if (isKeyed(kind)) return `${kind}:${panelName ?? 'panel'}`
  return PANELS[kind].singleton ? kind : `${kind}:${crypto.randomUUID().slice(0, 8)}`
}

/**
 * Open a panel, or focus it if it's already open.
 *
 * Where a new one goes, unless the caller says: terminals to the bottom
 * panel, everything else to the right panel — revealing that panel if it was
 * hidden, since opening something into a hidden panel would look like nothing
 * happened.
 *
 * Everything but terminals is single-instance: asking for the file tree when
 * one is open brings that one forward — or, when asked for it in a specific
 * group, moves it there.
 */
export function openPanel(
  kind: PanelKind,
  opts: { panelName?: string; group?: DockviewGroupPanel; newInstance?: boolean } = {},
) {
  if (!api) return
  const def = PANELS[kind]
  // While the right panel is stacked, what is opened there joins the stack.
  const stack = stackPanel()
  if (stack && kind !== 'terminal' && kind !== 'chat' && !opts.group && !opts.newInstance) {
    const params = paramsOf(stack)
    const items = params?.items ?? []
    if (!find(kind, opts.panelName)) {
      saveStack([...items, { kind, panelName: opts.panelName }], [...(params?.sizes ?? items.map(() => 1)), 1])
    }
    showZone('right')
    stack.api.setActive()
    return
  }
  if (!opts.newInstance && def.singleton) {
    const existing = find(kind, opts.panelName)
    if (existing) {
      if (opts.group && existing.group.id !== opts.group.id) existing.api.moveTo({ group: opts.group })
      const zone = zoneOf(existing.group)
      if (zone) showZone(zone)
      existing.api.setActive()
      syncState()
      return
    }
  }

  let target = opts.group
  if (!target && kind !== 'chat') {
    const zone: Zone = kind === 'terminal' || kind === 'quick' ? 'bottom' : 'right'
    showZone(zone)
    target = zoneGroup(zone)
  }
  api.addPanel({
    ...panelOptions(kind, opts.panelName, newId(kind, opts.panelName)),
    ...(target ? { position: { referenceGroup: target } } : {}),
  })
  const zone = zoneOf(target)
  if (zone) showZone(zone)
  syncState()
}

export function closePanel(kind: PanelKind, panelName?: string) {
  if (!api || !PANELS[kind].closable) return
  for (const p of [...api.panels]) {
    const params = paramsOf(p)
    if (params?.kind === 'stack') {
      const items = params.items ?? []
      const keep = items.filter((i) => !(i.kind === kind && (!isKeyed(kind) || i.panelName === panelName)))
      if (keep.length !== items.length) {
        saveStack(
          keep,
          (params.sizes ?? items.map(() => 1)).filter((_, i) => keep.includes(items[i])),
        )
      }
      continue
    }
    if (params?.kind === kind && (!isKeyed(kind) || params.panelName === panelName)) p.api.close()
  }
}

export function togglePanel(kind: PanelKind, panelName?: string) {
  if (find(kind, panelName)) closePanel(kind, panelName)
  else openPanel(kind, { panelName })
}

export function applyPreset(preset: Preset) {
  if (!api) return
  api.clear()
  ensureZones()
  api.addPanel(panelOptions('chat', undefined, 'chat'))
  const right = zoneGroup('right')
  const bottom = zoneGroup('bottom')

  if (preset === 'research' && right) {
    api.addPanel({ ...panelOptions('preview', undefined, 'preview'), position: { referenceGroup: right } })
    api.addPanel({ ...panelOptions('files', undefined, 'files'), position: { referenceGroup: right }, inactive: true })
  } else if (preset === 'coding' && right) {
    api.addPanel({ ...panelOptions('files', undefined, 'files'), position: { referenceGroup: right } })
    api.addPanel({ ...panelOptions('review', undefined, 'review'), position: { referenceGroup: right }, inactive: true })
  } else if (preset !== 'focus' && right) {
    api.addPanel({ ...panelOptions('files', undefined, 'files'), position: { referenceGroup: right } })
    api.addPanel({ ...panelOptions('preview', undefined, 'preview'), position: { referenceGroup: right }, inactive: true })
  }
  // The bottom panel is Quick access: shortcuts, plus mod tiles. Terminals
  // still live there too, and the coding layout opens on one.
  const withBottom = preset === 'full' || preset === 'coding'
  if (withBottom && bottom) {
    api.addPanel({ ...panelOptions('quick', undefined, 'quick'), position: { referenceGroup: bottom }, inactive: preset === 'coding' })
    if (preset === 'coding') {
      api.addPanel({ ...panelOptions('terminal', undefined, newId('terminal')), position: { referenceGroup: bottom } })
    }
  }
  api.setEdgeGroupVisible('right', preset !== 'focus')
  api.setEdgeGroupVisible('bottom', withBottom)
  keepEmptyZonesOpen()
  api.getPanel('chat')?.api.setActive()
  syncState()
  scheduleSave()
}

/**
 * Keep the chat usable as the window changes.
 *
 * A zone takes only what the chat can spare. If the window is too small for
 * the zone even at its smallest, it steps aside (and returns when there is
 * room again) — unless it was just asked for, in which case it stays and the
 * chat is as tight as that choice makes it.
 */
function fitZone(zone: Zone, total: number) {
  if (!api) return
  const group = zoneGroup(zone)
  if (!group) return
  const min = ZONE_SIZE[zone].minimumSize
  const room = total - CHAT_MIN[zone]
  const visible = api.isEdgeGroupVisible(zone)

  if (room < min) {
    if (visible && !requested[zone]) {
      autoHidden[zone] = true
      api.setEdgeGroupVisible(zone, false)
    }
    return
  }
  requested[zone] = false
  let revealed = false
  if (!visible) {
    if (!autoHidden[zone] || room < min + REVEAL_SLACK) return
    autoHidden[zone] = false
    api.setEdgeGroupVisible(zone, true)
    revealed = true
  }
  // Shrinking the window can leave a zone collapsed to its strip, with the
  // size it should come back to forgotten: bring it back at a sensible size.
  const edge = api.getEdgeGroup(zone)
  const stuck = !!edge?.isCollapsed()
  if (stuck) edge?.expand()
  const size = zone === 'right' ? group.api.width : group.api.height
  const wanted = revealed || stuck || size < min ? Math.min(openingSize(zone), room) : Math.min(size, room)
  if (wanted !== size) group.api.setSize(zone === 'right' ? { width: wanted } : { height: wanted })
}

let fitting = false
let measure: (() => { width: number; height: number }) | null = null
/** The workspace's way of reading its container's usable size. */
export function setFitMeasure(fn: (() => { width: number; height: number }) | null) {
  measure = fn
}
/** Re-fit both zones to the window. Safe to call from a layout event: its
 * own resizing is ignored while it runs. */
export function fitZones() {
  // The container as measured, when the workspace has registered a way to:
  // dockview's own width trails the container by a frame during a resize.
  const size = measure?.()
  const width = size?.width || api?.width
  const height = size?.height || api?.height
  if (!api || fitting || !width || !height) return
  fitting = true
  try {
    fitZone('right', width)
    fitZone('bottom', height)
  } finally {
    fitting = false
  }
  syncState()
}

/** A saved layout with today's zone minimums applied.
 *
 * A zone's minimum is stored in the layout itself, so a layout saved before
 * the minimum existed (or when it was smaller) would restore without it and
 * let the panel be dragged down to nothing. The saved size is raised to the
 * minimum too, so it doesn't open narrower than it can be dragged. */
function withZoneMinimums(saved: unknown): unknown {
  const edges = (saved as { edgeGroups?: Record<string, { minimumSize?: number; size?: number }> } | null)?.edgeGroups
  if (!edges) return saved
  for (const zone of ZONES) {
    const e = edges[zone]
    if (!e) continue
    const min = ZONE_SIZE[zone].minimumSize
    e.minimumSize = Math.max(e.minimumSize ?? 0, min)
    if (typeof e.size === 'number' && e.size < min) e.size = min
  }
  return saved
}

/** Called once dockview is ready: restore the saved layout, or start fresh. */
export function attach(dockview: DockviewApi) {
  api = dockview
  // Dev builds only: the live instance, for poking at the layout from
  // DevTools. A module imported from the console is a separate copy with no
  // dockview attached, so this is the only way to reach the real one.
  if (import.meta.env.DEV) (window as unknown as { __vivDock?: DockviewApi }).__vivDock = dockview

  dockview.onDidLayoutChange(() => {
    keepEmptyZonesOpen()
    fitZones()
    syncState()
    scheduleSave()
  })

  // A chosen startup layout wins over where you left off.
  const startup = useSavedLayouts.getState().startup
  const startupSaved = startup.startsWith('saved:')
    ? useSavedLayouts.getState().saved.find((l) => l.id === startup.slice('saved:'.length))?.layout
    : undefined
  const saved = withZoneMinimums(
    startupSaved ? structuredClone(startupSaved) : startup === 'last' ? loadPersisted<unknown>(STORAGE_KEY, null) : null,
  )
  let restored = false
  if (saved) {
    try {
      dockview.fromJSON(saved as Parameters<DockviewApi['fromJSON']>[0])
      restored = dockview.panels.length > 0
    } catch (e) {
      // A saved layout can name a panel that no longer exists — a custom
      // panel that was deleted, say. Starting over beats a blank window.
      console.warn('[workspace] saved layout could not be restored, starting fresh:', e)
      dockview.clear()
    }
  }
  if (!restored) applyPreset(startup in PRESET_LABELS ? (startup as Preset) : 'default')
  ensureZones()
  // The Editor panel was removed from the app; a layout saved while it was
  // open still restores (its component is kept for exactly this), minus it.
  for (const p of [...dockview.panels]) if (paramsOf(p)?.kind === 'ide') p.api.close()
  // The chat is the one panel that must always exist; a layout saved without
  // it gets it back.
  if (!find('chat')) dockview.addPanel(panelOptions('chat', undefined, 'chat'))
  keepEmptyZonesOpen()
  fitZones()
  syncState()
  useWorkspace.setState({ ready: true })
}

export function detach() {
  api = null
  useWorkspace.setState({ ready: false })
}

/** Throw away the saved arrangement and go back to the default one. */
export function resetLayout() {
  savePersisted(STORAGE_KEY, null)
  applyPreset('default')
}

/** For the crash fallback: clear the saved layout without a live api, so the
 * next launch starts from a known-good arrangement. */
export function forgetSavedLayout() {
  savePersisted(STORAGE_KEY, null)
}
