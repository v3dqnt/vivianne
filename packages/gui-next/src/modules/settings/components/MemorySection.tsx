import { useEffect, useState } from 'react'
import { Stack, Typography, Divider, Switch, Button, TextField, CircularProgress } from '../../../ui'
import { ArchiveBookLinear, SearchNormalLinear, TrashLinear } from '../../../icons'
import { useMemoryStore } from '../store/memoryStore'

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  return `${(n / (1024 * 1024)).toFixed(1)} MB`
}

function ToggleRow({
  label,
  description,
  checked,
  disabled,
  onToggle,
}: {
  label: string
  description: string
  checked: boolean
  disabled?: boolean
  onToggle: () => void
}) {
  return (
    <Stack direction="row" sx={{ alignItems: 'center', gap: 1.5, py: 1 }}>
      <Stack sx={{ flex: 1, minWidth: 0 }}>
        <Typography variant="body2" sx={{ fontWeight: 500 }}>
          {label}
        </Typography>
        <Typography variant="caption" color="text.secondary">
          {description}
        </Typography>
      </Stack>
      <Switch size="small" checked={checked} disabled={disabled} onChange={onToggle} />
    </Stack>
  )
}

export function MemorySection() {
  const status = useMemoryStore((s) => s.status)
  const recallResults = useMemoryStore((s) => s.recallResults)
  const refresh = useMemoryStore((s) => s.refresh)
  const toggleEnabled = useMemoryStore((s) => s.toggleEnabled)
  const toggleAutoConsolidate = useMemoryStore((s) => s.toggleAutoConsolidate)
  const toggleExtraction = useMemoryStore((s) => s.toggleExtraction)
  const purge = useMemoryStore((s) => s.purge)
  const recall = useMemoryStore((s) => s.recall)
  const [query, setQuery] = useState('')
  const [confirmingPurge, setConfirmingPurge] = useState(false)
  const [purging, setPurging] = useState(false)

  useEffect(() => {
    void refresh()
  }, [refresh])

  const doPurge = async () => {
    if (!confirmingPurge) {
      setConfirmingPurge(true)
      return
    }
    setPurging(true)
    try {
      await purge()
    } finally {
      setPurging(false)
      setConfirmingPurge(false)
    }
  }

  if (!status) {
    return (
      <Stack sx={{ alignItems: 'center', py: 4 }}>
        <CircularProgress size={20} />
      </Stack>
    )
  }

  return (
    <Stack sx={{ gap: 0.5 }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1, mb: 0.5 }}>
        <ArchiveBookLinear size={16} color="currentColor" />
        <Typography variant="subtitle2" sx={{ fontWeight: 600 }}>
          Memory
        </Typography>
      </Stack>
      <Typography variant="caption" color="text.secondary" sx={{ mb: 1 }}>
        Nothing is saved by an explicit action — after a turn that did real work, it's distilled into a fact
        automatically, and relevant facts are silently added to the prompt at the start of every turn (you'll see a
        "Recalled N memories" note in chat when that happens). Search below reads the same store on demand.
      </Typography>
      <Typography variant="caption" color="text.secondary" sx={{ mb: 1 }}>
        {status.entryCount} {status.entryCount === 1 ? 'entry' : 'entries'} · ~{status.estimatedTokens} tokens ·{' '}
        {formatBytes(status.bytes)}
      </Typography>
      <Divider sx={{ mb: 0.5 }} />

      <ToggleRow
        label="Remember across sessions"
        description="Let the agent save and recall facts, preferences, and corrections between chats."
        checked={status.state.enabled}
        onToggle={() => void toggleEnabled()}
      />
      <ToggleRow
        label="Auto-consolidate"
        description="Periodically fold related entries together instead of letting them accumulate unbounded."
        checked={status.state.autoConsolidate}
        disabled={!status.state.enabled}
        onToggle={() => void toggleAutoConsolidate()}
      />
      <ToggleRow
        label="Auto-extraction"
        description="Automatically pull memorable facts out of conversations, not just explicit 'remember this' asks."
        checked={status.state.extractionEnabled}
        disabled={!status.state.enabled}
        onToggle={() => void toggleExtraction()}
      />

      <Divider sx={{ my: 1 }} />

      <Typography variant="body2" sx={{ fontWeight: 500, mb: 0.5 }}>
        Search memory
      </Typography>
      <TextField
        size="small"
        placeholder="Search what the agent remembers…"
        value={query}
        onChange={(e) => {
          setQuery(e.target.value)
          void recall(e.target.value)
        }}
        slotProps={{ input: { startAdornment: <SearchNormalLinear size={14} color="currentColor" style={{ marginRight: 6 }} /> } }}
      />
      {recallResults.length > 0 && (
        <Stack sx={{ mt: 1, gap: 0.5, maxHeight: 220, overflowY: 'auto' }}>
          {recallResults.map((entry) => (
            <Stack key={entry.id} sx={{ px: 1.5, py: 1, borderRadius: 2, bgcolor: 'action.hover' }}>
              <Typography variant="caption" color="text.secondary">
                {entry.kind}
              </Typography>
              <Typography variant="body2">{entry.text}</Typography>
            </Stack>
          ))}
        </Stack>
      )}

      <Divider sx={{ my: 1 }} />

      <Stack direction="row" sx={{ alignItems: 'center', justifyContent: 'space-between' }}>
        <Typography variant="caption" color="text.secondary">
          Permanently erase everything the agent remembers.
        </Typography>
        <Button
          size="small"
          color="error"
          onClick={() => void doPurge()}
          onBlur={() => setConfirmingPurge(false)}
          disabled={purging || status.entryCount === 0}
          startIcon={purging ? <CircularProgress size={14} /> : <TrashLinear size={14} color="currentColor" />}
          sx={{ textTransform: 'none' }}
        >
          {confirmingPurge ? 'Really purge?' : 'Purge all'}
        </Button>
      </Stack>
    </Stack>
  )
}
