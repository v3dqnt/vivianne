import { useEffect, useState } from 'react'
import { Button, IconButton, Stack, TextField, Typography } from '../../../ui'
import { AddLinear, CloseLinear } from '../../../icons'
import { useMemoryStore } from '../store/memoryStore'

interface Draft {
  name: string
  question: string
}

const MAX_PAGES = 12

/** The project's pages: a few named questions whose answers the agent keeps up to date from what it learns. */
export function MemoryPagesRow() {
  const saved = useMemoryStore((s) => s.status?.state.pages) ?? []
  const enabled = useMemoryStore((s) => s.status?.state.enabled) ?? false
  const setPages = useMemoryStore((s) => s.setPages)
  const [drafts, setDrafts] = useState<Draft[]>(saved)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  // Follow what is saved when it changes underneath (another window, a purge).
  useEffect(() => setDrafts(saved), [JSON.stringify(saved)]) // eslint-disable-line react-hooks/exhaustive-deps
  const dirty = JSON.stringify(drafts) !== JSON.stringify(saved)

  const edit = (i: number, patch: Partial<Draft>) => setDrafts((d) => d.map((p, j) => (j === i ? { ...p, ...patch } : p)))
  const save = async () => {
    setBusy(true)
    setError(await setPages(drafts.filter((d) => d.name.trim() || d.question.trim())))
    setBusy(false)
  }

  return (
    <Stack sx={{ gap: 1 }}>
      <Stack>
        <Typography variant="body2" sx={{ fontWeight: 500 }}>
          Pages
        </Typography>
        <Typography variant="caption" color="text.secondary">
          Questions you want answered at a glance, like &ldquo;How do we release?&rdquo;. While the app is idle, the agent keeps each page up to
          date from what it has learned. Only the names are always in view; the agent finds a page&rsquo;s content when it needs it.
        </Typography>
      </Stack>
      {drafts.map((d, i) => (
        <Stack key={i} direction="row" sx={{ gap: 1, alignItems: 'flex-start' }}>
          <TextField size="small" placeholder="Name" value={d.name} onChange={(e) => edit(i, { name: e.target.value })} sx={{ width: 150, flexShrink: 0 }} slotProps={{ htmlInput: { maxLength: 60 } }} />
          <TextField size="small" placeholder="The question this page answers" value={d.question} onChange={(e) => edit(i, { question: e.target.value })} fullWidth slotProps={{ htmlInput: { maxLength: 300 } }} />
          <IconButton size="small" aria-label="Remove page" onClick={() => setDrafts((all) => all.filter((_, j) => j !== i))}>
            <CloseLinear size={14} color="currentColor" />
          </IconButton>
        </Stack>
      ))}
      <Stack direction="row" sx={{ gap: 1, alignItems: 'center' }}>
        <Button size="small" disabled={!enabled || drafts.length >= MAX_PAGES} startIcon={<AddLinear size={14} color="currentColor" />} onClick={() => setDrafts((d) => [...d, { name: '', question: '' }])} sx={{ textTransform: 'none' }}>
          Add a page
        </Button>
        {dirty && (
          <Button size="small" variant="contained" disabled={busy} onClick={() => void save()} sx={{ textTransform: 'none' }}>
            Save pages
          </Button>
        )}
        {error && (
          <Typography variant="caption" color="error">
            {error}
          </Typography>
        )}
      </Stack>
    </Stack>
  )
}
