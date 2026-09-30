import { useCallback, useEffect, useState } from 'react'
import { Stack, Typography, TextField, MenuItem, Divider, Alert, Chip } from '../../../ui'
import { call } from '../../../ipc'

/**
 * Speech models offered, smallest first.
 *
 * Only the English-only builds: Viv Instant's whole input is short imperative
 * English full of application names, and the multilingual models are both
 * slower and worse at exactly that. They stay reachable by hand in
 * `settings.json` for anyone who needs one.
 */
const MODELS = [
  { id: 'tiny.en', label: 'Tiny — fastest, least accurate', size: '~75 MB' },
  { id: 'base.en', label: 'Base — recommended', size: '~142 MB' },
  { id: 'small.en', label: 'Small — most accurate, slowest', size: '~466 MB' },
]

interface InstantSettings {
  instantSttEngine?: string
  instantSttModel?: string
  jevApiKey?: string
}

/**
 * Viv Instant's settings.
 *
 * Replaces the old Voice section, which was deleted with the voice module.
 * Three things live here and nothing else: the chord that wakes it, the speech
 * model it listens with, and the optional Jev key.
 */
export function InstantSection() {
  const [settings, setSettings] = useState<Record<string, unknown> | null>(null)
  const [engine, setEngine] = useState('parakeet')
  const [installed, setInstalled] = useState<boolean | null>(null)
  const [model, setModel] = useState('base.en')
  const [jevKey, setJevKey] = useState('')
  const [shortcut, setShortcut] = useState('')
  const [shortcutError, setShortcutError] = useState<string | null>(null)

  useEffect(() => {
    void (async () => {
      const [cfg, label, err, have] = await Promise.all([
        call<Record<string, unknown> & InstantSettings>('cmd_get_settings'),
        call<string>('cmd_instant_shortcut_label'),
        call<string | null>('cmd_instant_shortcut_error'),
        call<boolean>('cmd_instant_stt_installed').catch(() => null),
      ])
      if (cfg) {
        setSettings(cfg)
        setEngine(cfg.instantSttEngine ?? 'parakeet')
        setModel(cfg.instantSttModel ?? 'base.en')
        setJevKey(cfg.jevApiKey ?? '')
      }
      setInstalled(have)
      if (label) setShortcut(label)
      setShortcutError(err ?? null)
    })()
  }, [])

  const save = useCallback(
    async (next: InstantSettings) => {
      await call('cmd_save_settings', { settings: { ...(settings ?? {}), ...next } })
      setSettings((prev) => ({ ...(prev ?? {}), ...next }))
    },
    [settings],
  )

  return (
    <Stack sx={{ gap: 2.5 }}>
      <Stack sx={{ gap: 0.5 }}>
        <Typography variant="body2" sx={{ fontWeight: 600 }}>
          Viv Instant
        </Typography>
        <Typography variant="caption" color="text.secondary">
          Hold the chord from anywhere, say what you want done, and let go: it operates your computer.
          It runs without asking for approval on each step — the overlay shows a live log of
          everything it does, and anything that could destroy your work is held for three seconds
          first.
        </Typography>
      </Stack>

      <Divider />

      <Stack sx={{ gap: 0.75 }}>
        <Typography variant="body2" sx={{ fontWeight: 600 }}>
          Wake chord
        </Typography>
        <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
          <Chip
            size="small"
            label={shortcut || '…'}
            sx={{ fontFamily: '"Geist Mono", ui-monospace, monospace', fontWeight: 600 }}
          />
          <Typography variant="caption" color="text.secondary">
            Press again to dismiss. Escape stops a run in progress.
          </Typography>
        </Stack>
        {/* A chord another application already owns fails silently at the OS
            level, so the failure has to be surfaced somewhere the user will
            look — otherwise the feature simply appears not to exist. */}
        {shortcutError && (
          <Alert severity="warning" sx={{ mt: 0.5, py: 0.25 }}>
            <Typography variant="caption">
              Couldn’t register the chord — another app is probably using it. {shortcutError}
            </Typography>
          </Alert>
        )}
      </Stack>

      <Stack sx={{ gap: 0.75 }}>
        <Typography variant="body2" sx={{ fontWeight: 600 }}>
          Speech recognition
        </Typography>
        <TextField
          select
          size="small"
          value={engine}
          onChange={(e) => {
            setEngine(e.target.value)
            void save({ instantSttEngine: e.target.value })
          }}
          sx={{ maxWidth: 360 }}
        >
          <MenuItem value="parakeet">Parakeet TDT 0.6B — most accurate (~628 MB)</MenuItem>
          <MenuItem value="whisper">Whisper — small and fast</MenuItem>
        </TextField>
        {/* Said before the download rather than after: at ~628 MB the first
            summon otherwise looks like the feature is broken for several
            minutes. */}
        {engine === 'parakeet' && installed === false && (
          <Alert severity="info" sx={{ py: 0.25 }}>
            <Typography variant="caption">
              Not downloaded yet — the first time you open Viv Instant it will fetch ~628 MB. It is
              released from memory again whenever Viv Instant closes.
            </Typography>
          </Alert>
        )}
        <Typography variant="caption" color="text.secondary">
          Parakeet is markedly better at the thing that matters here: application and menu names.
          Whisper is a tenth of the size and a reasonable choice if disk or memory is tight.
        </Typography>
      </Stack>

      {engine === 'whisper' && (
      <Stack sx={{ gap: 0.75 }}>
        <Typography variant="body2" sx={{ fontWeight: 600 }}>
          Whisper model
        </Typography>
        <TextField
          select
          size="small"
          value={model}
          onChange={(e) => {
            setModel(e.target.value)
            void save({ instantSttModel: e.target.value })
          }}
          sx={{ maxWidth: 360 }}
        >
          {MODELS.map((m) => (
            <MenuItem key={m.id} value={m.id}>
              {m.label} ({m.size})
            </MenuItem>
          ))}
        </TextField>
        <Typography variant="caption" color="text.secondary">
          Downloaded on first use. Accuracy matters more here than in dictation: a misheard
          application name sends the whole sequence to the wrong window.
        </Typography>
      </Stack>
      )}

      <Divider />

      <Stack sx={{ gap: 0.75 }}>
        <Typography variant="body2" sx={{ fontWeight: 600 }}>
          Jev API key <Typography component="span" variant="caption" color="text.secondary">— optional</Typography>
        </Typography>
        <TextField
          size="small"
          type="password"
          placeholder="Leave empty unless you have early access"
          value={jevKey}
          onChange={(e) => setJevKey(e.target.value)}
          onBlur={() => void save({ jevApiKey: jevKey.trim() })}
          sx={{ maxWidth: 360 }}
        />
        {/* Deliberately plain about this. Jev is waitlisted, so most people
            will never have a key, and a setting that reads like a requirement
            makes a complete feature look broken. */}
        <Typography variant="caption" color="text.secondary">
          TypeSafe AI’s Jev makes fast yes/no judgements — is that step risky, did it work, have
          you finished speaking — that would otherwise cost a full model round trip. Everything
          works without it; with a key, Viv Instant is quicker to react and catches a few more
          risky steps. It never drives your computer — that is always the model you picked for
          chat.
        </Typography>
      </Stack>
    </Stack>
  )
}
