import { useEffect, useState } from 'react'
import { Button, Stack, Switch, TextField, Typography } from '../../../ui'
import { useMemoryStore } from '../store/memoryStore'

/**
 * An optional connection to a Hindsight server for people who already keep memory there. Off by default; Vivianne's own memory never needs it.
 * The key goes to the system keychain, not to a file, and what is stored is never shown again.
 */
export function HindsightRow() {
  const config = useMemoryStore((s) => s.hindsight)
  const load = useMemoryStore((s) => s.loadHindsight)
  const save = useMemoryStore((s) => s.saveHindsight)
  const test = useMemoryStore((s) => s.testHindsight)
  const [enabled, setEnabled] = useState(false)
  const [url, setUrl] = useState('')
  const [bank, setBank] = useState('')
  const [retain, setRetain] = useState(false)
  const [key, setKey] = useState('')
  const [note, setNote] = useState<{ text: string; bad: boolean } | null>(null)
  const [busy, setBusy] = useState(false)

  useEffect(() => void load(), [load])
  useEffect(() => {
    if (!config) return
    setEnabled(config.enabled)
    setUrl(config.url)
    setBank(config.bank)
    setRetain(config.retain)
  }, [config])

  const draft = { enabled, url, bank, retain }
  const run = async (what: 'save' | 'test') => {
    setBusy(true)
    setNote(null)
    if (what === 'save') {
      const err = await save(draft, key === '' ? undefined : key)
      setNote(err ? { text: err, bad: true } : { text: 'Saved.', bad: false })
      if (!err) setKey('')
    } else {
      const r = await test(draft)
      setNote({ text: r, bad: r !== 'Connected.' })
    }
    setBusy(false)
  }

  return (
    <Stack sx={{ gap: 1 }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1.5 }}>
        <Stack sx={{ flex: 1, minWidth: 0 }}>
          <Typography variant="body2" sx={{ fontWeight: 500 }}>
            Hindsight server
          </Typography>
          <Typography variant="caption" color="text.secondary">
            Also search a Hindsight server you run when recalling. Its answers join the ranking, marked as coming from Hindsight.
          </Typography>
        </Stack>
        <Switch size="small" checked={enabled} onChange={() => setEnabled(!enabled)} />
      </Stack>
      {enabled && (
        <Stack sx={{ gap: 1 }}>
          <TextField size="small" label="Server address" placeholder="https://memory.example.com" value={url} onChange={(e) => setUrl(e.target.value)} />
          <Stack direction="row" sx={{ gap: 1 }}>
            <TextField size="small" label="Bank" value={bank} onChange={(e) => setBank(e.target.value)} sx={{ width: 180 }} />
            <TextField
              size="small"
              label="API key"
              type="password"
              placeholder={config?.hasKey ? 'Stored in the keychain. Type to replace.' : 'Optional'}
              value={key}
              onChange={(e) => setKey(e.target.value)}
              fullWidth
              autoComplete="off"
            />
          </Stack>
          <Stack direction="row" sx={{ alignItems: 'center', gap: 1.5 }}>
            <Stack sx={{ flex: 1, minWidth: 0 }}>
              <Typography variant="body2">Send what I save to the server too</Typography>
              <Typography variant="caption" color="text.secondary">
                Off: the server is only read. On: each memory you save here is also sent there, with secrets hidden.
              </Typography>
            </Stack>
            <Switch size="small" checked={retain} onChange={() => setRetain(!retain)} />
          </Stack>
        </Stack>
      )}
      <Stack direction="row" sx={{ gap: 1, alignItems: 'center' }}>
        <Button size="small" variant="contained" disabled={busy} onClick={() => void run('save')} sx={{ textTransform: 'none' }}>
          Save
        </Button>
        {enabled && (
          <Button size="small" disabled={busy} onClick={() => void run('test')} sx={{ textTransform: 'none' }}>
            Test connection
          </Button>
        )}
        {note && (
          <Typography variant="caption" color={note.bad ? 'error' : 'text.secondary'}>
            {note.text}
          </Typography>
        )}
      </Stack>
    </Stack>
  )
}
