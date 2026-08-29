import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import Divider from '@mui/material/Divider'
import Switch from '@mui/material/Switch'
import TextField from '@mui/material/TextField'
import ButtonGroup from '@mui/material/ButtonGroup'
import Button from '@mui/material/Button'
import { useVoiceStore, type TtsEngine } from '../../voice/store/voiceStore'

const ENGINES: { id: TtsEngine; label: string }[] = [
  { id: 'cartesia', label: 'Cartesia (fast, needs key)' },
  { id: 'kokoro', label: 'Kokoro (local)' },
  { id: 'browser', label: 'Browser' },
]

/** Voice mode settings — enable, TTS engine + Cartesia key, whisper model.
 * The pipeline itself lives in `modules/voice`; this is only the picker,
 * same split as `ThemesSection`/`theme.ts`. */
export function VoiceSection() {
  const enabled = useVoiceStore((s) => s.enabled)
  const ttsEngine = useVoiceStore((s) => s.ttsEngine)
  const cartesiaApiKey = useVoiceStore((s) => s.cartesiaApiKey)
  const cartesiaVoiceId = useVoiceStore((s) => s.cartesiaVoiceId)
  const whisperModel = useVoiceStore((s) => s.whisperModel)
  const setSettings = useVoiceStore((s) => s.setSettings)

  return (
    <Stack sx={{ gap: 1.5 }}>
      <Stack direction="row" sx={{ alignItems: 'center', justifyContent: 'space-between' }}>
        <Stack>
          <Typography variant="body2" sx={{ fontWeight: 600 }}>
            Voice mode
          </Typography>
          <Typography variant="caption" color="text.secondary">
            Press Ctrl+Shift+V to open a live, interruptible conversation. The notch stays hidden until then — it
            only appears for the duration of a session.
          </Typography>
        </Stack>
        <Switch checked={enabled} onChange={(e) => setSettings({ enabled: e.target.checked })} />
      </Stack>

      <Divider />

      <Typography variant="subtitle2" sx={{ fontWeight: 600 }}>
        Speech
      </Typography>
      <Typography variant="caption" color="text.secondary">
        Cartesia gives near-instant, human-like speech but needs an API key and sends reply text (not your voice) to
        their API. Kokoro runs fully offline. Either falls back to the browser's built-in voice if it fails.
      </Typography>
      <ButtonGroup size="small">
        {ENGINES.map((e) => (
          <Button
            key={e.id}
            onClick={() => setSettings({ ttsEngine: e.id })}
            sx={{
              textTransform: 'none',
              bgcolor: ttsEngine === e.id ? 'action.selected' : 'transparent',
              '&:hover': { bgcolor: ttsEngine === e.id ? 'action.selected' : 'action.hover' },
            }}
          >
            {e.label}
          </Button>
        ))}
      </ButtonGroup>

      {ttsEngine === 'cartesia' && (
        <Stack sx={{ gap: 1, mt: 0.5 }}>
          <TextField
            label="Cartesia API key"
            type="password"
            size="small"
            fullWidth
            value={cartesiaApiKey}
            onChange={(e) => setSettings({ cartesiaApiKey: e.target.value })}
          />
          <TextField
            label="Voice ID (optional)"
            size="small"
            fullWidth
            value={cartesiaVoiceId}
            onChange={(e) => setSettings({ cartesiaVoiceId: e.target.value })}
            helperText="Leave blank to use Cartesia's default voice."
          />
        </Stack>
      )}

      <Divider />

      <Typography variant="subtitle2" sx={{ fontWeight: 600 }}>
        Speech recognition
      </Typography>
      <TextField
        select
        label="Whisper model"
        size="small"
        value={whisperModel}
        onChange={(e) => setSettings({ whisperModel: e.target.value })}
        slotProps={{ select: { native: true } }}
        sx={{ maxWidth: 220 }}
      >
        {['tiny.en', 'base.en', 'small.en', 'tiny', 'base', 'small'].map((m) => (
          <option key={m} value={m}>
            {m}
          </option>
        ))}
      </TextField>
      <Typography variant="caption" color="text.secondary">
        Runs fully offline via whisper.cpp — the model downloads once, on first use. Smaller models transcribe
        faster; the `.en` variants are English-only and slightly more accurate for it.
      </Typography>
    </Stack>
  )
}
