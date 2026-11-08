import { useEffect } from 'react'
import { Box, Button, CircularProgress, Stack, Typography } from '../../../ui'
import { useImageModels } from '../../ai/store/imageModelsStore'

const gb = (bytes: number) => `${(bytes / 1e9).toFixed(1)} GB`

/** Settings → Local models → Image generation: run picture models on this
 * computer through stable-diffusion.cpp. Downloaded models then appear in the
 * model picker next to the cloud image models. */
export function ImageModelsPanel() {
  const s = useImageModels()
  useEffect(() => {
    void s.refresh()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const have = new Set(s.models.map((m) => m.file))
  return (
    <Stack sx={{ gap: 1.25 }}>
      <Typography sx={{ fontSize: '0.875rem', fontWeight: 600 }}>Image generation</Typography>
      <Typography variant="body2" color="text.secondary">
        Draw pictures on this computer with stable-diffusion.cpp. Pick a downloaded model in the model picker, then describe the picture in the chat.
      </Typography>

      <Stack direction="row" sx={{ alignItems: 'center', gap: 1.5, p: 1.25, borderRadius: '8px', border: 1, borderColor: 'divider' }}>
        <Stack sx={{ flex: 1, minWidth: 0 }}>
          <Typography sx={{ fontSize: '0.8125rem', fontWeight: 500 }}>stable-diffusion.cpp</Typography>
          <Typography noWrap sx={{ fontSize: '0.75rem', color: 'text.secondary' }}>
            {s.installing !== null ? `Downloading… ${s.installing}%` : s.installed ? (s.path ?? 'Installed') : 'Not installed. Vivianne downloads the build that fits this computer (Vulkan: NVIDIA, AMD or Intel).'}
          </Typography>
        </Stack>
        {!s.installed && (
          <Button size="small" variant="contained" disabled={s.installing !== null} onClick={() => void s.install()}>
            {s.installing !== null ? <CircularProgress size={14} /> : 'Install'}
          </Button>
        )}
      </Stack>

      {s.catalog.map((m) => {
        const done = have.has(m.file)
        const progress = s.downloading[m.file]
        return (
          <Stack key={m.id} direction="row" sx={{ alignItems: 'center', gap: 1.5, p: 1.25, borderRadius: '8px', border: 1, borderColor: 'divider' }}>
            <Stack sx={{ flex: 1, minWidth: 0 }}>
              <Typography sx={{ fontSize: '0.8125rem', fontWeight: 500 }}>
                {m.name} <Box component="span" sx={{ fontWeight: 400, color: 'text.secondary' }}>· {m.gb} GB</Box>
              </Typography>
              <Typography sx={{ fontSize: '0.75rem', color: 'text.secondary' }}>{progress !== undefined ? `Downloading… ${progress}%` : m.note}</Typography>
            </Stack>
            {done ? (
              <Button size="small" variant="outlined" onClick={() => void s.remove(m.file)}>
                Delete
              </Button>
            ) : (
              <Button size="small" variant="outlined" disabled={progress !== undefined} onClick={() => void s.download(m.id)}>
                {progress !== undefined ? <CircularProgress size={14} /> : 'Download'}
              </Button>
            )}
          </Stack>
        )
      })}

      {s.models.filter((m) => !s.catalog.some((c) => c.file === m.file)).map((m) => (
        <Stack key={m.file} direction="row" sx={{ alignItems: 'center', gap: 1.5, p: 1.25, borderRadius: '8px', border: 1, borderColor: 'divider' }}>
          <Stack sx={{ flex: 1, minWidth: 0 }}>
            <Typography noWrap sx={{ fontSize: '0.8125rem', fontWeight: 500 }}>{m.file}</Typography>
            <Typography sx={{ fontSize: '0.75rem', color: 'text.secondary' }}>{gb(m.bytes)} · your own file</Typography>
          </Stack>
          <Button size="small" variant="outlined" onClick={() => void s.remove(m.file)}>
            Delete
          </Button>
        </Stack>
      ))}
      <Typography variant="caption" color="text.secondary">
        Have a model already? Put a .safetensors, .gguf or .ckpt file in <code>~/.viv/image-models</code> and it shows up here.
      </Typography>
      {s.error && <Typography sx={{ fontSize: '0.75rem', color: 'error.main', whiteSpace: 'pre-wrap' }}>{s.error}</Typography>}
    </Stack>
  )
}
