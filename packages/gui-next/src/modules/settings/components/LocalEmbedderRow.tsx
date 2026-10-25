import { useEffect, useState } from 'react'
import { Stack, Typography, Button, CircularProgress, Alert } from '../../../ui'
import { call, on } from '../../../ipc'

/** Mirrors `ai::local_onnx::LocalModelStatus`. */
interface LocalModelStatus {
  id: 'gemma' | 'miniLm'
  name: string
  dim: number
  sizeMb: number
  installed: boolean
  active: boolean
}

interface InstallProgress {
  model: LocalModelStatus['id']
  receivedMb: number
  totalMb: number
}

const LABEL: Record<LocalModelStatus['id'], { title: string; blurb: string }> = {
  gemma: {
    title: 'EmbeddingGemma',
    blurb: "Google's best small embedding model. Finds memories by meaning far better; ~65 ms per memory on CPU.",
  },
  miniLm: {
    title: 'MiniLM',
    blurb: 'Small and quick (~5 ms per memory), weaker at matching meaning.',
  },
}

/** The id the backend's `LocalModel::parse` accepts. */
const ARG: Record<LocalModelStatus['id'], string> = { gemma: 'gemma', miniLm: 'minilm' }

/**
 * Settings → Memory: the local embedding models. Installing one makes memory
 * (and code search) match by meaning offline, with no API key; the store is
 * re-indexed with it in the background.
 */
export function LocalEmbedderRow() {
  const [models, setModels] = useState<LocalModelStatus[] | null>(null)
  const [busy, setBusy] = useState<LocalModelStatus['id'] | null>(null)
  const [progress, setProgress] = useState<InstallProgress | null>(null)
  const [error, setError] = useState<string | null>(null)

  const refresh = async () => setModels((await call<LocalModelStatus[]>('cmd_local_embedder_status')) ?? [])

  useEffect(() => {
    void refresh()
    const off = on<InstallProgress>('embedder:install', (p) => setProgress(p))
    return () => {
      void off.then((f) => f())
    }
  }, [])

  const act = async (m: LocalModelStatus, install: boolean) => {
    setBusy(m.id)
    setError(null)
    setProgress(null)
    try {
      await call(install ? 'cmd_local_embedder_install' : 'cmd_local_embedder_remove', { model: ARG[m.id] })
    } catch (e) {
      setError(String(e))
    } finally {
      setBusy(null)
      setProgress(null)
      await refresh()
    }
  }

  if (!models) return null
  const anyInstalled = models.some((m) => m.installed)

  return (
    <Stack sx={{ gap: 0.75, py: 0.5 }}>
      <Typography variant="body2" sx={{ fontWeight: 500 }}>
        Search by meaning, offline
      </Typography>
      <Typography variant="caption" color="text.secondary">
        {anyInstalled
          ? 'Memories are matched by meaning on this computer — no API key, nothing sent anywhere. The best installed model is used.'
          : 'Without a local model, memory matches by meaning only through an OpenAI key, and by keywords otherwise.'}
      </Typography>
      {models.map((m) => {
        const downloading = busy === m.id && progress?.model === m.id
        return (
          <Stack key={m.id} direction="row" sx={{ alignItems: 'center', gap: 1.5, px: 1.5, py: 1, borderRadius: 2, bgcolor: 'action.hover' }}>
            <Stack sx={{ flex: 1, minWidth: 0 }}>
              <Typography variant="body2" sx={{ fontWeight: 500 }}>
                {LABEL[m.id].title}
                {m.active ? ' · in use' : ''}
              </Typography>
              <Typography variant="caption" color="text.secondary">
                {downloading
                  ? `Downloading… ${progress.receivedMb} of ~${progress.totalMb} MB`
                  : `${LABEL[m.id].blurb} ${m.sizeMb} MB.`}
              </Typography>
            </Stack>
            <Button
              size="small"
              color={m.installed ? 'error' : 'primary'}
              disabled={busy !== null}
              onClick={() => void act(m, !m.installed)}
              startIcon={busy === m.id ? <CircularProgress size={14} /> : undefined}
              sx={{ textTransform: 'none', flexShrink: 0 }}
            >
              {m.installed ? 'Remove' : busy === m.id ? 'Installing…' : 'Install'}
            </Button>
          </Stack>
        )
      })}
      {error && (
        <Alert severity="error">
          {error}
        </Alert>
      )}
    </Stack>
  )
}
