/**
 * The workspace layout: the chat in the centre, one side container beside it.
 *
 * ## Zones
 *
 * The side container is a dockview *edge group*: it belongs to the window's
 * edge rather than the grid, it can be shown and hidden as a unit from the
 * title bar, and — unlike ordinary groups, which dockview deletes the moment
 * their last tab leaves — it persists when empty. It sits on whichever edge
 * the chat is not: chat on the left puts the container on the right, and the
 * other way round.
 *
 * The centre is the chat's alone: nothing docks beside it or joins its tab
 * strip, and the chat itself stays put.
 *
 * ## Who calls this
 *
 * The UI (title bar, panel headers, empty states, settings) and the agent's
 * `set_panel` tool via `panelStore`. Both ask for a panel by name — "open the
 * files" — and placement is decided here.
 */
import { create } from 'zustand'
import type { DockviewApi, DockviewGroupPanel, IDockviewPanel } from 'dockview-react'
import { loadPersisted, savePersisted } from '../../lib/persist'
import { ADDABLE, PANELS, type PanelKind, RETIRED } from './registry'

export type Zone = 'left' | 'right'
/** Where the chat sits across the window. */
export type ChatSide = 'left' | 'right'
export type Preset = 'default' | 'focus' | 'coding' | 'research'

export const ZONE_LABELS: Record<Zone, string> = { left: 'Side panel', right: 'Side panel' }

export const CHAT_SIDE_LABELS: Record<ChatSide, string> = { left: 'Left', right: 'Right' }

const CHAT_SIDE_KEY = 'viv.workspace.chatSide'

/** The edge the side container uses: the one the chat is not on. */
export function sideZone(side: ChatSide = useWorkspace.getState().chatSide): Zone {
  return side === 'right' ? 'left' : 'right'
}

export const PRESET_LABELS: Record<Preset, string> = {
  default: 'Chat with files',
  focus: 'Chat only',
  coding: 'Coding: chat and a terminal',
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

/** Close the panels a saved layout still names that the app no longer has. */
function dropRetired(dv: DockviewApi) {
  for (const p of [...dv.panels]) if (RETIRED.has(p.id)) p.api.close()
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
  dropRetired(api)
  ensureZones()
  if (!find('chat')) api.addPanel(panelOptions('chat', 'chat'))
  confine()
  keepEmptyZonesOpen()
  syncState()
  scheduleSave()
  return true
}

const ZONES: Zone[] = ['left', 'right']
const ZONE_IDS: Record<Zone, string> = { left: 'zone-left', right: 'zone-right' }
/** How big the container starts, and the smallest it can be dragged to. Below
 * that a panel stops being usable: the file tree needs room for a path, the
 * browser for a page. */
const ZONE_SIZE: Record<Zone, { initialSize: number; minimumSize: number }> = {
  left: { initialSize: 400, minimumSize: 320 },
  right: { initialSize: 400, minimumSize: 320 },
}

/** The least room the chat keeps, however the container is sized or the
 * window shrinks. Below it the chat stops being a place to type and read. */
const CHAT_MIN = { width: 440, height: 240 } as const
/** A hidden zone comes back only once it has this much room to spare, so a
 * window resized across the line doesn't flicker it in and out. */
const REVEAL_SLACK = 40

/** Zones hidden because the window got too small for them (not by the user),
 * to be brought back when it grows. */
const autoHidden: Record<Zone, boolean> = { left: false, right: false }
/** Zones the user (or the agent) just asked for: kept even when the chat is
 * tight, rather than snatched away again by the fit below. */
const requested: Record<Zone, boolean> = { left: false, right: false }

/** How big the container opens for a window of this size: a share of it,
 * within the container's own bounds. A fixed 400px was a third of a laptop
 * screen and most of a small window. */
function openingSize(zone: Zone): number {
  const total = api?.width
  const { initialSize, minimumSize } = ZONE_SIZE[zone]
  if (!total) return initialSize
  return Math.round(Math.min(Math.max(total * 0.34, minimumSize), initialSize * 1.2))
}

/** Serialized dockview layout. Bumped from v1 when the zones arrived: a v1
 * layout has no edge groups, so it's rebuilt rather than restored. */
const STORAGE_KEY = 'viv.workspace.v3'

interface PanelParams {
  kind: PanelKind
}

interface WorkspaceState {
  /** One entry per open panel, the chat aside. */
  open: PanelKind[]
  zones: Record<Zone, boolean>
  chatSide: ChatSide
  ready: boolean
}

export const useWorkspace = create<WorkspaceState>(() => ({
  open: [],
  zones: { left: false, right: false },
  // A layout saved with the chat in the centre predates the single side
  // container: it loads as the chat on the left, which is the same thing.
  chatSide: loadPersisted<ChatSide | 'centre'>(CHAT_SIDE_KEY, 'left') === 'centre' ? 'left' : loadPersisted<ChatSide>(CHAT_SIDE_KEY, 'left'),
  ready: false,
}))

let api: DockviewApi | null = null
let saveTimer: number | undefined

/** The name a panel goes by in a tab. */
export function titleFor(kind: PanelKind): string {
  return PANELS[kind].title
}

function paramsOf(panel: IDockviewPanel): PanelParams | undefined {
  return panel.params as PanelParams | undefined
}

export function zoneOf(group: { id: string } | undefined): Zone | null {
  if (!group) return null
  return group.id === ZONE_IDS.left ? 'left' : group.id === ZONE_IDS.right ? 'right' : null
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
      g.api.setConstraints(isChat ? { minimumWidth: CHAT_MIN.width, minimumHeight: CHAT_MIN.height } : { minimumWidth: 0, minimumHeight: 0 })
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
    return params ? [params.kind] : []
  })
  // The chat's own group has nothing to switch between: no strip.
  for (const g of api.groups) g.header.hidden = g.panels.length === 1 && !zoneOf(g) && paramsOf(g.panels[0])?.kind === 'chat'
  useWorkspace.setState({
    open: open.filter((k) => k !== 'chat'),
    zones: {
      left: api.isEdgeGroupVisible('left'),
      right: api.isEdgeGroupVisible('right'),
    },
  })
}

/** The layout as it should be remembered: a zone that stepped aside only
 * because the window was small is kept as showing, so it is there again when
 * the window is big enough (a choice the user made is saved as made). */
function snapshot(): ReturnType<DockviewApi['toJSON']> {
  const json = api!.toJSON()
  const edges = (json as unknown as { edgeGroups?: Record<string, { visible?: boolean }> }).edgeGroups
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
  id: string,
): { id: string; component: string; title: string; params: PanelParams; renderer: 'always' | 'onlyWhenVisible' } {
  const def = PANELS[kind]
  return { id, component: kind, title: def.title, params: { kind }, renderer: def.renderer }
}

function find(kind: PanelKind): IDockviewPanel | undefined {
  return api?.panels.find((p) => paramsOf(p)?.kind === kind)
}

/**
 * Open a panel, or focus it if it is already open. Everything is
 * single-instance and everything lives in the side container, which is
 * revealed if it was hidden — opening something into a hidden container
 * would look like nothing happened.
 */
export function openPanel(kind: PanelKind, opts: { group?: DockviewGroupPanel } = {}) {
  if (!api) return
  const existing = find(kind)
  if (existing) {
    if (opts.group && existing.group.id !== opts.group.id) existing.api.moveTo({ group: opts.group })
    const zone = zoneOf(existing.group)
    if (zone) showZone(zone)
    existing.api.setActive()
    syncState()
    return
  }
  const zone = sideZone()
  showZone(zone)
  const target = opts.group && zoneOf(opts.group) ? opts.group : zoneGroup(zone)
  api.addPanel({
    ...panelOptions(kind, kind),
    ...(target ? { position: { referenceGroup: target } } : {}),
  })
  showZone(zone)
  syncState()
  scheduleSave()
}

export function closePanel(kind: PanelKind) {
  if (!api || !PANELS[kind].closable) return
  find(kind)?.api.close()
}

export function togglePanel(kind: PanelKind) {
  if (find(kind)) closePanel(kind)
  else openPanel(kind)
}

export function applyPreset(preset: Preset) {
  if (!api) return
  api.clear()
  ensureZones()
  api.addPanel(panelOptions('chat', 'chat'))
  const zone = sideZone()
  const home = zoneGroup(zone)
  if (preset !== 'focus' && home) {
    const order: PanelKind[] = preset === 'coding'
      ? ['terminal', 'files', 'browser']
      : preset === 'research'
        ? ['browser', 'files', 'terminal']
        : ['files', 'browser', 'terminal']
    order.forEach((kind, i) => {
      api!.addPanel({ ...panelOptions(kind, kind), position: { referenceGroup: home }, inactive: i > 0 })
    })
  }
  api.setEdgeGroupVisible(zone, preset !== 'focus')
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
  const room = total - CHAT_MIN.width
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
  const size = group.api.width
  const wanted = revealed || stuck || size < min ? Math.min(openingSize(zone), room) : Math.min(size, room)
  if (wanted !== size) group.api.setSize({ width: wanted })
}

let fitting = false
let measure: (() => { width: number; height: number }) | null = null
/** The workspace's way of reading its container's usable size. */
export function setFitMeasure(fn: (() => { width: number; height: number }) | null) {
  measure = fn
}
/** Re-fit the container to the window. Safe to call from a layout event: its
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
    fitZone(sideZone(), width)
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

/**
 * Hold the layout to the chat's position: the centre holds only the chat, and
 * every other panel belongs to the edge opposite it (a layout saved with the
 * chat elsewhere, or with panels in the wrong container, is put right).
 */
function confine() {
  if (!api) return
  const home = zoneGroup(sideZone())
  if (!home) return
  for (const p of [...api.panels]) {
    if (paramsOf(p)?.kind === 'chat') continue
    if (zoneOf(p.group) !== zoneOf(home)) p.api.moveTo({ group: home })
  }
  // The chat back in the centre, out of any zone it was dragged to.
  const chat = find('chat')
  if (chat && zoneOf(chat.group)) {
    const centre = api.groups.find((g) => !zoneOf(g) && g !== chat.group)
    if (centre) chat.api.moveTo({ group: centre })
    else {
      chat.api.close()
      api.addPanel(panelOptions('chat', 'chat'))
    }
  }
  const other: Zone = sideZone() === 'left' ? 'right' : 'left'
  if (api.isEdgeGroupVisible(other)) api.setEdgeGroupVisible(other, false)
  if (!zoneGroup(sideZone())?.panels.length && api.isEdgeGroupVisible(sideZone())) {
    api.setEdgeGroupVisible(sideZone(), false)
  }
}

/**
 * Show a panel full screen: lifted out of its zone into a floating group
 * that covers the workspace, like a dialog, still the same live panel (a
 * terminal keeps its shell, the preview its page). `dockBack` returns it.
 *
 * Its tab strip doubles as the drag handle (`dragHandle: 'tabbar'`): a dialog
 * wants one row of chrome, not a grip bar above a tab row.
 */
export function openFullScreen(panelId: string) {
  const panel = api?.getPanel(panelId)
  if (!api || !panel) return
  if (isFloating(panel.group)) {
    panel.api.setActive()
    return
  }
  const size = measure?.() ?? { width: window.innerWidth, height: window.innerHeight }
  const inset = 12
  const source = panel.group
  const from = zoneOf(source)
  api.addFloatingGroup(panel, {
    x: inset,
    y: inset,
    width: Math.max(320, size.width - inset * 2),
    height: Math.max(240, size.height - inset * 2),
    dragHandle: 'tabbar',
  })
  panel.api.setActive()
  // A container that just handed over its only panel has nothing left to
  // show, so it steps aside the way its own close button does. One with other
  // tabs keeps them where they were.
  if (from && source.panels.length === 0) setZoneVisible(from, false)
  syncState()
}

/** Whether a group is one `openFullScreen` lifted out. */
export function isFloating(group: { api: { location: { type: string } } } | undefined): boolean {
  return group?.api.location.type === 'floating'
}

/**
 * Close a whole container, not only the panel in front of it.
 *
 * A zone belongs to the frame, so it steps aside rather than being destroyed:
 * its panels stay open, and the title bar's Panels menu brings the container
 * back with all of them. A floating group is only a temporary home for a
 * dialog, so closing it closes what it carried — `dockBack` is the way to put
 * the panel back where it came from without closing it.
 */
export function closeContainer(group: DockviewGroupPanel) {
  if (!api) return
  const zone = zoneOf(group)
  if (zone) {
    setZoneVisible(zone, false)
    return
  }
  for (const p of [...group.panels]) {
    if (paramsOf(p)?.kind === 'chat') continue
    p.api.close()
  }
  syncState()
  scheduleSave()
}

/** Put a full-screen panel back in the side container. */
export function dockBack(group: DockviewGroupPanel) {
  if (!api) return
  const zone = sideZone()
  showZone(zone)
  const target = zoneGroup(zone)
  for (const p of [...group.panels]) if (target) p.api.moveTo({ group: target })
  syncState()
  scheduleSave()
}

/** Put the chat on the left or the right. The container follows it. */
export function setChatSide(side: ChatSide) {
  savePersisted(CHAT_SIDE_KEY, side)
  useWorkspace.setState({ chatSide: side })
  if (!api) return
  ensureZones()
  confine()
  // Panels move across with the chat; the container opens if it has any.
  const zone = sideZone()
  if (zoneGroup(zone)?.panels.length) showZone(zone)
  keepEmptyZonesOpen()
  fitZones()
  syncState()
  scheduleSave()
}

/**
 * A side panel dragged wider (or a bottom one taller) raises no layout
 * event, so the fit that keeps the chat its room never ran and the panel
 * could be dragged right over the chat. Each zone's size is watched instead,
 * and held between its minimum and what the chat can spare, frame by frame.
 */
let zoneObserver: ResizeObserver | null = null
function watchZoneSizes(dv: DockviewApi) {
  zoneObserver?.disconnect()
  let raf = 0
  zoneObserver = new ResizeObserver(() => {
    cancelAnimationFrame(raf)
    raf = requestAnimationFrame(fitZones)
  })
  for (const zone of ZONES) {
    const el = dv.groups.find((g) => g.id === ZONE_IDS[zone])?.element
    if (el) zoneObserver.observe(el)
  }
  // And once more when a drag ends, whatever the observer saw.
  if (!releaseHooked) {
    releaseHooked = true
    window.addEventListener('pointerup', () => fitZones(), { capture: true })
  }
}
let releaseHooked = false

/** Called once dockview is ready: restore the saved layout, or start fresh. */
export function attach(dockview: DockviewApi) {
  api = dockview
  // Dev builds only: the live instance, for poking at the layout from
  // DevTools. A module imported from the console is a separate copy with no
  // dockview attached, so this is the only way to reach the real one.
  if (import.meta.env.DEV) Object.assign(window, { __vivDock: dockview, __vivFit: fitZones })

  // Panels drop only into the zones; the chat's tab doesn't drag.
  dockview.onWillShowOverlay((e) => {
    if (!zoneOf(e.group)) e.preventDefault()
  })
  dockview.onWillDragPanel((e) => {
    if (paramsOf(e.panel)?.kind === 'chat') e.nativeEvent.preventDefault()
  })

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
      dropRetired(dockview)
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
  // The chat is the one panel that must always exist; a layout saved without
  // it gets it back.
  if (!find('chat')) dockview.addPanel(panelOptions('chat', 'chat'))
  // A layout saved when the chat used the overlay renderer keeps it.
  find('chat')?.api.setRenderer(PANELS.chat.renderer)
  // A restored layout keeps whatever survived; the three panels are seeded
  // when missing so the container is never a chooser on launch. A preset that
  // hides the container stays hidden.
  if (api.isEdgeGroupVisible(sideZone())) for (const kind of ADDABLE) if (!find(kind)) openPanel(kind)
  confine()
  keepEmptyZonesOpen()
  fitZones()
  syncState()
  watchZoneSizes(dockview)
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
