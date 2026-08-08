import { useEffect } from 'react'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import Divider from '@mui/material/Divider'
import { CLOUD_PROVIDERS } from '../lib/providers'
import { useAuthStore } from '../store/authStore'
import { ProviderRow } from './ProviderRow'

export function ProvidersSection() {
  const refresh = useAuthStore((s) => s.refresh)

  useEffect(() => {
    void refresh()
  }, [refresh])

  return (
    <Stack sx={{ gap: 0.5 }}>
      <Typography variant="subtitle2" sx={{ fontWeight: 600, mb: 0.5 }}>
        Providers
      </Typography>
      <Typography variant="caption" color="text.secondary" sx={{ mb: 1 }}>
        Keys are stored in your OS keychain, never written to disk in plain text.
      </Typography>
      <Divider sx={{ mb: 0.5 }} />
      {CLOUD_PROVIDERS.map((p) => (
        <ProviderRow key={p.id} provider={p} />
      ))}
    </Stack>
  )
}
