import { useEffect, useMemo, useState } from 'react'
import { Alert, Box, Button, ButtonBase, CircularProgress, InputBase, LinearProgress, Stack, Typography, alpha, useTheme } from '../../../ui'
import { SearchNormalLinear, DownloadLinear, ComputerLinear, CloseLinear } from '../../../icons'
import { call, on } from '../../../ipc'
import { useLlamaStore } from '../store/llamaStore'
import { useCustomProviderStore } from '../store/customProviderStore'

/** Mirrors `local_models::HardwareInfo`. */
interface Hardware {
  ramGb: number
  cpuThreads: number
  gpus: { name: string; vramGb: number }[]
  unified: boolean
  os: string
}
type Fit = 'well' | 'slow' | 'no'

/** Same rule as `local_models::fit`, so the labels match what runs. */
function fitOf(sizeGb: number, hw: Hardware | null): Fit | null {
  if (!hw) return null
  const need = sizeGb * 1.2
  const vram = Math.max(0, ...hw.gpus.map((g) => g.vramGb))
  const gpu = hw.unified ? hw.ramGb * 0.7 : vram
  return need <= gpu ? 'well' : need <= hw.ramGb * 0.7 ? 'slow' : 'no'
}
const FIT_LABEL: Record<Fit, string> = { well: 'Runs well', slow: 'Runs, slowly', no: 'Too big' }

/**
 * Open models worth starting with: official or well-kept GGUF builds. Each
 * names its repo and the quantization to take; the exact file and size come
 * from Hugging Face when it's downloaded. `gb` is an estimate for sorting
 * and for "runs well" before that.
 */
const STARTERS: { family: string; blurb: string; variants: { label: string; repo: string; quant: string; gb: number }[] }[] = [
  {
    family: 'Qwen3',
    blurb: 'Strong all-rounder; thinks before answering',
    variants: [
      { label: '0.6B', repo: 'Qwen/Qwen3-0.6B-GGUF', quant: 'Q8_0', gb: 0.64 },
      { label: '1.7B', repo: 'Qwen/Qwen3-1.7B-GGUF', quant: 'Q8_0', gb: 1.8 },
      { label: '4B', repo: 'Qwen/Qwen3-4B-GGUF', quant: 'Q4_K_M', gb: 2.5 },
      { label: '8B', repo: 'Qwen/Qwen3-8B-GGUF', quant: 'Q4_K_M', gb: 5.0 },
      { label: '14B', repo: 'Qwen/Qwen3-14B-GGUF', quant: 'Q4_K_M', gb: 9.0 },
      { label: '32B', repo: 'Qwen/Qwen3-32B-GGUF', quant: 'Q4_K_M', gb: 19.8 },
    ],
  },
  {
    family: 'Gemma 3',
    blurb: "Google's open model; sees images",
    variants: [
      { label: '4B', repo: 'ggml-org/gemma-3-4b-it-GGUF', quant: 'Q4_K_M', gb: 2.5 },
      { label: '12B', repo: 'ggml-org/gemma-3-12b-it-GGUF', quant: 'Q4_K_M', gb: 7.3 },
      { label: '27B', repo: 'ggml-org/gemma-3-27b-it-GGUF', quant: 'Q4_K_M', gb: 16.5 },
    ],
  },
  {
    family: 'gpt-oss',
    blurb: "OpenAI's open-weight reasoning model",
    variants: [{ label: '20B', repo: 'ggml-org/gpt-oss-20b-GGUF', quant: 'mxfp4', gb: 12.1 }],
  },
  {
    family: 'Llama',
    blurb: "Meta's widely used open models",
    variants: [
      { label: '3.2 3B', repo: 'bartowski/Llama-3.2-3B-Instruct-GGUF', quant: 'Q4_K_M', gb: 2.0 },
      { label: '3.1 8B', repo: 'bartowski/Meta-Llama-3.1-8B-Instruct-GGUF', quant: 'Q4_K_M', gb: 4.9 },
    ],
  },
  {
    family: 'Phi-4 mini',
    blurb: "Microsoft's small, quick model",
    variants: [{ label: '3.8B', repo: 'bartowski/microsoft_Phi-4-mini-instruct-GGUF', quant: 'Q4_K_M', gb: 2.5 }],
  },
  {
    family: 'Mistral Nemo',
    blurb: 'Good at writing and many languages',
    variants: [{ label: '12B', repo: 'bartowski/Mistral-Nemo-Instruct-2407-GGUF', quant: 'Q4_K_M', gb: 7.5 }],
  },
]

interface HfFile {
  path: string
  size: number
}

/** A repo's single-file GGUFs (split ones need every part; skipped). */
const treeCache = new Map<string, Promise<HfFile[]>>()
function ggufFiles(repo: string): Promise<HfFile[]> {
  let p = treeCache.get(repo)
  if (!p) {
    p = fetchGgufFiles(repo)
    treeCache.set(repo, p)
    p.catch(() => treeCache.delete(repo))
  }
  return p
}

async function fetchGgufFiles(repo: string): Promise<HfFile[]> {
  const r = await fetch(`https://huggingface.co/api/models/${repo}/tree/main`)
  if (!r.ok) throw new Error(`Hugging Face answered ${r.status} for ${repo}`)
  const tree = (await r.json()) as { type: string; path: string; size: number; lfs?: { size: number } }[]
  return tree
    .filter((f) => f.type === 'file' && f.path.toLowerCase().endsWith('.gguf') && !/-\d{5}-of-\d{5}\.gguf$/i.test(f.path) && !/mmproj/i.test(f.path))
    .map((f) => ({ path: f.path, size: f.lfs?.size ?? f.size }))
    .sort((a, b) => a.size - b.size)
}

const gb = (bytes: number) => bytes / 1_073_741_824
const fmtGb = (g: number) => (g < 1 ? `${Math.round(g * 1024)} MB` : `${g.toFixed(1)} GB`)

/** Downloads in flight, by file name, fed by `local-models:progress`. */
function useDownloads() {
  const [progress, setProgress] = useState<Record<string, { received: number; total: number }>>({})
  useEffect(() => {
    const off = on<{ file: string; received: number; total: number }>('local-models:progress', (p) =>
      setProgress((s) => ({ ...s, [p.file]: { received: p.received, total: p.total } })),
    )
    return () => void off.then((f) => f())
  }, [])
  return progress
}

function FitTag({ fit }: { fit: Fit | null }) {
  if (!fit) return null
  const color = fit === 'well' ? 'success.main' : fit === 'slow' ? 'warning.main' : 'text.disabled'
  return (
    <Box component="span" sx={{ fontSize: '0.6875rem', fontWeight: 600, color, whiteSpace: 'nowrap' }}>
      {FIT_LABEL[fit]}
    </Box>
  )
}

export function DiscoverModels() {
  const theme = useTheme()
  const [hw, setHw] = useState<Hardware | null>(null)
  const [server, setServer] = useState<string | null | undefined>(undefined)
  const [installing, setInstalling] = useState<number | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState<Record<string, boolean>>({})
  const [done, setDone] = useState<Record<string, string>>({})
  /** What each button's download saves as, for its progress. */
  const [fileFor, setFileFor] = useState<Record<string, string>>({})
  const [query, setQuery] = useState('')
  const [results, setResults] = useState<{ id: string; downloads: number }[] | null>(null)
  const [openRepo, setOpenRepo] = useState<{ repo: string; files: HfFile[] | null } | null>(null)
  const progress = useDownloads()
  const addEndpoint = useLlamaStore((s) => s.addEndpoint)

  useEffect(() => {
    void call<Hardware>('cmd_hardware_info').then((h) => h && setHw(h))
    void call<string | null>('cmd_llama_server_path').then((p) => setServer(p ?? null))
  }, [])

  useEffect(() => {
    if (installing === null) return
    const off = on<{ received: number; total: number }>('local-models:llama', (p) => setInstalling(p.total ? Math.round((p.received / p.total) * 100) : 0))
    return () => void off.then((f) => f())
  }, [installing])

  // Per family, the biggest model that runs well here.
  const recommended = useMemo(
    () =>
      STARTERS.map((f) => {
        const best = [...f.variants].reverse().find((v) => fitOf(v.gb, hw) === 'well')
        return best ? { ...best, family: f.family } : null
      }).filter((x): x is NonNullable<typeof x> => !!x),
    [hw],
  )

  const install = async () => {
    setError(null)
    setInstalling(0)
    try {
      setServer(await call<string>('cmd_llama_install'))
    } catch (e) {
      setError(`Couldn't install llama.cpp: ${e instanceof Error ? e.message : String(e)}`)
    } finally {
      setInstalling(null)
    }
  }

  /** Download one GGUF, then add it as a local model ready to start. */
  const download = async (repo: string, file: HfFile, name: string, key = file.path) => {
    const local = file.path.split('/').pop()!
    setError(null)
    setFileFor((m) => ({ ...m, [key]: local }))
    setBusy((b) => ({ ...b, [key]: true }))
    try {
      const url = `https://huggingface.co/${repo}/resolve/main/${file.path.split('/').map(encodeURIComponent).join('/')}`
      const path = await call<string>('cmd_local_model_download', { url, file: local })
      if (!path) throw new Error('The download finished without a file.')
      await addEndpoint(path, name)
      setDone((d) => ({ ...d, [key]: name }))
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      if (msg !== 'Cancelled') setError(`Download failed: ${msg}`)
    } finally {
      setBusy((b) => ({ ...b, [key]: false }))
    }
  }

  const downloadStarter = async (repo: string, quant: string, name: string) => {
    setError(null)
    try {
      const files = await ggufFiles(repo)
      const file = files.find((f) => f.path.toLowerCase().includes(quant.toLowerCase())) ?? files[0]
      if (!file) throw new Error('No downloadable file in that repo.')
      await download(repo, file, name, repo)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  const search = async () => {
    const q = query.trim()
    if (!q) return
    setResults(null)
    setOpenRepo(null)
    try {
      const r = await fetch(`https://huggingface.co/api/models?search=${encodeURIComponent(q)}&filter=gguf&sort=downloads&direction=-1&limit=20`)
      const list = (await r.json()) as { id: string; downloads?: number }[]
      setResults(list.map((m) => ({ id: m.id, downloads: m.downloads ?? 0 })))
    } catch (e) {
      setError(`Search failed: ${e instanceof Error ? e.message : String(e)}`)
      setResults([])
    }
  }

  const openRepoFiles = async (repo: string) => {
    setOpenRepo({ repo, files: null })
    try {
      setOpenRepo({ repo, files: await ggufFiles(repo) })
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
      setOpenRepo(null)
    }
  }

  /** The download button, or its progress, or "added". */
  const action = (key: string, start: () => void) => {
    const local = fileFor[key] ?? key.split('/').pop()!
    if (done[key]) return <Typography sx={{ fontSize: '0.75rem', color: 'success.main', fontWeight: 600 }}>Added</Typography>
    if (busy[key]) {
      const p = progress[local]
      const pct = p?.total ? Math.round((p.received / p.total) * 100) : 0
      return (
        <Stack direction="row" sx={{ alignItems: 'center', gap: 0.75, width: 150 }}>
          <LinearProgress variant="determinate" value={pct} sx={{ flex: 1 }} />
          <Typography sx={{ fontSize: '0.6875rem', width: 30, textAlign: 'right' }}>{pct}%</Typography>
          <ButtonBase aria-label="Cancel download" onClick={() => void call('cmd_local_model_cancel', { file: local })} sx={{ borderRadius: '50%', color: 'text.secondary' }}>
            <CloseLinear size={13} color="currentColor" />
          </ButtonBase>
        </Stack>
      )
    }
    return (
      <Button size="small" variant="outlined" onClick={start} startIcon={<DownloadLinear size={13} color="currentColor" />} sx={{ textTransform: 'none', height: 28 }}>
        Get
      </Button>
    )
  }

  const card = { p: 1.5, borderRadius: '12px', bgcolor: alpha(theme.palette.text.primary, 0.03) } as const

  return (
    <Stack sx={{ gap: 1.5 }}>
      {/* What this computer has, and whether it can run models yet. */}
      <Stack direction="row" sx={{ ...card, alignItems: 'center', gap: 1.25 }}>
        <ComputerLinear size={18} color="currentColor" />
        <Stack sx={{ flex: 1, minWidth: 0 }}>
          <Typography sx={{ fontSize: '0.8125rem', fontWeight: 600 }}>This computer</Typography>
          <Typography sx={{ fontSize: '0.75rem', color: 'text.secondary' }}>
            {hw
              ? `${hw.ramGb} GB memory · ${hw.cpuThreads} threads · ${hw.gpus.length ? hw.gpus.map((g) => (hw.unified ? `${g.name} (shared memory)` : `${g.name} (${g.vramGb} GB)`)).join(', ') : 'no dedicated GPU'}`
              : 'Checking…'}
          </Typography>
        </Stack>
        {server === null &&
          (installing !== null ? (
            <Stack direction="row" sx={{ alignItems: 'center', gap: 0.75, width: 160 }}>
              <LinearProgress variant="determinate" value={installing} sx={{ flex: 1 }} />
              <Typography sx={{ fontSize: '0.6875rem' }}>{installing}%</Typography>
            </Stack>
          ) : (
            <Button size="small" variant="contained" onClick={() => void install()} sx={{ textTransform: 'none' }}>
              Install llama.cpp
            </Button>
          ))}
        {server && <Typography sx={{ fontSize: '0.6875rem', color: 'success.main', fontWeight: 600 }}>llama.cpp ready</Typography>}
      </Stack>
      {server === null && (
        <Typography sx={{ fontSize: '0.75rem', color: 'text.secondary', mt: -0.5 }}>
          Local models run on llama.cpp. Install it once (about 30 MB) and every model you get here can start with one click.
        </Typography>
      )}

      {error && (
        <Alert severity="error">
          {error}
        </Alert>
      )}

      {recommended.length > 0 && (
        <Stack sx={{ gap: 0.75 }}>
          <Typography sx={{ fontSize: '0.75rem', fontWeight: 700, textTransform: 'uppercase', letterSpacing: 0.4, color: 'text.secondary' }}>Recommended for this computer</Typography>
          <Box sx={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(190px, 1fr))', gap: 1 }}>
            {recommended.map((r) => (
              <Stack key={r.repo} sx={{ ...card, gap: 0.5 }}>
                <Typography sx={{ fontSize: '0.875rem', fontWeight: 650 }}>
                  {r.family} {r.label}
                </Typography>
                <Typography sx={{ fontSize: '0.6875rem', color: 'text.secondary' }}>
                  ~{fmtGb(r.gb)} · <FitTag fit="well" />
                </Typography>
                <Box sx={{ mt: 0.5 }}>{action(r.repo, () => void downloadStarter(r.repo, r.quant, `${r.family} ${r.label}`))}</Box>
              </Stack>
            ))}
          </Box>
        </Stack>
      )}

      <Stack sx={{ gap: 0.5 }}>
        <Typography sx={{ fontSize: '0.75rem', fontWeight: 700, textTransform: 'uppercase', letterSpacing: 0.4, color: 'text.secondary' }}>All starter models</Typography>
        {STARTERS.map((f) => (
          <Stack key={f.family} sx={{ py: 0.75, borderBottom: 1, borderColor: 'divider', gap: 0.5 }}>
            <Stack direction="row" sx={{ alignItems: 'baseline', gap: 1 }}>
              <Typography sx={{ fontSize: '0.8125rem', fontWeight: 600 }}>{f.family}</Typography>
              <Typography sx={{ fontSize: '0.75rem', color: 'text.secondary' }}>{f.blurb}</Typography>
            </Stack>
            {f.variants.map((v) => (
              <Stack key={v.repo} direction="row" sx={{ alignItems: 'center', gap: 1, pl: 1 }}>
                <Typography sx={{ fontSize: '0.8125rem', width: 64 }}>{v.label}</Typography>
                <Typography sx={{ fontSize: '0.75rem', color: 'text.secondary', width: 70 }}>~{fmtGb(v.gb)}</Typography>
                <Box sx={{ flex: 1 }}>
                  <FitTag fit={fitOf(v.gb, hw)} />
                </Box>
                {action(v.repo, () => void downloadStarter(v.repo, v.quant, `${f.family} ${v.label}`))}
              </Stack>
            ))}
          </Stack>
        ))}
      </Stack>

      {/* Anything else on Hugging Face that ships a GGUF. */}
      <Stack sx={{ gap: 0.75 }}>
        <Typography sx={{ fontSize: '0.75rem', fontWeight: 700, textTransform: 'uppercase', letterSpacing: 0.4, color: 'text.secondary' }}>Search Hugging Face</Typography>
        <Stack direction="row" sx={{ alignItems: 'center', gap: 1, px: 1.25, height: 34, borderRadius: '10px', bgcolor: alpha(theme.palette.text.primary, 0.05) }}>
          <SearchNormalLinear size={14} color="currentColor" />
          <InputBase
            fullWidth
            placeholder="e.g. deepseek, coder, qwen3 30b"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && void search()}
            sx={{ fontSize: '0.8125rem' }}
          />
        </Stack>
        {results?.length === 0 && <Typography sx={{ fontSize: '0.75rem', color: 'text.secondary' }}>Nothing found.</Typography>}
        {results?.map((r) => (
          <Box key={r.id}>
            <Stack
              direction="row"
              onClick={() => void (openRepo?.repo === r.id ? setOpenRepo(null) : openRepoFiles(r.id))}
              sx={{ alignItems: 'center', gap: 1, px: 1, py: 0.75, borderRadius: '8px', cursor: 'pointer', '&:hover': { bgcolor: 'action.hover' } }}
            >
              <Typography noWrap sx={{ fontSize: '0.8125rem', flex: 1, minWidth: 0 }}>
                {r.id}
              </Typography>
              <Typography sx={{ fontSize: '0.6875rem', color: 'text.disabled' }}>{r.downloads.toLocaleString()} downloads</Typography>
            </Stack>
            {openRepo?.repo === r.id && (
              <Stack sx={{ pl: 2, pb: 1, gap: 0.5 }}>
                {openRepo.files === null && <CircularProgress size={14} />}
                {openRepo.files?.length === 0 && <Typography sx={{ fontSize: '0.75rem', color: 'text.secondary' }}>No single-file GGUFs in this repo.</Typography>}
                {openRepo.files?.map((f) => (
                  <Stack key={f.path} direction="row" sx={{ alignItems: 'center', gap: 1 }}>
                    <Typography noWrap sx={{ fontSize: '0.75rem', fontFamily: '"Geist Mono", ui-monospace, monospace', flex: 1, minWidth: 0 }} title={f.path}>
                      {f.path}
                    </Typography>
                    <Typography sx={{ fontSize: '0.6875rem', color: 'text.secondary', width: 60, textAlign: 'right' }}>{fmtGb(gb(f.size))}</Typography>
                    <Box sx={{ width: 80 }}>
                      <FitTag fit={fitOf(gb(f.size), hw)} />
                    </Box>
                    {action(f.path, () => void download(r.id, f, f.path.replace(/\.gguf$/i, '')))}
                  </Stack>
                ))}
              </Stack>
            )}
          </Box>
        ))}
      </Stack>

      <OllamaModels />
    </Stack>
  )
}

/** Models in a running Ollama: listed, pulled, and added to the picker. */
function OllamaModels() {
  const [models, setModels] = useState<{ name: string; size: number }[] | null>(null)
  const [pull, setPull] = useState('')
  const [pulling, setPulling] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const providers = useCustomProviderStore((s) => s.providers)
  const refreshProviders = useCustomProviderStore((s) => s.refresh)
  const addProvider = useCustomProviderStore((s) => s.addProvider)
  const addModel = useCustomProviderStore((s) => s.addModel)

  const load = () =>
    fetch('http://localhost:11434/api/tags')
      .then((r) => r.json())
      .then((d: { models?: { name: string; size: number }[] }) => setModels(d.models ?? []))
      .catch(() => setModels(null))
  useEffect(() => {
    void load()
    void refreshProviders()
  }, [refreshProviders])

  if (models === null) return null
  const ollama = providers.find((p) => p.baseUrl.startsWith('http://localhost:11434'))

  const use = async (name: string) => {
    const id = ollama?.id ?? (await addProvider({ name: 'Ollama', baseUrl: 'http://localhost:11434/v1', apiFormat: 'openAiChat', apiKey: 'ollama' }))
    if (!ollama?.models.some((m) => m.id === name)) await addModel(id, { id: name, contextWindow: 32768, maxOutputTokens: null })
  }

  const doPull = async () => {
    const name = pull.trim()
    if (!name) return
    setError(null)
    setPulling(name)
    try {
      const r = await fetch('http://localhost:11434/api/pull', { method: 'POST', body: JSON.stringify({ model: name }) })
      const reader = r.body?.getReader()
      const dec = new TextDecoder()
      let last = ''
      while (reader) {
        const { done, value } = await reader.read()
        if (done) break
        last = dec.decode(value).trim().split('\n').pop() ?? last
        try {
          const j = JSON.parse(last) as { status?: string; completed?: number; total?: number; error?: string }
          if (j.error) throw new Error(j.error)
          setPulling(`${name}: ${j.status ?? ''}${j.total ? ` ${Math.round(((j.completed ?? 0) / j.total) * 100)}%` : ''}`)
        } catch (e) {
          if (e instanceof Error && !(e instanceof SyntaxError)) throw e
        }
      }
      setPull('')
      await load()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setPulling(null)
    }
  }

  return (
    <Stack sx={{ gap: 0.75 }}>
      <Typography sx={{ fontSize: '0.75rem', fontWeight: 700, textTransform: 'uppercase', letterSpacing: 0.4, color: 'text.secondary' }}>Ollama</Typography>
      {models.map((m) => {
        const added = ollama?.models.some((x) => x.id === m.name)
        return (
          <Stack key={m.name} direction="row" sx={{ alignItems: 'center', gap: 1, px: 1 }}>
            <Typography sx={{ fontSize: '0.8125rem', flex: 1 }}>{m.name}</Typography>
            <Typography sx={{ fontSize: '0.6875rem', color: 'text.secondary' }}>{fmtGb(gb(m.size))}</Typography>
            {added ? (
              <Typography sx={{ fontSize: '0.75rem', color: 'success.main', fontWeight: 600 }}>In the picker</Typography>
            ) : (
              <Button size="small" onClick={() => void use(m.name)} sx={{ textTransform: 'none' }}>
                Use
              </Button>
            )}
          </Stack>
        )
      })}
      <Stack direction="row" sx={{ gap: 1, alignItems: 'center' }}>
        <InputBase placeholder="Pull a model, e.g. qwen3:8b" value={pull} onChange={(e) => setPull(e.target.value)} sx={{ flex: 1, fontSize: '0.8125rem', px: 1.25, height: 32, borderRadius: '8px', border: 1, borderColor: 'divider' }} />
        <Button size="small" variant="outlined" disabled={!!pulling || !pull.trim()} onClick={() => void doPull()} sx={{ textTransform: 'none' }}>
          {pulling ? 'Pulling…' : 'Pull'}
        </Button>
      </Stack>
      {pulling && <Typography sx={{ fontSize: '0.75rem', color: 'text.secondary' }}>{pulling}</Typography>}
      {error && (
        <Alert severity="error">
          {error}
        </Alert>
      )}
    </Stack>
  )
}
