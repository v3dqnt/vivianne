import { useEffect } from 'react'
import { ButtonBase, Typography } from '../../../ui'
import { ClipboardTextLinear } from '../../../icons'
import { usePlanStore } from '../store/planStore'
import { usePanelStore } from '../../dock/store/panelStore'

/**
 * Plan mode for the conversation. While on, the agent only researches — the
 * project, its dependencies, the web — and ends by proposing a plan in the
 * Plan panel for review. Nothing is changed until a plan is approved, which
 * turns plan mode off again by itself.
 */
export function PlanPill() {
  const on = usePlanStore((s) => s.planMode)
  const init = usePlanStore((s) => s.init)
  const setPlanMode = usePlanStore((s) => s.setPlanMode)

  useEffect(() => {
    void init()
  }, [init])

  return (
    <ButtonBase
      onClick={() => {
        void setPlanMode(!on)
        if (!on) usePanelStore.getState().openRight('plan')
      }}
      aria-pressed={on}
      title={on ? 'Plan mode — the agent researches and proposes a plan, and changes nothing. Click to turn off.' : 'Plan first: research and propose a plan for review before doing anything'}
      sx={{
        display: 'flex',
        alignItems: 'center',
        gap: 0.75,
        px: 1,
        py: 0.375,
        borderRadius: '8px',
        border: 1,
        borderColor: on ? 'primary.main' : 'divider',
        bgcolor: on ? 'action.selected' : 'transparent',
        color: on ? 'text.primary' : 'text.secondary',
      }}
    >
      <ClipboardTextLinear size={13} color="currentColor" />
      <Typography className="cq-label" variant="caption" sx={{ fontWeight: 500 }}>
        Plan
      </Typography>
    </ButtonBase>
  )
}
