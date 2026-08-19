import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import Box from '@mui/material/Box'
import { MonitorLinear } from '../../../icons'
import { useAgentStore } from '../store/agentStore'

/** Set/cleared by `agentStore` on the `computer` tool's start/end. */
export function ComputerUseBanner() {
  const active = useAgentStore((s) => s.computerUseActive)

  if (!active) return null

  return (
    <Stack
      direction="row"
      spacing={0.75}
      sx={{
        alignItems: 'center',
        px: 1.5,
        py: 0.5,
        borderRadius: 999,
        bgcolor: 'warning.main',
        alignSelf: 'flex-start',
      }}
    >
      <Box sx={{ display: 'flex', color: 'warning.contrastText' }}>
        <MonitorLinear size={13} color="currentColor" />
      </Box>
      <Typography variant="caption" sx={{ color: 'warning.contrastText', fontWeight: 600 }}>
        Vivianne is controlling your screen
      </Typography>
    </Stack>
  )
}
