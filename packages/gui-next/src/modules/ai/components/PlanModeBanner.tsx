import { useEffect } from 'react'
import { Box, Stack, Typography } from '../../../ui'
import { ClipboardTextLinear } from '../../../icons'
import { usePlanModeStore } from '../store/planModeStore'

/** Driven by `agentStore` calling `refresh()` on `enter_plan_mode`/
 * `exit_plan_mode` — this component just renders whatever that store holds,
 * plus a one-time fetch on mount so a plan already active from a previous
 * turn shows up immediately instead of waiting for the next tool call. */
export function PlanModeBanner() {
  const active = usePlanModeStore((s) => s.active)
  const plan = usePlanModeStore((s) => s.plan)
  const refresh = usePlanModeStore((s) => s.refresh)

  useEffect(() => {
    void refresh()
  }, [refresh])

  if (!active) return null

  return (
    <Stack
      direction="row"
      spacing={1}
      sx={{
        alignItems: 'flex-start',
        px: 1.5,
        py: 1,
        borderRadius: 2,
        border: 1,
        borderColor: 'info.main',
        bgcolor: 'rgba(41,121,255,0.08)',
      }}
    >
      <Box sx={{ display: 'flex', color: 'info.main', pt: 0.25 }}>
        <ClipboardTextLinear size={15} color="currentColor" />
      </Box>
      <Stack sx={{ minWidth: 0 }}>
        <Typography variant="caption" sx={{ fontWeight: 600, color: 'info.main' }}>
          Plan mode — mutating tools are off
        </Typography>
        {plan && (
          <Typography variant="caption" color="text.secondary" sx={{ whiteSpace: 'pre-wrap' }}>
            {plan}
          </Typography>
        )}
      </Stack>
    </Stack>
  )
}
