import { useEffect, useState } from 'react'
import { ButtonBase, Popover, Stack, Typography, alpha, useTheme } from '../../../ui'
import { ArrowDownLinear, ClipboardTextLinear, MessageQuestionLinear, ShieldTickLinear, TickCircleLinear } from '../../../icons'
import { usePermissionModeStore } from '../store/permissionModeStore'
import { usePlanStore } from '../../plan/store/planStore'
import { usePanelStore } from '../../dock/store/panelStore'

type Mode = 'ask' | 'auto' | 'plan'

const MODES: { id: Mode; label: string; hint: string; Icon: typeof ShieldTickLinear }[] = [
  { id: 'ask', label: 'Ask for approval', hint: 'Pauses before a tool that changes things, so you approve each one.', Icon: MessageQuestionLinear },
  { id: 'auto', label: 'Auto', hint: 'Acts without asking. Files, commands and the browser run as the agent decides.', Icon: ShieldTickLinear },
  { id: 'plan', label: 'Plan', hint: 'Researches and proposes a plan for review. Changes nothing until you approve it.', Icon: ClipboardTextLinear },
]

/**
 * How much the agent may do on its own, as one control: ask first, act
 * freely, or plan before doing anything. Plan mode wins while it is on; the
 * other two are the approval setting.
 */
export function ModePill() {
  const theme = useTheme()
  const bypass = usePermissionModeStore((s) => s.bypassMode)
  const refreshPerm = usePermissionModeStore((s) => s.refresh)
  const togglePerm = usePermissionModeStore((s) => s.toggle)
  const planOn = usePlanStore((s) => s.planMode)
  const initPlan = usePlanStore((s) => s.init)
  const setPlanMode = usePlanStore((s) => s.setPlanMode)
  const [anchor, setAnchor] = useState<HTMLElement | null>(null)

  useEffect(() => {
    void refreshPerm()
    void initPlan()
  }, [refreshPerm, initPlan])

  const current: Mode = planOn ? 'plan' : bypass ? 'auto' : 'ask'
  const shown = MODES.find((m) => m.id === current)!

  const choose = async (mode: Mode) => {
    setAnchor(null)
    if (mode === current) return
    if (mode === 'plan') {
      await setPlanMode(true)
      usePanelStore.getState().openRight('plan')
      return
    }
    if (planOn) await setPlanMode(false)
    // The approval setting is a flip; only flip it when it differs.
    if ((mode === 'auto') !== bypass) await togglePerm()
  }

  return (
    <>
      <ButtonBase
        onClick={(e) => setAnchor(e.currentTarget)}
        aria-haspopup="listbox"
        aria-expanded={!!anchor}
        title={`${shown.label}: ${shown.hint}`}
        sx={{
          display: 'flex',
          alignItems: 'center',
          gap: 0.75,
          px: 1,
          py: 0.375,
          borderRadius: '8px',
          border: 1,
          borderColor: current === 'plan' ? 'primary.main' : 'divider',
          bgcolor: current === 'plan' ? 'action.selected' : 'transparent',
          color: current === 'ask' ? 'text.primary' : 'text.secondary',
          '&:hover': { bgcolor: 'action.hover' },
        }}
      >
        <shown.Icon size={13} color="currentColor" />
        <Typography className="cq-label" variant="caption" sx={{ fontWeight: 500 }}>
          {shown.label}
        </Typography>
        <ArrowDownLinear size={11} color="currentColor" />
      </ButtonBase>
      <Popover
        open={!!anchor}
        anchorEl={anchor}
        onClose={() => setAnchor(null)}
        anchorOrigin={{ vertical: 'top', horizontal: 'left' }}
        transformOrigin={{ vertical: 'bottom', horizontal: 'left' }}
        slotProps={{ paper: { sx: { width: 300, borderRadius: '14px', p: 0.5, mt: -0.5 } } }}
      >
        <Stack role="listbox" sx={{ gap: 0.25 }}>
          {MODES.map((m) => {
            const on = m.id === current
            return (
              <ButtonBase
                key={m.id}
                role="option"
                aria-selected={on}
                onClick={() => void choose(m.id)}
                sx={{
                  display: 'flex',
                  alignItems: 'flex-start',
                  justifyContent: 'flex-start',
                  textAlign: 'left',
                  gap: 1.25,
                  px: 1.25,
                  py: 1,
                  borderRadius: '10px',
                  bgcolor: on ? alpha(theme.palette.primary.main, 0.1) : 'transparent',
                  '&:hover': { bgcolor: on ? alpha(theme.palette.primary.main, 0.14) : 'action.hover' },
                }}
              >
                <Stack sx={{ color: on ? 'primary.main' : 'text.secondary', pt: 0.25 }}>
                  <m.Icon size={15} color="currentColor" />
                </Stack>
                <Stack sx={{ flex: 1, minWidth: 0, gap: 0.125 }}>
                  <Typography variant="body2" sx={{ fontWeight: 600, fontSize: '0.8125rem' }}>
                    {m.label}
                  </Typography>
                  <Typography variant="caption" color="text.secondary" sx={{ lineHeight: 1.4 }}>
                    {m.hint}
                  </Typography>
                </Stack>
                {on && (
                  <Stack sx={{ color: 'primary.main', pt: 0.25 }}>
                    <TickCircleLinear size={14} color="currentColor" />
                  </Stack>
                )}
              </ButtonBase>
            )
          })}
        </Stack>
      </Popover>
    </>
  )
}
