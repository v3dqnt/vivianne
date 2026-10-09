import { useEffect, useState } from 'react'
import { Box, Button, Stack, Typography } from '../../../ui'
import { call } from '../../../ipc'

/** Mirrors `mac_permissions::Permission`. */
interface Permission {
  id: string
  label: string
  why: string
  state: 'granted' | 'denied' | 'ask'
}

/**
 * Whether this WebKit is new enough for everything the interface uses.
 * Selector `:has()` arrived in Safari 15.4 — the newest Safari macOS 10.15
 * can run is 15.6, so a Catalina Mac that hasn't updated Safari lands here.
 */
function webkitTooOld(): boolean {
  try {
    return typeof CSS !== 'undefined' && !CSS.supports('selector(:has(*))')
  } catch {
    return false
  }
}

/**
 * The macOS privacy switches Vivianne needs, each with its state and a
 * button that asks macOS (and opens the right pane of System Settings).
 * Renders nothing on Windows.
 */
export function MacPermissions() {
  const [perms, setPerms] = useState<Permission[]>([])

  const refresh = () => void call<Permission[]>('cmd_mac_permissions').then((p) => p && setPerms(p)).catch(() => {})

  useEffect(() => {
    refresh()
    // Granting happens in System Settings; pick it up when the user comes back.
    const onFocus = () => refresh()
    window.addEventListener('focus', onFocus)
    return () => window.removeEventListener('focus', onFocus)
  }, [])

  if (!perms.length) return null

  return (
    <Stack sx={{ gap: 1.25 }}>
      <Stack sx={{ gap: 0.25 }}>
        <Typography variant="body2" sx={{ fontWeight: 600 }}>
          Mac permissions
        </Typography>
        <Typography variant="caption" color="text.secondary">
          macOS keeps these switches in System Settings → Privacy &amp; Security. Vivianne can ask, but only you can turn them on.
        </Typography>
      </Stack>
      {webkitTooOld() && (
        <Typography variant="caption" sx={{ color: 'warning.main' }}>
          Your Safari is older than 15.4, and parts of Vivianne may look wrong. Update Safari from Software Update (macOS 10.15 can run 15.6).
        </Typography>
      )}
      {perms.map((p) => (
        <Stack key={p.id} direction="row" sx={{ gap: 1.5, alignItems: 'center' }}>
          <Box
            sx={{
              width: 8,
              height: 8,
              borderRadius: '50%',
              flexShrink: 0,
              bgcolor: p.state === 'granted' ? 'success.main' : p.state === 'denied' ? 'error.main' : 'text.disabled',
            }}
          />
          <Stack sx={{ flex: 1, minWidth: 0 }}>
            <Typography variant="body2" sx={{ fontWeight: 500 }}>
              {p.label}
            </Typography>
            <Typography variant="caption" color="text.secondary">
              {p.why}
            </Typography>
          </Stack>
          {p.state !== 'granted' && (
            <Button size="small" variant="outlined" onClick={() => void call('cmd_mac_request_permission', { id: p.id }).then(refresh)} sx={{ borderRadius: '8px', flexShrink: 0 }}>
              {p.state === 'denied' ? 'Allow' : 'Open'}
            </Button>
          )}
        </Stack>
      ))}
    </Stack>
  )
}
