import { useMemo, useState } from 'react'
import { create } from 'zustand'
import { Box, ButtonBase, Stack, Tooltip, Typography, alpha, useTheme } from '../../ui'
import { ArrowDownLinear, CloseLinear, LayerLinear } from '../../icons'
import { loadPersisted, savePersisted } from '../../lib/persist'
import { useAgentStore } from '../ai/store/agentStore'
import { quickTiles, runningMods, useModsStore } from '../mods/store/modsStore'
import type { ModInfo } from '../mods/lib/types'
import { ADDABLE, PANELS, type PanelKind } from './registry'
import { openPanel } from './layout'

/** What the user changed about the tiles: the ones they hid and the order
 * they put them in. Anything not mentioned keeps its default place. */
interface QuickPrefs {
  hidden: string[]
  order: string[]
}

const PREFS_KEY = 'viv.quickAccess.v1'

const usePrefs = create<QuickPrefs>(() => loadPersisted<QuickPrefs>(PREFS_KEY, { hidden: [], order: [] }))

function setPrefs(next: QuickPrefs) {
  savePersisted(PREFS_KEY, next)
  usePrefs.setState(next)
}

interface Tile {
  /** Stable across sessions: `app:<kind>` or `mod:<mod key>/<tile id>`. */
  id: string
  title: string
  description?: string
  /** An emoji or symbol, for tiles that bring their own. */
  glyph?: string
  icon?: (typeof PANELS)[PanelKind]['icon']
  from?: string
  run: () => void
}

const APP_TILES: { kind: Exclude<PanelKind, 'chat' | 'stack' | 'custom' | 'ide' | 'document' | 'documents'>; description: string; run?: () => void }[] = [
  { kind: 'terminal', description: 'A new shell', run: () => openPanel('terminal', { newInstance: true }) },
  { kind: 'files', description: 'Browse the project' },
  { kind: 'agents', description: 'Your team and what it is doing' },
  { kind: 'plan', description: 'The plan being written' },
  { kind: 'preview', description: 'The page or document' },
  { kind: 'review', description: 'Changes, ready to review' },
  { kind: 'tasks', description: 'What is running behind the scenes' },
  { kind: 'timeline', description: 'What happened, in order' },
  { kind: 'memory', description: 'What Vivianne remembers' },
  { kind: 'multiplayer', description: 'Work with others' },
]

/** Tiles for the mods' `quickAccess` contributions. A tile can only do what
 * its mod was granted: the manifest check refuses the rest. */
function modTiles(mods: ModInfo[]): Tile[] {
  return quickTiles(mods).map(({ mod, tile }) => ({
    id: `mod:${mod.key}/${tile.id}`,
    title: tile.title,
    description: tile.description ?? undefined,
    glyph: tile.icon ?? undefined,
    from: mod.manifest?.name,
    run: () => {
      const a = tile.action
      if (a.type === 'panel') openPanel('custom', { panelName: `${mod.key}/${a.panel}` })
      else if (a.type === 'open') openPanel(a.target as PanelKind)
      else if (a.type === 'compose') useAgentStore.getState().setDraft(a.text)
      else if (a.type === 'send') void useAgentStore.getState().sendMessage(a.text)
    },
  }))
}

/**
 * Quick access: one-press shortcuts to what you reach for most. Mods add
 * their own tiles (`contributes.quickAccess`); Customize hides, shows and
 * reorders them.
 */
export function QuickAccess() {
  const theme = useTheme()
  const mods = useModsStore((s) => s.mods)
  const prefs = usePrefs()
  const [editing, setEditing] = useState(false)

  const tiles = useMemo(() => {
    const app: Tile[] = APP_TILES.filter((t) => (ADDABLE as string[]).includes(t.kind)).map((t) => ({
      id: `app:${t.kind}`,
      title: t.kind === 'terminal' ? 'Terminal' : PANELS[t.kind].title,
      description: t.description,
      icon: PANELS[t.kind].icon,
      run: t.run ?? (() => openPanel(t.kind)),
    }))
    const all = [...app, ...modTiles(runningMods(mods))]
    const rank = (id: string) => {
      const i = prefs.order.indexOf(id)
      return i < 0 ? Number.MAX_SAFE_INTEGER : i
    }
    // Stable: tiles without a saved place stay in their default order.
    return all.map((t, i) => ({ t, i })).sort((a, b) => rank(a.t.id) - rank(b.t.id) || a.i - b.i).map(({ t }) => t)
  }, [mods, prefs.order])

  const shown = editing ? tiles : tiles.filter((t) => !prefs.hidden.includes(t.id))

  const toggle = (id: string) =>
    setPrefs({ ...prefs, hidden: prefs.hidden.includes(id) ? prefs.hidden.filter((h) => h !== id) : [...prefs.hidden, id] })

  const move = (id: string, by: -1 | 1) => {
    const ids = tiles.map((t) => t.id)
    const at = ids.indexOf(id)
    const to = at + by
    if (at < 0 || to < 0 || to >= ids.length) return
    ;[ids[at], ids[to]] = [ids[to], ids[at]]
    setPrefs({ ...prefs, order: ids })
  }

  return (
    <Stack sx={{ height: '100%', minHeight: 0 }}>
      {/* The panel's tab already says "Quick access"; this row is its tools. */}
      <Stack direction="row" sx={{ alignItems: 'center', justifyContent: 'flex-end', px: 1.5, height: 28, flexShrink: 0 }}>
        {editing && (
          <ButtonBase
            onClick={() => setPrefs({ hidden: [], order: [] })}
            sx={{ px: 1, py: 0.25, mr: 0.5, borderRadius: '6px', fontSize: '0.75rem', color: 'text.secondary', '&:hover': { bgcolor: 'action.hover' } }}
          >
            Reset
          </ButtonBase>
        )}
        <ButtonBase
          onClick={() => setEditing((v) => !v)}
          sx={{
            px: 1,
            py: 0.25,
            borderRadius: '6px',
            fontSize: '0.75rem',
            color: editing ? 'primary.main' : 'text.secondary',
            '&:hover': { bgcolor: 'action.hover' },
          }}
        >
          {editing ? 'Done' : 'Customize'}
        </ButtonBase>
      </Stack>
      <Box
        sx={{
          flex: 1,
          minHeight: 0,
          overflowY: 'auto',
          px: 1.5,
          pb: 1.5,
          display: 'grid',
          // As many columns as fit; the grid reflows with the panel.
          gridTemplateColumns: 'repeat(auto-fill, minmax(170px, 1fr))',
          gridAutoRows: 'min-content',
          gap: 0.75,
        }}
      >
        {shown.map((tile) => {
          const hidden = prefs.hidden.includes(tile.id)
          const Icon = tile.icon ?? LayerLinear
          return (
            <Box key={tile.id} sx={{ position: 'relative', opacity: hidden ? 0.45 : 1 }}>
              <ButtonBase
                onClick={() => (editing ? toggle(tile.id) : tile.run())}
                sx={{
                  width: '100%',
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'flex-start',
                  textAlign: 'left',
                  gap: 1.25,
                  px: 1.25,
                  py: 1,
                  borderRadius: '10px',
                  border: 1,
                  borderColor: 'divider',
                  color: 'text.primary',
                  transition: 'background-color 0.12s, border-color 0.12s',
                  '&:hover': { bgcolor: alpha(theme.palette.primary.main, 0.08), borderColor: alpha(theme.palette.primary.main, 0.4) },
                }}
              >
                <Box sx={{ width: 22, display: 'grid', placeItems: 'center', flexShrink: 0, color: 'text.secondary', fontSize: '1rem', lineHeight: 1 }}>
                  {tile.glyph ? tile.glyph : <Icon size={16} color="currentColor" />}
                </Box>
                <Stack sx={{ minWidth: 0 }}>
                  <Typography noWrap sx={{ fontSize: '0.8125rem', fontWeight: 500 }}>
                    {tile.title}
                  </Typography>
                  {(tile.description || tile.from) && (
                    <Typography noWrap variant="caption" color="text.secondary">
                      {tile.description ?? tile.from}
                    </Typography>
                  )}
                </Stack>
              </ButtonBase>
              {editing && (
                <Stack direction="row" sx={{ position: 'absolute', top: 4, right: 4, gap: 0.25 }}>
                  <Tooltip title="Move earlier">
                    <ButtonBase onClick={() => move(tile.id, -1)} aria-label={`Move ${tile.title} earlier`} sx={{ width: 20, height: 20, borderRadius: '5px', color: 'text.secondary', transform: 'rotate(90deg)', '&:hover': { bgcolor: 'action.hover' } }}>
                      <ArrowDownLinear size={11} color="currentColor" />
                    </ButtonBase>
                  </Tooltip>
                  <Tooltip title="Move later">
                    <ButtonBase onClick={() => move(tile.id, 1)} aria-label={`Move ${tile.title} later`} sx={{ width: 20, height: 20, borderRadius: '5px', color: 'text.secondary', transform: 'rotate(-90deg)', '&:hover': { bgcolor: 'action.hover' } }}>
                      <ArrowDownLinear size={11} color="currentColor" />
                    </ButtonBase>
                  </Tooltip>
                  <Tooltip title={hidden ? 'Show' : 'Hide'}>
                    <ButtonBase onClick={() => toggle(tile.id)} aria-label={`${hidden ? 'Show' : 'Hide'} ${tile.title}`} sx={{ width: 20, height: 20, borderRadius: '5px', color: 'text.secondary', '&:hover': { bgcolor: 'action.hover' } }}>
                      <CloseLinear size={11} color="currentColor" />
                    </ButtonBase>
                  </Tooltip>
                </Stack>
              )}
            </Box>
          )
        })}
        {shown.length === 0 && (
          <Typography variant="caption" color="text.secondary" sx={{ gridColumn: '1 / -1', py: 2, textAlign: 'center' }}>
            Everything is hidden. Customize to bring tiles back.
          </Typography>
        )}
      </Box>
    </Stack>
  )
}
