import { useEffect } from 'react'
import { ButtonBase, Typography } from '../../../ui'
// NOTE: import only icons that mx-icons actually exports at runtime — its
// bundled `.d.ts` declares many names its `dist/index.js` does not ship, so
// `tsc` will happily pass on a name that then fails the bundler.
import { ShieldTickLinear, MessageQuestionLinear } from '../../../icons'
import { usePermissionModeStore } from '../store/permissionModeStore'
import { TOOLBAR_PILL } from '../lib/toolbarPill'

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
      sx={{ ...TOOLBAR_PILL, ...(!bypassMode && { color: 'text.primary' }) }}
    >
      {bypassMode ? <ShieldTickLinear size={14} color="currentColor" /> : <MessageQuestionLinear size={13} color="currentColor" />}
      <Typography className="cq-label" variant="caption" sx={{ fontWeight: 500 }}>
        {bypassMode ? 'Auto' : 'Ask for approval'}
      </Typography>
    </ButtonBase>
  )
}
