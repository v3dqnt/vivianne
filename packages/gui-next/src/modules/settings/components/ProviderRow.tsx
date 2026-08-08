import { useState } from 'react'
import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import InputBase from '@mui/material/InputBase'
import IconButton from '@mui/material/IconButton'
import CircularProgress from '@mui/material/CircularProgress'
import { TickCircleLinear, TrashLinear, KeyLinear } from 'mx-icons'
import type { ProviderMeta } from '../lib/providers'
import { useAuthStore } from '../store/authStore'

export function ProviderRow({ provider }: { provider: ProviderMeta }) {
  const connected = useAuthStore((s) => s.status[provider.id] ?? false)
  const saveKey = useAuthStore((s) => s.saveKey)
  const deleteKey = useAuthStore((s) => s.deleteKey)
  const [draft, setDraft] = useState('')
  const [busy, setBusy] = useState(false)

  const save = async () => {
    if (!draft.trim()) return
    setBusy(true)
    try {
      await saveKey(provider.id, draft.trim())
      setDraft('')
    } finally {
      setBusy(false)
    }
  }

  const remove = async () => {
    setBusy(true)
    try {
      await deleteKey(provider.id)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Stack
      direction="row"
      sx={{ alignItems: 'center', gap: 1.5, py: 1.25, px: 1.5, borderRadius: 2, '&:hover': { bgcolor: 'action.hover' } }}
    >
      <Box sx={{ width: 34, display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'text.secondary' }}>
        <KeyLinear size={16} color="currentColor" />
      </Box>

      <Box sx={{ width: 160, flexShrink: 0 }}>
        <Typography variant="body2" sx={{ fontWeight: 500 }}>
          {provider.label}
        </Typography>
        <Typography variant="caption" color={connected ? 'success.main' : 'text.secondary'}>
          {connected ? 'Connected' : 'Not connected'}
        </Typography>
      </Box>

      {connected ? (
        <>
          <Box sx={{ flex: 1 }} />
          <IconButton size="small" onClick={() => void remove()} disabled={busy}>
            {busy ? <CircularProgress size={14} /> : <TrashLinear size={15} color="currentColor" />}
          </IconButton>
        </>
      ) : (
        <>
          <InputBase
            fullWidth
            type="password"
            placeholder="Paste API key…"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && void save()}
            sx={{
              fontSize: '0.8125rem',
              fontFamily: 'monospace',
              bgcolor: 'rgba(255,255,255,0.05)',
              borderRadius: 999,
              px: 1.5,
              py: 0.5,
            }}
          />
          <IconButton size="small" onClick={() => void save()} disabled={busy || !draft.trim()}>
            {busy ? <CircularProgress size={14} /> : <TickCircleLinear size={16} color="currentColor" />}
          </IconButton>
        </>
      )}
    </Stack>
  )
}
