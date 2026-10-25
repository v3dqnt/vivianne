import { useRef, useState, type ReactNode } from 'react'
import { Box, Menu, MenuItem, Stack, Tooltip, Typography, useTheme } from '../../ui'
import { AddLinear, CloseLinear, LayerLinear, MaximizeLinear, MinimizeLinear } from '../../icons'
import { usePanelStore } from '../dock/store/panelStore'
import { useModsStore, modPanels } from '../mods/store/modsStore'
import { SURFACE_RADIUS } from '../../theme'
import { ADDABLE, PANELS } from './registry'
import { itemKey, openPanel, titleFor, type StackItem } from './layout'

const MIN_CARD = 120
const GAP = 6

const btnSx = {
  width: 24,
  height: 24,
  border: 0,
  bgcolor: 'transparent',
  color: 'text.secondary',
  borderRadius: '6px',
  cursor: 'pointer',
  display: 'grid',
  placeItems: 'center',
  flexShrink: 0,
  '&:hover': { bgcolor: 'action.hover', color: 'text.primary' },
} as const

interface Props {
  items: StackItem[]
  sizes?: number[]
  /** Save a new arrangement (items and their relative heights). */
  save: (items: StackItem[], sizes: number[]) => void
  /** Leave stacked mode: the panels go back to tabs. */
  toTabs: () => void
  /** Nothing left to show. */
  closeAll: () => void
  render: (item: StackItem, close: () => void) => ReactNode
}

/**
 * Several panels at once, one above another, each its own card with a title,
 * an expand button and a close button: the alternative to tabs for a side
 * panel you want to watch (a diff, the plan and the background tasks, say).
 * Drag the gap between two cards to resize them.
 */
export function StackedPanels({ items, sizes, save, toTabs, closeAll, render }: Props) {
  const theme = useTheme()
  const t = theme.palette.tokens
  const [solo, setSolo] = useState<string | null>(null)
  const [anchor, setAnchor] = useState<HTMLElement | null>(null)
  const [live, setLive] = useState<number[] | null>(null)
  const cards = useRef<(HTMLDivElement | null)[]>([])

  const weights = live ?? items.map((_, i) => sizes?.[i] ?? 1)
  const soloIndex = solo === null ? -1 : items.findIndex((i) => itemKey(i) === solo)
  const shown = soloIndex >= 0 ? [soloIndex] : items.map((_, i) => i)
  // Shares of the height among the cards showing. As flex-grow they must add
  // up to at least 1, or the cards share out only that fraction of the room
  // and leave the rest blank (as when one of two was closed): so they are
  // scaled to a total of 100.
  const total = shown.reduce((sum, i) => sum + (weights[i] > 0 ? weights[i] : 0), 0) || 1
  const grow = (i: number) => ((weights[i] > 0 ? weights[i] : 0) / total) * 100

  const remove = (key: string) => {
    const index = items.findIndex((i) => itemKey(i) === key)
    if (index < 0) return
    const next = items.filter((_, i) => i !== index)
    if (!next.length) return closeAll()
    setSolo(null)
    save(
      next,
      weights.filter((_, i) => i !== index),
    )
  }

  // Dragging the gap below card `a` trades height between it and the next
  // shown card, keeping their combined share.
  const drag = (a: number, b: number, e: React.PointerEvent) => {
    const first = cards.current[a]
    const second = cards.current[b]
    if (!first || !second) return
    e.preventDefault()
    const startY = e.clientY
    const h1 = first.getBoundingClientRect().height
    const h2 = second.getBoundingClientRect().height
    const sum = weights[a] + weights[b]
    const shares = (clientY: number) => {
      const top = Math.min(Math.max(h1 + clientY - startY, MIN_CARD), h1 + h2 - MIN_CARD)
      const w = [...weights]
      w[a] = (sum * top) / (h1 + h2)
      w[b] = sum - w[a]
      return w
    }
    // A page or webview under the pointer would swallow the move events and
    // freeze the drag, so panels are told a resize is going on (as for the
    // dock's own sashes) and the cursor is pinned to the whole page.
    usePanelStore.getState().setPanelResizing(true)
    const prevCursor = document.body.style.cursor
    document.body.style.cursor = 'row-resize'
    const move = (ev: PointerEvent) => setLive(shares(ev.clientY))
    const up = (ev: PointerEvent) => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', up)
      window.removeEventListener('pointercancel', up)
      document.body.style.cursor = prevCursor
      usePanelStore.getState().setPanelResizing(false)
      const w = shares(ev.clientY)
      setLive(null)
      save(items, w)
    }
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', up)
    window.addEventListener('pointercancel', up)
  }

  /** Keyboard: arrows trade 24px between two neighbours. */
  const nudge = (a: number, b: number, by: number) => {
    const first = cards.current[a]
    const second = cards.current[b]
    if (!first || !second) return
    const h1 = first.getBoundingClientRect().height
    const h2 = second.getBoundingClientRect().height
    const top = Math.min(Math.max(h1 + by, MIN_CARD), h1 + h2 - MIN_CARD)
    const sum = weights[a] + weights[b]
    const w = [...weights]
    w[a] = (sum * top) / (h1 + h2)
    w[b] = sum - w[a]
    save(items, w)
  }

  const mods = useModsStore((s) => s.mods)
  const missing = ADDABLE.filter((k) => k !== 'quick' && !items.some((i) => i.kind === k))
  const modMissing = modPanels(mods).filter((p) => !items.some((i) => i.kind === 'custom' && i.panelName === p.name))

  return (
    <Stack data-viv-stack sx={{ height: '100%', minHeight: 0, overflowY: 'auto', gap: `${GAP}px` }}>
      {shown.map((index, position) => {
        const item = items[index]
        const key = itemKey(item)
        const Icon = PANELS[item.kind].icon
        const first = position === 0
        const next = shown[position + 1]
        return (
          <Box key={key} sx={{ display: 'contents' }}>
            <Box
              ref={(el: HTMLDivElement | null) => {
                cards.current[index] = el
              }}
              sx={{
                flex: `${grow(index)} 1 0`,
                minHeight: MIN_CARD,
                display: 'flex',
                flexDirection: 'column',
                overflow: 'hidden',
                bgcolor: t.background,
                border: `1px solid ${t.border}`,
                borderRadius: `${SURFACE_RADIUS}px`,
              }}
            >
              <Stack direction="row" sx={{ alignItems: 'center', gap: 0.25, px: 1.25, height: 36, flexShrink: 0 }}>
                <Icon size={12} color="currentColor" />
                <Typography noWrap sx={{ flex: 1, minWidth: 0, ml: 0.5, fontSize: '0.8125rem', fontWeight: 500 }}>
                  {titleFor(item.kind, item.panelName)}
                </Typography>
                {first && soloIndex < 0 && (
                  <>
                    <Tooltip title="Add a panel">
                      <Box component="button" type="button" aria-label="Add a panel" onClick={(e) => setAnchor(e.currentTarget as HTMLElement)} sx={btnSx}>
                        <AddLinear size={13} color="currentColor" />
                      </Box>
                    </Tooltip>
                    <Tooltip title="Show as tabs">
                      <Box component="button" type="button" aria-label="Show as tabs" onClick={toTabs} sx={btnSx}>
                        <LayerLinear size={13} color="currentColor" />
                      </Box>
                    </Tooltip>
                  </>
                )}
                {items.length > 1 && (
                  <Tooltip title={soloIndex >= 0 ? 'Restore' : 'Expand'}>
                    <Box
                      component="button"
                      type="button"
                      aria-label={soloIndex >= 0 ? 'Restore' : 'Expand'}
                      onClick={() => setSolo(soloIndex >= 0 ? null : key)}
                      sx={btnSx}
                    >
                      {soloIndex >= 0 ? <MinimizeLinear size={12} color="currentColor" /> : <MaximizeLinear size={12} color="currentColor" />}
                    </Box>
                  </Tooltip>
                )}
                <Tooltip title="Close">
                  <Box component="button" type="button" aria-label={`Close ${titleFor(item.kind, item.panelName)}`} onClick={() => remove(key)} sx={btnSx}>
                    <CloseLinear size={12} color="currentColor" />
                  </Box>
                </Tooltip>
              </Stack>
              <Box sx={{ flex: 1, minHeight: 0, overflow: 'hidden' }}>{render(item, () => remove(key))}</Box>
            </Box>
            {next !== undefined && (
              <Box
                role="separator"
                aria-orientation="horizontal"
                aria-label="Resize panels"
                tabIndex={0}
                onPointerDown={(e) => drag(index, next, e)}
                // Double-click shares the two cards' height equally again.
                onDoubleClick={() => {
                  const w = [...weights]
                  const sum = w[index] + w[next]
                  w[index] = sum / 2
                  w[next] = sum / 2
                  save(items, w)
                }}
                onKeyDown={(e) => {
                  if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
                    e.preventDefault()
                    nudge(index, next, e.key === 'ArrowUp' ? -24 : 24)
                  }
                }}
                sx={{
                  // The gap is the handle: taller than it looks so it is easy
                  // to grab, with a grip that shows on hover, focus and drag.
                  position: 'relative',
                  // The gap is the handle: taller than it looks so it is easy
                  // to grab, with a grip that shows on hover, focus and drag.
                  height: 14,
                  my: '-4px',
                  cursor: 'row-resize',
                  flexShrink: 0,
                  touchAction: 'none',
                  outline: 'none',
                  '&::after': {
                    content: '""',
                    position: 'absolute',
                    left: '50%',
                    top: '50%',
                    width: 36,
                    height: 4,
                    borderRadius: 2,
                    transform: 'translate(-50%, -50%)',
                    bgcolor: live ? 'primary.main' : t.border,
                    opacity: live ? 1 : 0.7,
                    transition: 'background-color 0.12s, opacity 0.12s, width 0.12s',
                  },
                  '&:hover::after, &:focus-visible::after': { bgcolor: 'primary.main', opacity: 1, width: 56 },
                }}
              />
            )}
          </Box>
        )
      })}
      <Menu anchorEl={anchor} open={!!anchor} onClose={() => setAnchor(null)}>
        {missing.length === 0 && modMissing.length === 0 && (
          <MenuItem disabled sx={{ fontSize: '0.8125rem' }}>
            Everything is already here
          </MenuItem>
        )}
        {missing.map((kind) => {
          const MenuIcon = PANELS[kind].icon
          return (
            <MenuItem
              key={kind}
              onClick={() => {
                setAnchor(null)
                openPanel(kind)
              }}
              sx={{ gap: 1, fontSize: '0.8125rem' }}
            >
              <MenuIcon size={14} color="currentColor" />
              {PANELS[kind].title}
            </MenuItem>
          )
        })}
        {modMissing.map((p) => (
          <MenuItem
            key={p.name}
            onClick={() => {
              setAnchor(null)
              openPanel('custom', { panelName: p.name })
            }}
            sx={{ gap: 1, fontSize: '0.8125rem' }}
          >
            <LayerLinear size={14} color="currentColor" />
            {p.title}
          </MenuItem>
        ))}
      </Menu>
    </Stack>
  )
}
