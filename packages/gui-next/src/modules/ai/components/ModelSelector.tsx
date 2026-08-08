import { useEffect, useMemo, useState } from 'react'
import Box from '@mui/material/Box'
import Button from '@mui/material/Button'
import Menu from '@mui/material/Menu'
import MenuItem from '@mui/material/MenuItem'
import ListSubheader from '@mui/material/ListSubheader'
import InputBase from '@mui/material/InputBase'
import Typography from '@mui/material/Typography'
import { TickCircleLinear, SearchNormalLinear } from 'mx-icons'
import { useModelStore } from '../store/modelStore'

// Deterministic color per provider name — avoids fetching real brand logos
// over the network (this is an offline-capable desktop app), while still
// giving each provider a distinct, recognizable mark in the list.
function providerColor(provider: string): string {
  let hash = 0
  for (let i = 0; i < provider.length; i++) hash = (hash * 31 + provider.charCodeAt(i)) >>> 0
  return `hsl(${hash % 360}, 55%, 55%)`
}

function ProviderMark({ provider, size = 16 }: { provider: string; size?: number }) {
  return (
    <Box
      sx={{
        width: size,
        height: size,
        borderRadius: '30%',
        flexShrink: 0,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        bgcolor: providerColor(provider),
        color: 'rgba(0,0,0,0.75)',
        fontSize: size * 0.55,
        fontWeight: 700,
        lineHeight: 1,
      }}
    >
      {provider.charAt(0).toUpperCase()}
    </Box>
  )
}

export function ModelSelector() {
  const models = useModelStore((s) => s.models)
  const stats = useModelStore((s) => s.stats)
  const selectModel = useModelStore((s) => s.selectModel)
  const init = useModelStore((s) => s.init)
  const [anchor, setAnchor] = useState<HTMLElement | null>(null)
  const [query, setQuery] = useState('')

  useEffect(() => {
    void init()
  }, [init])

  useEffect(() => {
    if (!anchor) setQuery('')
  }, [anchor])

  const filtered = useMemo(() => {
    if (!query.trim()) return models
    const q = query.trim().toLowerCase()
    return models.filter((m) => m.name.toLowerCase().includes(q) || m.provider.toLowerCase().includes(q))
  }, [models, query])

  const grouped = useMemo(() => {
    const map = new Map<string, typeof models>()
    for (const m of filtered) map.set(m.provider, [...(map.get(m.provider) ?? []), m])
    return map
  }, [filtered])

  const label = stats?.modelName || 'Select model'

  return (
    <>
      <Button
        size="small"
        onClick={(e) => setAnchor(e.currentTarget)}
        startIcon={stats?.provider && <ProviderMark provider={stats.provider} size={13} />}
        sx={{
          textTransform: 'none',
          fontSize: '0.8125rem',
          color: 'text.primary',
          maxWidth: 200,
          minWidth: 0,
          borderRadius: 999,
          px: 1,
          py: 0.5,
          '&:hover': { bgcolor: 'action.hover' },
        }}
      >
        <Typography variant="body2" noWrap sx={{ fontWeight: 600 }}>
          {label}
        </Typography>
      </Button>
      <Menu
        anchorEl={anchor}
        open={!!anchor}
        onClose={() => setAnchor(null)}
        anchorOrigin={{ vertical: 'top', horizontal: 'left' }}
        transformOrigin={{ vertical: 'bottom', horizontal: 'left' }}
        slotProps={{ paper: { sx: { maxHeight: 400, width: 320 } } }}
      >
        <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, px: 1.5, py: 1, position: 'sticky', top: 0, bgcolor: 'background.paper', zIndex: 1 }}>
          <SearchNormalLinear size={14} color="currentColor" />
          <InputBase
            autoFocus
            fullWidth
            placeholder="Search models…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => e.stopPropagation()}
            sx={{ fontSize: '0.8125rem' }}
          />
        </Box>
        {filtered.length === 0 && (
          <MenuItem disabled>
            <Typography variant="caption">{models.length === 0 ? 'No authenticated models' : 'No matches'}</Typography>
          </MenuItem>
        )}
        {Array.from(grouped.entries()).map(([provider, providerModels]) => [
          <ListSubheader key={`h-${provider}`} sx={{ fontSize: '0.6875rem', textTransform: 'uppercase', letterSpacing: 0.4, lineHeight: '28px' }}>
            {provider}
          </ListSubheader>,
          ...providerModels.map((m) => {
            const selected = stats?.modelId === m.id && stats?.provider === m.provider
            return (
              <MenuItem
                key={`${provider}-${m.id}`}
                selected={selected}
                onClick={() => {
                  void selectModel(m)
                  setAnchor(null)
                }}
                sx={{ gap: 1, fontSize: '0.8125rem' }}
              >
                <ProviderMark provider={m.provider} />
                <Box sx={{ flex: 1, minWidth: 0 }}>
                  <Typography variant="body2" noWrap>
                    {m.name}
                  </Typography>
                </Box>
                {m.contextWindow && (
                  <Typography variant="caption" color="text.secondary" sx={{ fontFamily: 'monospace' }}>
                    {Math.round(m.contextWindow / 1000)}K
                  </Typography>
                )}
                {selected && <TickCircleLinear size={14} color="currentColor" />}
              </MenuItem>
            )
          }),
        ])}
      </Menu>
    </>
  )
}
