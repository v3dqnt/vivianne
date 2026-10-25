import { useCallback, useEffect, useMemo, useState } from 'react'
import { Alert, Box, ButtonBase, InputBase, Stack, Tooltip, Typography, alpha, useTheme } from '../../ui'
import { BrainLinear, SearchNormalLinear, EditLinear, TrashLinear, TickCircleLinear, CloseLinear } from '../../icons'
import { call } from '../../ipc'

/** Mirrors `agent::memory::MemoryEntry`. */
interface Memory {
  id: string
  key: string
  text: string
  kind: string
  createdAt: number
  confidence: number
  accessCount: number
  lastAccessedAt: number
  userWide?: boolean
}

const KINDS: { id: string; label: string; kinds: string[] }[] = [
  { id: 'all', label: 'All', kinds: [] },
  { id: 'facts', label: 'Facts', kinds: ['fact', 'auto'] },
  { id: 'prefs', label: 'Preferences', kinds: ['preference', 'instruction', 'correction', 'persona'] },
  { id: 'episodes', label: 'Episodes', kinds: ['episodic'] },
]

function ago(ms: number): string {
  if (!ms) return 'never'
  const s = Math.max(0, Math.floor((Date.now() - ms) / 1000))
  if (s < 3600) return `${Math.max(1, Math.floor(s / 60))}m ago`
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`
  return `${Math.floor(s / 86400)}d ago`
}

/** What the agent remembers, in the open: read it, correct it, forget it.
 * Facts about you apply to every project; the rest to this one. */
export function MemoryPanel() {
  const theme = useTheme()
  const [items, setItems] = useState<Memory[] | null>(null)
  const [query, setQuery] = useState('')
  const [kind, setKind] = useState('all')
  const [editing, setEditing] = useState<{ id: string; text: string } | null>(null)
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(async () => {
    try {
      setItems((await call<Memory[]>('cmd_memory_list')) ?? [])
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
      setItems([])
    }
  }, [])
  useEffect(() => {
    void load()
  }, [load])

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase()
    const kinds = KINDS.find((k) => k.id === kind)?.kinds ?? []
    return (items ?? []).filter((m) => (kinds.length === 0 || kinds.includes(m.kind)) && (!q || m.text.toLowerCase().includes(q) || m.key.toLowerCase().includes(q)))
  }, [items, query, kind])

  const about = shown.filter((m) => m.userWide)
  const project = shown.filter((m) => !m.userWide)

  const forget = async (m: Memory) => {
    setError(null)
    try {
      await call('cmd_memory_delete', { id: m.id, userWide: !!m.userWide })
      setItems((list) => (list ?? []).filter((x) => x.id !== m.id))
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  const saveEdit = async (m: Memory) => {
    if (!editing) return
    setError(null)
    try {
      await call('cmd_memory_edit', { id: m.id, text: editing.text, userWide: !!m.userWide })
      setEditing(null)
      await load()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  const row = (m: Memory) => {
    const isEditing = editing?.id === m.id
    return (
      <Stack key={m.id} sx={{ gap: 0.5, p: 1.25, borderRadius: '10px', bgcolor: alpha(theme.palette.text.primary, 0.035), '&:hover .mem-actions': { opacity: 1 } }}>
        {isEditing ? (
          <>
            <InputBase
              autoFocus
              multiline
              fullWidth
              value={editing.text}
              onChange={(e) => setEditing({ id: m.id, text: e.target.value })}
              sx={{ fontSize: '0.8125rem', lineHeight: 1.5, bgcolor: 'background.paper', borderRadius: '8px', px: 1, py: 0.5 }}
            />
            <Stack direction="row" sx={{ gap: 0.5, justifyContent: 'flex-end' }}>
              <ButtonBase onClick={() => setEditing(null)} sx={{ height: 24, px: 1, borderRadius: '7px', fontSize: '0.75rem', color: 'text.secondary' }}>
                Cancel
              </ButtonBase>
              <ButtonBase onClick={() => void saveEdit(m)} sx={{ height: 24, px: 1.25, borderRadius: '7px', fontSize: '0.75rem', fontWeight: 600, bgcolor: 'primary.main', color: 'primary.contrastText' }}>
                Save
              </ButtonBase>
            </Stack>
          </>
        ) : (
          <>
            <Typography sx={{ fontSize: '0.8125rem', lineHeight: 1.5, whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>{m.text}</Typography>
            <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
              <Typography sx={{ fontSize: '0.6875rem', color: 'text.disabled', flex: 1 }}>
                {m.kind} · saved {ago(m.createdAt)} · {m.accessCount > 0 ? `used ${m.accessCount}× (last ${ago(m.lastAccessedAt)})` : 'not used yet'}
                {m.confidence < 0.5 ? ' · fading' : ''}
              </Typography>
              <Stack className="mem-actions" direction="row" sx={{ gap: 0.25, opacity: 0, transition: 'opacity 0.12s' }}>
                <Tooltip title="Correct it">
                  <ButtonBase aria-label="Edit" onClick={() => setEditing({ id: m.id, text: m.text })} sx={{ width: 24, height: 24, borderRadius: '7px', color: 'text.secondary', '&:hover': { bgcolor: 'action.hover' } }}>
                    <EditLinear size={13} color="currentColor" />
                  </ButtonBase>
                </Tooltip>
                <Tooltip title="Forget it">
                  <ButtonBase aria-label="Forget" onClick={() => void forget(m)} sx={{ width: 24, height: 24, borderRadius: '7px', color: 'text.secondary', '&:hover': { bgcolor: 'action.hover', color: 'error.main' } }}>
                    <TrashLinear size={13} color="currentColor" />
                  </ButtonBase>
                </Tooltip>
              </Stack>
            </Stack>
          </>
        )}
      </Stack>
    )
  }

  const group = (title: string, hint: string, list: Memory[]) =>
    list.length > 0 && (
      <Stack sx={{ gap: 0.75 }}>
        <Box>
          <Typography sx={{ fontSize: '0.6875rem', fontWeight: 700, textTransform: 'uppercase', letterSpacing: 0.4, color: 'text.secondary' }}>
            {title} · {list.length}
          </Typography>
          <Typography sx={{ fontSize: '0.75rem', color: 'text.disabled' }}>{hint}</Typography>
        </Box>
        {list.map(row)}
      </Stack>
    )

  return (
    <Stack sx={{ height: '100%', minHeight: 0 }}>
      <Stack sx={{ gap: 1, px: 2, py: 1.25, borderBottom: 1, borderColor: 'divider', flexShrink: 0 }}>
        <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
          <BrainLinear size={15} color="currentColor" />
          <Typography sx={{ fontSize: '0.875rem', fontWeight: 600 }}>Memory</Typography>
          <Typography sx={{ fontSize: '0.75rem', color: 'text.secondary' }}>{items ? `${items.length} remembered` : ''}</Typography>
        </Stack>
        <Stack direction="row" sx={{ alignItems: 'center', gap: 1, px: 1.25, height: 32, borderRadius: '9px', bgcolor: alpha(theme.palette.text.primary, 0.05) }}>
          <SearchNormalLinear size={13} color="currentColor" />
          <InputBase fullWidth placeholder="Search what's remembered" value={query} onChange={(e) => setQuery(e.target.value)} sx={{ fontSize: '0.8125rem' }} />
          {query && (
            <ButtonBase aria-label="Clear" onClick={() => setQuery('')} sx={{ color: 'text.secondary' }}>
              <CloseLinear size={12} color="currentColor" />
            </ButtonBase>
          )}
        </Stack>
        <Stack direction="row" sx={{ gap: 0.5, flexWrap: 'wrap' }}>
          {KINDS.map((k) => (
            <ButtonBase
              key={k.id}
              aria-pressed={kind === k.id}
              onClick={() => setKind(k.id)}
              sx={{ px: 1, height: 22, borderRadius: '999px', fontSize: '0.6875rem', fontWeight: 600, border: 1, borderColor: kind === k.id ? 'primary.main' : 'divider', color: kind === k.id ? 'primary.main' : 'text.secondary' }}
            >
              {kind === k.id && <TickCircleLinear size={11} color="currentColor" />}
              {k.label}
            </ButtonBase>
          ))}
        </Stack>
      </Stack>
      <Box sx={{ flex: 1, minHeight: 0, overflowY: 'auto', p: 1.5 }}>
        <Stack sx={{ gap: 2 }}>
          {error && <Alert severity="error">{error}</Alert>}
          {items === null && <Typography sx={{ fontSize: '0.8125rem', color: 'text.secondary' }}>Loading…</Typography>}
          {items !== null && items.length === 0 && !error && (
            <Typography sx={{ fontSize: '0.8125rem', color: 'text.secondary' }}>Nothing remembered yet. Ask Vivianne to remember something, or keep working: useful facts are picked up as you go.</Typography>
          )}
          {items !== null && items.length > 0 && shown.length === 0 && <Typography sx={{ fontSize: '0.8125rem', color: 'text.secondary' }}>No matches.</Typography>}
          {group('About you', 'Applies in every project', about)}
          {group('This project', 'Only used here', project)}
        </Stack>
      </Box>
    </Stack>
  )
}
