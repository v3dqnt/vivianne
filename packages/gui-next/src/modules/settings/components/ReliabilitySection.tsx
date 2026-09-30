import { useCallback, useEffect, useMemo, useState } from 'react'
import { Box, Divider, IconButton, MenuItem, Stack, Switch, TextField, Tooltip, Typography } from '../../../ui'
import { ArrowDownLinear, ArrowUpLinear, TrashLinear } from '../../../icons'
import { call } from '../../../ipc'
import { useModelStore } from '../../ai/store/modelStore'

/** `agent::types::ModelRef` exactly as the backend deserializes it — no
 * serde rename, so snake_case, and every field present. */
interface ModelRef {
  id: string
  name: string
  api: string
  provider: string
  base_url: string | null
  reasoning: boolean
  max_tokens: number
  context_window: number
}

const keyOf = (m: { provider: string; id: string }) => `${m.provider}/${m.id}`

/**
 * What the agent does when its model fails: which models take over, in what
 * order, and whether transient failures are retried at all.
 *
 * The chain is what keeps a long task alive when a provider goes down or a
 * key runs dry — the run moves to the next model with the same context and
 * says so in the chat, then tries its own model again half an hour later.
 */
export function ReliabilitySection() {
  const models = useModelStore((s) => s.models)
  const initModels = useModelStore((s) => s.init)
  const [settings, setSettings] = useState<Record<string, unknown> | null>(null)
  const [chain, setChain] = useState<ModelRef[]>([])
  const [autoRetry, setAutoRetry] = useState(true)

  useEffect(() => {
    void initModels()
    void (async () => {
      const cfg = await call<Record<string, unknown>>('cmd_get_settings')
      if (!cfg) return
      setSettings(cfg)
      setAutoRetry(cfg.autoRetryEnabled !== false)
      // The legacy single `fallbackModel` becomes the chain's head.
      const list: ModelRef[] = []
      if (cfg.fallbackModel && typeof cfg.fallbackModel === 'object') list.push(cfg.fallbackModel as ModelRef)
      if (Array.isArray(cfg.fallbackModels)) list.push(...(cfg.fallbackModels as ModelRef[]))
      setChain(list.filter((m, i) => list.findIndex((o) => keyOf(o) === keyOf(m)) === i))
    })()
  }, [initModels])

  const save = useCallback(
    async (next: Record<string, unknown>) => {
      const merged = { ...(settings ?? {}), ...next }
      await call('cmd_save_settings', { settings: merged })
      setSettings(merged)
    },
    [settings],
  )

  const saveChain = (next: ModelRef[]) => {
    setChain(next)
    // Written as the list alone: the legacy key is folded in and cleared, so
    // the two can never disagree about what comes first.
    void save({ fallbackModels: next, fallbackModel: null })
  }

  const choices = useMemo(
    () => models.filter((m) => !m.isImageModel && !m.isAudioModel && !chain.some((c) => keyOf(c) === keyOf(m))),
    [models, chain],
  )

  const add = (key: string) => {
    const m = models.find((x) => keyOf(x) === key)
    if (!m) return
    saveChain([
      ...chain,
      {
        id: m.id,
        name: m.name,
        api: m.api,
        provider: m.provider,
        base_url: null,
        reasoning: false,
        max_tokens: m.maxOutputTokens ?? 8192,
        context_window: m.contextWindow ?? 128_000,
      },
    ])
  }

  const move = (i: number, by: -1 | 1) => {
    const next = [...chain]
    const [m] = next.splice(i, 1)
    next.splice(i + by, 0, m)
    saveChain(next)
  }

  return (
    <Stack sx={{ gap: 2.5 }}>
      <Stack sx={{ gap: 0.5 }}>
        <Typography variant="body2" sx={{ fontWeight: 600 }}>
          Reliability
        </Typography>
        <Typography variant="caption" color="text.secondary">
          What happens when the model fails. Rate limits and outages are waited out; if the model stays down or its key
          runs out, the agent continues on the next model below with the same context, and says so in the chat.
        </Typography>
      </Stack>

      <Divider />

      <Stack sx={{ gap: 1 }}>
        <Typography variant="body2" sx={{ fontWeight: 500 }}>
          Fallback models
        </Typography>
        <Typography variant="caption" color="text.secondary">
          Tried in order after your selected model. Pick models from different providers, so one provider's outage
          can't stop the agent.
        </Typography>
        {chain.length === 0 && (
          <Typography variant="caption" color="text.disabled" sx={{ py: 0.5 }}>
            No fallbacks — a failed model ends the run.
          </Typography>
        )}
        {chain.map((m, i) => (
          <Stack
            key={keyOf(m)}
            direction="row"
            sx={{ alignItems: 'center', gap: 1, px: 1.25, py: 0.75, border: 1, borderColor: 'divider', borderRadius: '10px' }}
          >
            <Typography variant="caption" color="text.disabled" sx={{ width: 16 }}>
              {i + 1}
            </Typography>
            <Box sx={{ flex: 1, minWidth: 0 }}>
              <Typography variant="body2" noWrap>
                {m.name || m.id}
              </Typography>
              <Typography variant="caption" color="text.secondary" noWrap sx={{ display: 'block' }}>
                {m.provider}
              </Typography>
            </Box>
            <Tooltip title="Move up">
              <span>
                <IconButton size="small" disabled={i === 0} onClick={() => move(i, -1)}>
                  <ArrowUpLinear size={14} color="currentColor" />
                </IconButton>
              </span>
            </Tooltip>
            <Tooltip title="Move down">
              <span>
                <IconButton size="small" disabled={i === chain.length - 1} onClick={() => move(i, 1)}>
                  <ArrowDownLinear size={14} color="currentColor" />
                </IconButton>
              </span>
            </Tooltip>
            <Tooltip title="Remove">
              <IconButton size="small" onClick={() => saveChain(chain.filter((_, j) => j !== i))}>
                <TrashLinear size={14} color="currentColor" />
              </IconButton>
            </Tooltip>
          </Stack>
        ))}
        <TextField
          select
          size="small"
          label="Add a fallback model"
          value=""
          onChange={(e) => add(e.target.value)}
          disabled={choices.length === 0}
          helperText={models.length === 0 ? 'Connect a provider in Providers to choose from its models.' : undefined}
        >
          {choices.map((m) => (
            <MenuItem key={keyOf(m)} value={keyOf(m)}>
              {m.name} — {m.provider}
            </MenuItem>
          ))}
        </TextField>
      </Stack>

      <Divider />

      <Stack direction="row" sx={{ alignItems: 'center', gap: 2 }}>
        <Box sx={{ flex: 1 }}>
          <Typography variant="body2" sx={{ fontWeight: 500 }}>
            Retry transient failures
          </Typography>
          <Typography variant="caption" color="text.secondary">
            Retry rate limits, overloads and dropped connections automatically instead of stopping on the first one.
          </Typography>
        </Box>
        <Switch
          checked={autoRetry}
          onChange={(e) => {
            setAutoRetry(e.target.checked)
            void save({ autoRetryEnabled: e.target.checked })
          }}
        />
      </Stack>
    </Stack>
  )
}
