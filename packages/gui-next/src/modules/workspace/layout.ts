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
import { PANELS, type PanelKind } from './registry'

export type Zone = 'right' | 'bottom'
export type Preset = 'default' | 'full' | 'focus'

export const ZONE_LABELS: Record<Zone, string> = { right: 'Right panel', bottom: 'Bottom panel' }

export const PRESET_LABELS: Record<Preset, string> = {
  default: 'Chat with right panel',
  full: 'Chat with right and bottom panels',
  focus: 'Chat only',
}

const ZONE_IDS: Record<Zone, string> = { right: 'zone-right', bottom: 'zone-bottom' }
const ZONE_SIZE: Record<Zone, { initialSize: number; minimumSize: number }> = {
  right: { initialSize: 400, minimumSize: 240 },
  bottom: { initialSize: 260, minimumSize: 120 },
}

/** Serialized dockview layout. Bumped from v1 when the zones arrived: a v1
 * layout has no edge groups, so it's rebuilt rather than restored. */
const STORAGE_KEY = 'viv.workspace.v2'

interface PanelParams {
  kind: PanelKind
  panelName?: string
}

interface WorkspaceState {
  /** One entry per open panel: `kind`, or `custom:<name>` for custom panels. */
  open: string[]
  zones: Record<Zone, boolean>
  ready: boolean
}

export const useWorkspace = create<WorkspaceState>(() => ({
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

function syncState() {
  if (!api) return
  const open = api.panels.map((p) => {
    const params = paramsOf(p)
    return params ? kindKey(params.kind, params.panelName) : p.id
  })
  useWorkspace.setState({
    open,
    zones: {
      right: api.isEdgeGroupVisible('right'),
      bottom: api.isEdgeGroupVisible('bottom'),
    },
  })
}

function scheduleSave() {
  if (!api) return
  window.clearTimeout(saveTimer)
  // Debounced: a drag fires a layout change per frame.
  saveTimer = window.setTimeout(() => {
    if (api) savePersisted(STORAGE_KEY, api.toJSON())
  }, 400)
}

/** Create any zone that doesn't exist yet. */
function ensureZones() {
  if (!api) return
  for (const zone of ['right', 'bottom'] as Zone[]) {
    if (!api.getEdgeGroup(zone)) api.addEdgeGroup(zone, { id: ZONE_IDS[zone], ...ZONE_SIZE[zone] })
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
  for (const zone of ['right', 'bottom'] as Zone[]) {
    const edge = api.getEdgeGroup(zone)
    const group = zoneGroup(zone)
    if (edge && group && group.panels.length === 0 && api.isEdgeGroupVisible(zone) && edge.isCollapsed()) edge.expand()
  }
}

function showZone(zone: Zone) {
  if (!api) return
  api.setEdgeGroupVisible(zone, true)
  const edge = api.getEdgeGroup(zone)
  if (edge?.isCollapsed()) edge.expand()
}

export function setZoneVisible(zone: Zone, visible: boolean) {
  if (!api) return
  if (visible) showZone(zone)
  else api.setEdgeGroupVisible(zone, false)
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
    title:
      kind === 'custom'
        ? (panelName ?? 'Panel')
        : kind === 'document'
          ? (panelName?.split(/[\\/]/).filter(Boolean).pop() ?? 'Document')
          : def.title,
    params: { kind, panelName },
    renderer: def.renderer,
  }
}

function find(kind: PanelKind, panelName?: string): IDockviewPanel | undefined {
  return api?.panels.find((p) => {
    const params = paramsOf(p)
    return params?.kind === kind && (!isKeyed(kind) || params.panelName === panelName)
  })
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
    const zone: Zone = kind === 'terminal' ? 'bottom' : 'right'
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

  if (preset !== 'focus' && right) {
    api.addPanel({ ...panelOptions('files', undefined, 'files'), position: { referenceGroup: right } })
    api.addPanel({ ...panelOptions('preview', undefined, 'preview'), position: { referenceGroup: right }, inactive: true })
  }
  if (preset === 'full' && bottom) {
    api.addPanel({ ...panelOptions('terminal', undefined, newId('terminal')), position: { referenceGroup: bottom } })
  }
  api.setEdgeGroupVisible('right', preset !== 'focus')
  api.setEdgeGroupVisible('bottom', preset === 'full')
  keepEmptyZonesOpen()
  api.getPanel('chat')?.api.setActive()
  syncState()
  scheduleSave()
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
    syncState()
    scheduleSave()
  })

  const saved = loadPersisted<unknown>(STORAGE_KEY, null)
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
  if (!restored) applyPreset('default')
  ensureZones()
  // The Editor panel was removed from the app; a layout saved while it was
  // open still restores (its component is kept for exactly this), minus it.
  for (const p of [...dockview.panels]) if (paramsOf(p)?.kind === 'ide') p.api.close()
  // The chat is the one panel that must always exist; a layout saved without
  // it gets it back.
  if (!find('chat')) dockview.addPanel(panelOptions('chat', undefined, 'chat'))
  keepEmptyZonesOpen()
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
