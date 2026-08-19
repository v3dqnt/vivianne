import { useEffect } from 'react'
import ButtonBase from '@mui/material/ButtonBase'
import Typography from '@mui/material/Typography'
// NOTE: import only icons that mx-icons actually exports at runtime — its
// bundled `.d.ts` declares many names its `dist/index.js` does not ship, so
// `tsc` will happily pass on a name that then fails the bundler.
import { ShieldTickLinear, MessageQuestionLinear } from '../../../icons'
import { usePermissionModeStore } from '../store/permissionModeStore'

/** Toggles the same `bypassMode` flag the old app's AUTO/PERM control wrote — auto skips tool-call approval, "Ask for approval" pauses for it. */
export function PermissionModePill() {
  const bypassMode = usePermissionModeStore((s) => s.bypassMode)
  const refresh = usePermissionModeStore((s) => s.refresh)
  const toggle = usePermissionModeStore((s) => s.toggle)

  useEffect(() => {
    void refresh()
  }, [refresh])

  return (
    <ButtonBase
      onClick={() => void toggle()}
      title={bypassMode ? 'Auto mode — click to require approval for tool calls' : 'Click to let the agent act without asking'}
      sx={{
        display: 'flex',
        alignItems: 'center',
        gap: 0.75,
        px: 1.25,
        py: 0.5,
        borderRadius: 999,
        border: 1,
        borderColor: bypassMode ? 'transparent' : 'divider',
        bgcolor: bypassMode ? 'rgba(255,255,255,0.05)' : 'transparent',
        color: bypassMode ? 'text.secondary' : 'text.primary',
      }}
    >
      {bypassMode ? <ShieldTickLinear size={13} color="currentColor" /> : <MessageQuestionLinear size={13} color="currentColor" />}
      <Typography variant="caption" sx={{ fontWeight: 500 }}>
        {bypassMode ? 'Auto' : 'Ask for approval'}
      </Typography>
    </ButtonBase>
  )
}
