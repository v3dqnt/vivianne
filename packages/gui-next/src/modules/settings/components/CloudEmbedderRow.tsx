import { useCallback, useEffect, useState } from 'react'
import { Alert, Button, CircularProgress, InputBase, MenuItem, Select, Stack, ToggleButton, ToggleButtonGroup, Typography } from '../../../ui'
import { call } from '../../../ipc'

type Provider = 'openai' | 'nebius' | 'nvidia-nim'
interface Config {
  prefer: 'local' | 'cloud'
  provider: Provider
  model: string
}
interface TestResult {
  model: string
  dimensions: number
  ms: number
  similar: number
  unrelated: number
  ranksCorrectly: boolean
}

const PROVIDERS: { id: Provider; label: string; hint: string }[] = [
  { id: 'openai', label: 'OpenAI', hint: 'text-embedding-3-small. The default when no local model is installed.' },
  { id: 'nebius', label: 'Nebius Token Factory', hint: 'Qwen3-Embedding-8B, 4096 dimensions. Uses your Nebius key.' },
  { id: 'nvidia-nim', label: 'NVIDIA (hosted)', hint: "NVIDIA's retrieval models, Nemotron embed among them. Uses your NVIDIA key." },
]

const DEFAULT: Config = { prefer: 'local', provider: 'openai', model: 'text-embedding-3-small' }

/**
 * Settings → Memory: embed memory with a hosted model (Nebius, NVIDIA or
 * OpenAI) instead of, or when there is no, local one. Switching re-embeds
 * memory in the background; keyword search keeps working meanwhile.
 */
export function CloudEmbedderRow() {
  const [cfg, setCfg] = useState<Config>(DEFAULT)
  const [models, setModels] = useState<string[]>([])
  const [loadingModels, setLoadingModels] = useState(false)
  const [testing, setTesting] = useState(false)
  const [result, setResult] = useState<TestResult | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [saved, setSaved] = useState(false)

  useEffect(() => {
    void call<Record<string, unknown>>('cmd_get_settings').then((s) => {
      const c = s?.memoryEmbedder as Partial<Config> | undefined
      if (c?.provider && c.model) setCfg({ prefer: c.prefer === 'cloud' ? 'cloud' : 'local', provider: c.provider, model: c.model })
    })
  }, [])

  const loadModels = useCallback(async (provider: Provider) => {
    setLoadingModels(true)
    try {
      setModels((await call<string[]>('cmd_list_embedding_models', { provider })) ?? [])
    } catch {
      setModels([])
    } finally {
      setLoadingModels(false)
    }
  }, [])
  useEffect(() => {
    void loadModels(cfg.provider)
  }, [cfg.provider, loadModels])

  /** Save the choice and apply it now. */
  const save = async (next: Config) => {
    setCfg(next)
    setResult(null)
    setError(null)
    const st = (await call<Record<string, unknown>>('cmd_get_settings')) ?? {}
    await call('cmd_save_settings', { settings: { ...st, memoryEmbedder: next } })
    await call('cmd_embedder_refresh')
    setSaved(true)
    window.setTimeout(() => setSaved(false), 1500)
  }

  const test = async () => {
    setTesting(true)
    setResult(null)
    setError(null)
    try {
      setResult((await call<TestResult>('cmd_test_embedder')) ?? null)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setTesting(false)
    }
  }

  const provider = PROVIDERS.find((p) => p.id === cfg.provider)
  return (
    <Stack sx={{ gap: 1, py: 0.5 }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1.5 }}>
        <Stack sx={{ flex: 1, minWidth: 0 }}>
          <Typography variant="body2" sx={{ fontWeight: 500 }}>
            Cloud embedder
          </Typography>
          <Typography variant="caption" color="text.secondary">
            Embed memory with a hosted model. Changing it re-embeds memory in the background.
          </Typography>
        </Stack>
        <ToggleButtonGroup size="small" exclusive value={cfg.prefer} onChange={(_: unknown, v: Config['prefer'] | null) => v && void save({ ...cfg, prefer: v })}>
          <ToggleButton value="local" sx={{ textTransform: 'none', px: 1.25 }}>
            Local first
          </ToggleButton>
          <ToggleButton value="cloud" sx={{ textTransform: 'none', px: 1.25 }}>
            Use cloud
          </ToggleButton>
        </ToggleButtonGroup>
      </Stack>

      <Stack direction="row" sx={{ gap: 1, alignItems: 'center', flexWrap: 'wrap' }}>
        <Select
          size="small"
          value={cfg.provider}
          onChange={(e) => {
            const p = e.target.value as Provider
            void save({ ...cfg, provider: p, model: p === 'nebius' ? 'Qwen/Qwen3-Embedding-8B' : p === 'nvidia-nim' ? 'nvidia/nv-embedqa-e5-v5' : 'text-embedding-3-small' })
          }}
          sx={{ minWidth: 190, fontSize: '0.8125rem' }}
        >
          {PROVIDERS.map((p) => (
            <MenuItem key={p.id} value={p.id}>
              {p.label}
            </MenuItem>
          ))}
        </Select>
        <Select size="small" value={models.includes(cfg.model) ? cfg.model : ''} onChange={(e) => e.target.value && void save({ ...cfg, model: String(e.target.value) })} sx={{ minWidth: 240, fontSize: '0.8125rem' }}>
          {models.map((m) => (
            <MenuItem key={m} value={m} sx={{ fontSize: '0.8125rem' }}>
              {m}
            </MenuItem>
          ))}
          {!models.includes(cfg.model) && (
            <MenuItem value="" disabled>
              {loadingModels ? 'Loading…' : cfg.model || 'Pick a model'}
            </MenuItem>
          )}
        </Select>
        <InputBase
          placeholder="or type a model id"
          value={models.includes(cfg.model) ? '' : cfg.model}
          onChange={(e) => setCfg({ ...cfg, model: e.target.value })}
          onBlur={() => cfg.model.trim() && void save({ ...cfg, model: cfg.model.trim() })}
          sx={{ fontSize: '0.8125rem', px: 1.25, height: 32, border: 1, borderColor: 'divider', borderRadius: '8px', minWidth: 200 }}
        />
        <Button size="small" variant="outlined" disabled={testing} onClick={() => void test()} startIcon={testing ? <CircularProgress size={13} /> : undefined} sx={{ textTransform: 'none' }}>
          {testing ? 'Testing…' : 'Test'}
        </Button>
        {saved && (
          <Typography variant="caption" color="success.main">
            Saved
          </Typography>
        )}
      </Stack>
      {provider && (
        <Typography variant="caption" color="text.secondary">
          {provider.hint}
        </Typography>
      )}
      {result && (
        <Alert severity={result.ranksCorrectly ? 'success' : 'warning'}>
          {result.model}: {result.dimensions} dimensions in {result.ms} ms. A paraphrase scored {result.similar.toFixed(2)} against {result.unrelated.toFixed(2)} for an unrelated
          sentence{result.ranksCorrectly ? '.' : ', which is the wrong way round: check the model id.'}
        </Alert>
      )}
      {error && <Alert severity="error">{error}</Alert>}
    </Stack>
  )
}
