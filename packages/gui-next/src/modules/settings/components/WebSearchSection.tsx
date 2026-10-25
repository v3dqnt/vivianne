import { useEffect, useState } from 'react'
import { Stack, Typography, Button, InputBase, Select, MenuItem, Link, Alert } from '../../../ui'
import { call } from '../../../ipc'

/** Credential-store ids — must match `SearchApi::ids` in `tools_ext.rs`. */
const APIS = [
  { id: 'brave-search', value: 'brave', label: 'Brave Search', url: 'https://brave.com/search/api/', note: 'Free tier: 2,000 queries a month. Fast, independent index.' },
  { id: 'tavily', value: 'tavily', label: 'Tavily', url: 'https://tavily.com', note: 'Free tier: 1,000 searches a month. Built for AI agents.' },
  { id: 'exa', value: 'exa', label: 'Exa', url: 'https://exa.ai', note: 'Neural search with page highlights. Free credits to start.' },
] as const

const inputSx = { fontSize: '0.8125rem', px: 1.25, py: 0.5, border: 1, borderColor: 'divider', borderRadius: '8px', flex: 1 } as const

/**
 * How the agent's `web_search` finds things. Without a key it uses DuckDuckGo,
 * which throttles bursts, and falls back to OpenAI's web search when an
 * OpenAI key is set. A dedicated search API is the fast, dependable option.
 */
export function WebSearchSection() {
  const [settings, setSettings] = useState<Record<string, unknown> | null>(null)
  const [status, setStatus] = useState<Record<string, boolean>>({})
  const [drafts, setDrafts] = useState<Record<string, string>>({})
  const [busy, setBusy] = useState<string | null>(null)
  const [testing, setTesting] = useState(false)
  const [test, setTest] = useState<{ engine: string; ms: number; count: number; first: { title: string; url: string } | null } | { error: string } | null>(null)

  const runTest = async () => {
    setTesting(true)
    setTest(null)
    try {
      const r = await call<{ engine: string; ms: number; count: number; first: { title: string; url: string } | null }>('cmd_web_search_test', { query: 'NVIDIA Nemotron 3 open models' })
      setTest(r ?? { error: 'No answer.' })
    } catch (e) {
      setTest({ error: e instanceof Error ? e.message : String(e) })
    } finally {
      setTesting(false)
    }
  }

  const refresh = async () => {
    const s = await call<Record<string, unknown>>('cmd_get_settings')
    setSettings(s ?? {})
    const entries = await Promise.all(APIS.map(async (a) => [a.id, (await call<boolean>('auth_is_authenticated', { providerId: a.id })) ?? false] as const))
    setStatus(Object.fromEntries(entries))
  }

  useEffect(() => {
    void refresh()
  }, [])

  const provider = (settings?.webSearchProvider as string | undefined) ?? 'auto'
  const setProvider = async (value: string) => {
    const next = { ...(settings ?? {}), webSearchProvider: value }
    setSettings(next)
    await call('cmd_save_settings', { settings: next })
  }

  const saveKey = async (id: string) => {
    const key = drafts[id]?.trim()
    if (!key) return
    setBusy(id)
    try {
      await call('auth_store_api_key', { providerId: id, apiKey: key })
      setDrafts((d) => ({ ...d, [id]: '' }))
      await refresh()
    } finally {
      setBusy(null)
    }
  }

  const removeKey = async (id: string) => {
    setBusy(id)
    try {
      await call('auth_delete_credential', { providerId: id })
      await refresh()
    } finally {
      setBusy(null)
    }
  }

  const configured = APIS.filter((a) => status[a.id])
  const active =
    provider === 'duckduckgo'
      ? 'DuckDuckGo'
      : provider === 'auto'
        ? (configured[0]?.label ?? 'DuckDuckGo, then OpenAI web search if you have an OpenAI key')
        : (APIS.find((a) => a.value === provider)?.label ?? provider)

  return (
    <Stack sx={{ gap: 2.5 }}>
      <Stack sx={{ gap: 0.5 }}>
        <Typography variant="subtitle1" sx={{ fontWeight: 700 }}>
          Web search
        </Typography>
        <Typography variant="body2" color="text.secondary">
          What the agent’s <code>web_search</code> tool uses. Without a key it searches DuckDuckGo, which slows down or blocks
          several searches in a row. A search API key makes it fast and reliable.
        </Typography>
      </Stack>

      <Stack direction="row" sx={{ alignItems: 'center', gap: 1.5 }}>
        <Typography variant="body2" sx={{ fontWeight: 600, minWidth: 90 }}>
          Provider
        </Typography>
        <Select size="small" value={provider} onChange={(e) => void setProvider(e.target.value)} sx={{ minWidth: 220, fontSize: '0.8125rem' }}>
          <MenuItem value="auto">Automatic</MenuItem>
          {APIS.map((a) => (
            <MenuItem key={a.value} value={a.value}>
              {a.label}
            </MenuItem>
          ))}
          <MenuItem value="duckduckgo">DuckDuckGo only (no key)</MenuItem>
        </Select>
      </Stack>
      <Typography variant="caption" color="text.secondary" sx={{ mt: -1.5 }}>
        In use: {active}
      </Typography>

      {/* One real query through the engine in use: proof the key works and
          which engine answers. */}
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1.5 }}>
        <Button size="small" variant="outlined" disabled={testing} onClick={() => void runTest()} sx={{ borderRadius: '8px', textTransform: 'none' }}>
          {testing ? 'Searching…' : 'Test search'}
        </Button>
        {test && 'error' in test && (
          <Alert severity="warning" sx={{ flex: 1 }}>
            {test.error}
          </Alert>
        )}
        {test && 'engine' in test && (
          <Typography variant="caption" color="text.secondary" sx={{ flex: 1, minWidth: 0 }}>
            <Typography component="span" variant="caption" sx={{ fontWeight: 600, color: 'success.main' }}>
              {test.engine}
            </Typography>{' '}
            answered in {test.ms} ms with {test.count} result{test.count === 1 ? '' : 's'}
            {test.first ? ` · ${test.first.title}` : ''}
          </Typography>
        )}
      </Stack>

      {APIS.map((a) => (
        <Stack key={a.id} sx={{ gap: 0.75, p: 1.5, border: 1, borderColor: 'divider', borderRadius: '10px' }}>
          <Stack direction="row" sx={{ alignItems: 'baseline', gap: 1 }}>
            <Typography variant="body2" sx={{ fontWeight: 600 }}>
              {a.label}
            </Typography>
            <Typography variant="caption" color={status[a.id] ? 'success.main' : 'text.disabled'}>
              {status[a.id] ? 'Key saved' : 'No key'}
            </Typography>
            <Link href={a.url} target="_blank" rel="noopener noreferrer" variant="caption" sx={{ ml: 'auto' }}>
              Get a key
            </Link>
          </Stack>
          <Typography variant="caption" color="text.secondary">
            {a.note}
          </Typography>
          <Stack direction="row" sx={{ gap: 1 }}>
            <InputBase
              type="password"
              placeholder={status[a.id] ? 'Replace the saved key' : 'Paste an API key'}
              value={drafts[a.id] ?? ''}
              onChange={(e) => setDrafts((d) => ({ ...d, [a.id]: e.target.value }))}
              onKeyDown={(e) => e.key === 'Enter' && void saveKey(a.id)}
              sx={inputSx}
            />
            <Button size="small" variant="contained" disableElevation disabled={!drafts[a.id]?.trim() || busy === a.id} onClick={() => void saveKey(a.id)} sx={{ borderRadius: '8px' }}>
              Save
            </Button>
            {status[a.id] && (
              <Button size="small" color="inherit" disabled={busy === a.id} onClick={() => void removeKey(a.id)} sx={{ borderRadius: '8px', color: 'text.secondary' }}>
                Remove
              </Button>
            )}
          </Stack>
        </Stack>
      ))}
    </Stack>
  )
}
