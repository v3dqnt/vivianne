import { useEffect, useState } from 'react'
import { Alert, Box, Button, Checkbox, CircularProgress, Dialog, DialogActions, DialogContent, DialogTitle, FormControlLabel, InputBase, Stack, Typography, alpha, useTheme } from '../../ui'
import { TickCircleLinear } from '../../icons'
import { call } from '../../ipc'
import { useModelStore, type SelectableModel } from '../ai/store/modelStore'
import { ModelPicker } from '../ai/components/ModelPicker'
import { useAuthStore } from '../settings/store/authStore'
import type { DiscoveredModel } from '../ai/lib/types'
import type { SubagentModelOverride } from '../settings/lib/subagentProfiles'
import { EDITION, pickNemotron, useQuickstart } from './edition'

const toOverride = (m: DiscoveredModel): SubagentModelOverride => ({
  id: m.id,
  name: m.name,
  api: m.api,
  provider: m.provider,
  baseUrl: null,
  maxTokens: m.maxOutputTokens ?? 8192,
  contextWindow: m.contextWindow ?? 128000,
})

const field = { fontSize: '0.8125rem', px: 1.25, height: 36, border: 1, borderColor: 'divider', borderRadius: '9px', flex: 1 } as const

/**
 * First-run card for the edition: paste a Nebius key, and the app picks
 * Nemotron Ultra for chat and a Nano for background work, turns on cost-aware
 * routing, and optionally wires Tavily search and a hosted embedder.
 */
export function Quickstart() {
  const theme = useTheme()
  const { open, done, close } = useQuickstart()
  const refreshAuth = useAuthStore((s) => s.refresh)
  const [nebiusKey, setNebiusKey] = useState('')
  const [tavilyKey, setTavilyKey] = useState('')
  const [nvidiaKey, setNvidiaKey] = useState('')
  const [connected, setConnected] = useState(false)
  const [models, setModels] = useState<DiscoveredModel[]>([])
  const [smart, setSmart] = useState<SelectableModel | null>(null)
  const [quick, setQuick] = useState<SubagentModelOverride | null>(null)
  const [tiers, setTiers] = useState<{ easy: DiscoveredModel | null; medium: DiscoveredModel | null; hard: DiscoveredModel | null }>({ easy: null, medium: null, hard: null })
  const [embed, setEmbed] = useState(true)
  const [busy, setBusy] = useState<'connect' | 'apply' | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [applied, setApplied] = useState<string[] | null>(null)

  // The first time this edition runs, show the card.
  useEffect(() => {
    if (done) return
    const t = window.setTimeout(() => useQuickstart.getState().show(), 800)
    return () => window.clearTimeout(t)
  }, [done])

  /** Already connected? Pick the roles from what the account lists. */
  const detect = async () => {
    const list = (await call<DiscoveredModel[]>('cmd_list_available_models', { refresh: true })) ?? []
    useModelStore.setState({ models: list })
    setModels(list)
    const p = pickNemotron(list)
    setSmart(p.smart)
    setQuick(p.quick ? toOverride(p.quick) : null)
    setTiers({ easy: p.quick, medium: p.super, hard: p.smart })
    return list.some((m) => m.provider === 'nebius')
  }
  useEffect(() => {
    if (open) void detect().then(setConnected)
  }, [open])

  const connect = async () => {
    if (!nebiusKey.trim()) return
    setBusy('connect')
    setError(null)
    try {
      await call('auth_store_api_key', { providerId: 'nebius', apiKey: nebiusKey.trim() })
      await refreshAuth()
      const ok = await detect()
      setConnected(ok)
      if (!ok) setError("The key was saved, but Nebius listed no models for it. Check the key, then try again.")
      else setNebiusKey('')
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(null)
    }
  }

  const apply = async () => {
    setBusy('apply')
    setError(null)
    const did: string[] = []
    try {
      const st = (await call<Record<string, unknown>>('cmd_get_settings')) ?? {}
      const next: Record<string, unknown> = { ...st }

      if (smart) {
        await useModelStore.getState().selectModel(smart)
        did.push(`Chat and planning: ${smart.name || smart.id}`)
      }
      if (quick) {
        const tier = (m: DiscoveredModel | null) => (m ? toOverride(m) : null)
        next.routing = {
          enabled: true,
          quick,
          // Hard tasks to the biggest model, easy ones to the smallest.
          auto: !!(tiers.easy || tiers.medium || tiers.hard),
          tiers: { easy: tier(tiers.easy), medium: tier(tiers.medium), hard: tier(tiers.hard) },
        }
        did.push(`Background work routed to ${quick.name || quick.id}`)
        if (tiers.hard && tiers.easy) did.push(`Tasks matched to difficulty: ${tiers.easy.name || tiers.easy.id} to ${tiers.hard.name || tiers.hard.id}`)
      }
      if (tavilyKey.trim()) {
        await call('auth_store_api_key', { providerId: 'tavily', apiKey: tavilyKey.trim() })
        next.webSearchProvider = 'tavily'
        did.push('Web search through Tavily')
      }
      if (nvidiaKey.trim()) {
        await call('auth_store_api_key', { providerId: 'nvidia-nim', apiKey: nvidiaKey.trim() })
        await refreshAuth()
      }
      if (embed) {
        // NVIDIA's Nemotron embedder when there is an NVIDIA key; Nebius's Qwen3 otherwise.
        const provider = nvidiaKey.trim() || (await call<boolean>('auth_is_authenticated', { providerId: 'nvidia-nim' })) ? 'nvidia-nim' : 'nebius'
        const ids = (await call<string[]>('cmd_list_embedding_models', { provider }).catch(() => [])) ?? []
        const model = provider === 'nvidia-nim' ? (ids.find((i) => /nemotron/i.test(i)) ?? ids[0] ?? 'nvidia/nv-embedqa-e5-v5') : (ids.find((i) => /qwen3-embedding/i.test(i)) ?? 'Qwen/Qwen3-Embedding-8B')
        next.memoryEmbedder = { prefer: 'cloud', provider, model }
        did.push(`Memory embedded with ${model}`)
      }
      await call('cmd_save_settings', { settings: next })
      if (embed) await call('cmd_embedder_refresh')
      setApplied(did)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(null)
    }
  }

  const nebiusModels = models.filter((m) => m.provider === 'nebius').length

  return (
    <Dialog open={open} onClose={() => close(false)} maxWidth="sm" fullWidth>
      <DialogTitle>
        <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
          <Box sx={{ width: 10, height: 10, borderRadius: '50%', bgcolor: 'primary.main' }} />
          {EDITION.name}
        </Stack>
      </DialogTitle>
      <DialogContent>
        {applied ? (
          <Stack sx={{ gap: 1.25 }}>
            <Alert severity="success">You're set up.</Alert>
            {applied.map((a) => (
              <Stack key={a} direction="row" sx={{ gap: 1, alignItems: 'center' }}>
                <TickCircleLinear size={15} color="currentColor" />
                <Typography variant="body2">{a}</Typography>
              </Stack>
            ))}
            <Typography variant="caption" color="text.secondary">
              Settings → Usage → By model shows what each model costs as you work. Cost-aware routing is under Settings → Providers.
            </Typography>
          </Stack>
        ) : (
          <Stack sx={{ gap: 2 }}>
            <Typography variant="body2" color="text.secondary">
              Vivianne on NVIDIA Nemotron through Nebius Token Factory: a strong model plans and answers, a small one does the background work, and you see what each cost.
            </Typography>

            <Stack sx={{ gap: 0.75 }}>
              <Typography variant="body2" sx={{ fontWeight: 600 }}>
                1. Nebius API key
              </Typography>
              <Stack direction="row" sx={{ gap: 1 }}>
                <InputBase type="password" placeholder={connected ? 'Connected. Paste a new key to replace it' : 'Paste your Nebius Token Factory key'} value={nebiusKey} onChange={(e) => setNebiusKey(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && void connect()} sx={field} />
                <Button variant="contained" disabled={!nebiusKey.trim() || busy !== null} onClick={() => void connect()} startIcon={busy === 'connect' ? <CircularProgress size={14} /> : undefined} sx={{ textTransform: 'none' }}>
                  Connect
                </Button>
              </Stack>
              {connected && (
                <Typography variant="caption" sx={{ color: 'success.main' }}>
                  Connected: {nebiusModels} models available.
                </Typography>
              )}
            </Stack>

            {connected && (
              <Stack sx={{ gap: 1, p: 1.5, borderRadius: '10px', bgcolor: alpha(theme.palette.primary.main, 0.07) }}>
                <Typography variant="body2" sx={{ fontWeight: 600 }}>
                  2. Models
                </Typography>
                <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
                  <Typography variant="caption" sx={{ width: 120 }}>
                    Chat and planning
                  </Typography>
                  <Typography variant="body2" sx={{ fontWeight: 500 }}>
                    {smart ? smart.name || smart.id : 'No Nemotron found: pick one in the chat picker'}
                  </Typography>
                </Stack>
                <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
                  <Typography variant="caption" sx={{ width: 120 }}>
                    Background work
                  </Typography>
                  <Box sx={{ flex: 1, minWidth: 0 }}>
                    <ModelPicker mode="agent" variant="field" value={quick} onChange={setQuick} />
                  </Box>
                </Stack>
                <Typography variant="caption" color="text.secondary">
                  Picked from what your account lists. The quick model gets summaries, memory upkeep, routines and read-only exploring; change either any time.
                </Typography>
              </Stack>
            )}

            <Stack sx={{ gap: 0.75 }}>
              <Typography variant="body2" sx={{ fontWeight: 600 }}>
                {connected ? '3. Optional keys' : '2. Optional keys'}
              </Typography>
              <Stack direction="row" sx={{ gap: 1 }}>
                <InputBase type="password" placeholder="Tavily key: web search the agent can cite" value={tavilyKey} onChange={(e) => setTavilyKey(e.target.value)} sx={field} />
              </Stack>
              <Stack direction="row" sx={{ gap: 1 }}>
                <InputBase type="password" placeholder="NVIDIA key: Nemotron embedder for memory" value={nvidiaKey} onChange={(e) => setNvidiaKey(e.target.value)} sx={field} />
              </Stack>
              <FormControlLabel
                control={<Checkbox size="small" checked={embed} onChange={(e) => setEmbed(e.target.checked)} />}
                label={<Typography variant="caption">Embed memory in the cloud (Nemotron with an NVIDIA key, Qwen3 on Nebius without). It re-embeds in the background.</Typography>}
              />
            </Stack>

            {error && <Alert severity="error">{error}</Alert>}
          </Stack>
        )}
      </DialogContent>
      <DialogActions>
        {applied ? (
          <Button variant="contained" onClick={() => close(true)} sx={{ textTransform: 'none' }}>
            Start
          </Button>
        ) : (
          <>
            <Button onClick={() => close(true)} sx={{ textTransform: 'none' }}>
              Skip for now
            </Button>
            <Button variant="contained" disabled={!connected || busy !== null} onClick={() => void apply()} startIcon={busy === 'apply' ? <CircularProgress size={14} /> : undefined} sx={{ textTransform: 'none' }}>
              Use these
            </Button>
          </>
        )}
      </DialogActions>
    </Dialog>
  )
}
