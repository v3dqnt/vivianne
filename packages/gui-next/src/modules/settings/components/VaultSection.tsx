import { useCallback, useEffect, useState } from 'react'
import { Alert, Box, Button, ButtonBase, Dialog, DialogActions, DialogContent, DialogTitle, InputBase, Stack, ToggleButton, ToggleButtonGroup, Typography, alpha, useTheme } from '../../../ui'
import { LockLinear, TrashLinear } from '../../../icons'
import { call } from '../../../ipc'

type Kind = 'password' | 'card' | 'note'
interface Entry {
  id: string
  label: string
  kind: Kind
  site: string | null
  username: string | null
  hint: string | null
  fields: string[]
  lastUsed: number | null
}

const FIELDS: Record<Kind, { key: string; label: string; secret: boolean }[]> = {
  password: [{ key: 'password', label: 'Password', secret: true }],
  card: [
    { key: 'name', label: 'Name on card', secret: false },
    { key: 'number', label: 'Card number', secret: true },
    { key: 'expiry', label: 'Expiry (MM/YY)', secret: true },
    { key: 'cvv', label: 'Security code', secret: true },
  ],
  note: [{ key: 'value', label: 'Secret', secret: true }],
}
const KIND_LABEL: Record<Kind, string> = { password: 'Login', card: 'Card', note: 'Secret' }

const input = { fontSize: '0.8125rem', px: 1.25, height: 36, border: 1, borderColor: 'divider', borderRadius: '9px' } as const

function ago(ms: number | null): string {
  if (!ms) return 'never used'
  const d = Math.max(0, Date.now() - ms)
  const m = Math.round(d / 60000)
  if (m < 1) return 'used just now'
  if (m < 60) return `used ${m} min ago`
  if (m < 1440) return `used ${Math.round(m / 60)} h ago`
  return `used ${Math.round(m / 1440)} d ago`
}

/**
 * The vault: passwords, cards and secrets the agent can ask to use but never
 * read. Values are stored in the operating system's keychain, and this screen
 * can save, replace and delete them but has no way to show one again.
 */
export function VaultSection() {
  const theme = useTheme()
  const [entries, setEntries] = useState<Entry[]>([])
  const [editing, setEditing] = useState<{ entry: Entry | null } | null>(null)
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(async () => setEntries((await call<Entry[]>('cmd_vault_list')) ?? []), [])
  useEffect(() => {
    void load()
  }, [load])

  const remove = async (e: Entry) => {
    if (!window.confirm(`Delete "${e.label}" from the vault? This cannot be undone.`)) return
    try {
      await call('cmd_vault_delete', { id: e.id })
      await load()
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    }
  }

  return (
    <Stack sx={{ gap: 2, maxWidth: 560 }}>
      <Stack sx={{ gap: 0.5 }}>
        <Typography variant="subtitle1" sx={{ fontWeight: 700 }}>
          Vault
        </Typography>
        <Typography variant="body2" color="text.secondary" sx={{ lineHeight: 1.55 }}>
          Passwords and payment details the agent can use but never see. It asks to fill one into a field of your browser, you approve each
          time, and the value goes from your computer's keychain straight into the page. It never appears in a chat, a log or a screenshot.
        </Typography>
      </Stack>

      <Stack sx={{ gap: 0.5, p: 1.5, borderRadius: '10px', bgcolor: alpha(theme.palette.text.primary, 0.04) }}>
        <Typography variant="body2">What the agent can see: the label, the site, the account name and a hint like “ending 4242”.</Typography>
        <Typography variant="body2" color="text.secondary">
          What it never sees: the password, card number, expiry or security code. It is refused on pages that aren't secure, and on any
          site other than the one you set.
        </Typography>
      </Stack>

      {error && <Alert severity="error">{error}</Alert>}

      <Stack sx={{ gap: 0.75 }}>
        {entries.length === 0 && (
          <Typography variant="body2" color="text.secondary">
            Nothing stored yet.
          </Typography>
        )}
        {entries.map((e) => (
          <Stack key={e.id} direction="row" sx={{ alignItems: 'center', gap: 1.25, px: 1.5, py: 1, border: 1, borderColor: 'divider', borderRadius: '10px' }}>
            <LockLinear size={15} color="currentColor" />
            <Stack sx={{ flex: 1, minWidth: 0 }}>
              <Typography variant="body2" sx={{ fontWeight: 600 }} noWrap>
                {e.label}
              </Typography>
              <Typography variant="caption" color="text.secondary" noWrap>
                {[KIND_LABEL[e.kind], e.site, e.username, e.hint, ago(e.lastUsed)].filter(Boolean).join(' · ')}
              </Typography>
            </Stack>
            <Button size="small" onClick={() => setEditing({ entry: e })} sx={{ textTransform: 'none' }}>
              Edit
            </Button>
            <ButtonBase onClick={() => void remove(e)} sx={{ p: 0.75, borderRadius: '8px', color: 'text.secondary', '&:hover': { color: 'error.main', bgcolor: 'action.hover' } }} aria-label={`Delete ${e.label}`}>
              <TrashLinear size={15} color="currentColor" />
            </ButtonBase>
          </Stack>
        ))}
        <Button variant="outlined" size="small" onClick={() => setEditing({ entry: null })} sx={{ alignSelf: 'flex-start', textTransform: 'none', borderRadius: '8px' }}>
          Add to the vault
        </Button>
      </Stack>

      {editing && <EntryDialog entry={editing.entry} onClose={() => setEditing(null)} onSaved={() => { setEditing(null); void load() }} />}
    </Stack>
  )
}

function EntryDialog({ entry, onClose, onSaved }: { entry: Entry | null; onClose: () => void; onSaved: () => void }) {
  const [kind, setKind] = useState<Kind>(entry?.kind ?? 'password')
  const [label, setLabel] = useState(entry?.label ?? '')
  const [site, setSite] = useState(entry?.site ?? '')
  const [username, setUsername] = useState(entry?.username ?? '')
  const [values, setValues] = useState<Record<string, string>>({})
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const save = async () => {
    setSaving(true)
    setError(null)
    try {
      await call('cmd_vault_save', { entry: { id: entry?.id ?? null, label, kind, site: site || null, username: username || null, values } })
      onSaved()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setSaving(false)
    }
  }

  return (
    <Dialog open onClose={onClose} maxWidth="xs" fullWidth>
      <DialogTitle>{entry ? 'Edit vault entry' : 'Add to the vault'}</DialogTitle>
      <DialogContent>
        <Stack sx={{ gap: 1.5, pt: 0.5 }}>
          {!entry && (
            <ToggleButtonGroup size="small" exclusive value={kind} onChange={(_: unknown, v: Kind | null) => v && setKind(v)}>
              <ToggleButton value="password" sx={{ textTransform: 'none', px: 1.5 }}>Login</ToggleButton>
              <ToggleButton value="card" sx={{ textTransform: 'none', px: 1.5 }}>Card</ToggleButton>
              <ToggleButton value="note" sx={{ textTransform: 'none', px: 1.5 }}>Secret</ToggleButton>
            </ToggleButtonGroup>
          )}
          <Field label="Label (what the agent sees)">
            <InputBase fullWidth value={label} onChange={(e) => setLabel(e.target.value)} placeholder={kind === 'card' ? 'Visa personal' : 'GitHub'} sx={input} />
          </Field>
          {kind !== 'note' && (
            <Field label={kind === 'card' ? 'Only use on (optional)' : 'Only use on this site'}>
              <InputBase fullWidth value={site} onChange={(e) => setSite(e.target.value)} placeholder="github.com" sx={input} />
            </Field>
          )}
          {kind === 'password' && (
            <Field label="Account name (the agent can see this)">
              <InputBase fullWidth value={username} onChange={(e) => setUsername(e.target.value)} placeholder="me@example.com" sx={input} />
            </Field>
          )}
          {FIELDS[kind].map((f) => (
            <Field key={f.key} label={f.label}>
              <InputBase
                fullWidth
                type={f.secret ? 'password' : 'text'}
                autoComplete="off"
                value={values[f.key] ?? ''}
                onChange={(e) => setValues((v) => ({ ...v, [f.key]: e.target.value }))}
                placeholder={entry?.fields.includes(f.key) ? 'Saved. Type to replace it' : ''}
                sx={input}
              />
            </Field>
          ))}
          <Typography variant="caption" color="text.secondary">
            Stored in your computer's keychain. Once saved it can be replaced or deleted, but not shown again.
          </Typography>
          {error && <Alert severity="error">{error}</Alert>}
        </Stack>
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose} color="inherit" sx={{ textTransform: 'none' }}>
          Cancel
        </Button>
        <Button variant="contained" onClick={() => void save()} disabled={saving || !label.trim()} sx={{ textTransform: 'none' }}>
          Save
        </Button>
      </DialogActions>
    </Dialog>
  )
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <Box>
      <Typography variant="caption" color="text.secondary" sx={{ display: 'block', mb: 0.5 }}>
        {label}
      </Typography>
      {children}
    </Box>
  )
}
