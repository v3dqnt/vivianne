import { useMemo } from 'react'
import { MenuItem, Select, Typography } from '../../../ui'
import { useModelOptions } from '../../settings/components/SubagentsSection'
import type { SubagentModelOverride } from '../../settings/lib/subagentProfiles'

const INHERIT = '__inherit__'

/** A model is a provider and an id: the same id can be on two providers
 * (an OpenAI key and a ChatGPT plan both offer `gpt-…`). */
const keyOf = (m: Pick<SubagentModelOverride, 'provider' | 'id'>) => `${m.provider}::${m.id}`

const PROVIDER_LABEL: Record<string, string> = {
  chatgpt: 'ChatGPT plan',
  openai: 'OpenAI',
  anthropic: 'Anthropic',
  google: 'Google',
  xai: 'xAI',
  local: 'On this computer',
}

/**
 * Which model an agent runs on: "Same as the chat" (follows the chat's model
 * picker) or a specific one, grouped by provider.
 */
export function ModelPicker({
  value,
  onChange,
  size = 'small',
  fullWidth,
}: {
  value: SubagentModelOverride | null
  onChange: (model: SubagentModelOverride | null) => void
  size?: 'small' | 'medium'
  fullWidth?: boolean
}) {
  const options = useModelOptions()
  const groups = useMemo(() => {
    const by = new Map<string, SubagentModelOverride[]>()
    for (const m of options) by.set(m.provider, [...(by.get(m.provider) ?? []), m])
    return [...by.entries()]
  }, [options])
  const current = value ? keyOf(value) : INHERIT
  // A model that's no longer listed (key removed, server stopped) stays
  // selectable as itself rather than silently reading "Same as the chat".
  const missing = value && !options.some((m) => keyOf(m) === current)

  return (
    <Select
      size={size}
      fullWidth={fullWidth}
      value={current}
      onChange={(e) => {
        const k = String(e.target.value)
        onChange(k === INHERIT ? null : (options.find((m) => keyOf(m) === k) ?? value))
      }}
      sx={{ fontSize: '0.8125rem', borderRadius: '8px', minWidth: 180 }}
    >
      <MenuItem value={INHERIT} sx={{ fontSize: '0.8125rem' }}>
        Same as the chat
      </MenuItem>
      {missing && value && (
        <MenuItem value={current} sx={{ fontSize: '0.8125rem' }}>
          {value.name} (unavailable)
        </MenuItem>
      )}
      {groups.flatMap(([provider, models]) => [
        <MenuItem key={`h-${provider}`} value={`__header__${provider}`} disabled sx={{ opacity: '1 !important', py: 0.25, minHeight: 0 }}>
          <Typography variant="caption" color="text.secondary" sx={{ fontWeight: 700, fontSize: '0.625rem', textTransform: 'uppercase', letterSpacing: 0.5 }}>
            {PROVIDER_LABEL[provider] ?? provider}
          </Typography>
        </MenuItem>,
        ...models.map((m) => (
          <MenuItem key={keyOf(m)} value={keyOf(m)} sx={{ fontSize: '0.8125rem', pl: 2.5 }}>
            {m.name}
          </MenuItem>
        )),
      ])}
    </Select>
  )
}
