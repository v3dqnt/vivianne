import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import CircularProgress from '@mui/material/CircularProgress'
import { RefreshLinear, ArchiveBookLinear, KeyLinear } from '../../../icons'
import { useAgentStore } from '../store/agentStore'

/**
 * `RetryStatus` (transient — clears itself on the next agent event, per
 * `agent/events.rs`) and `ModelFallback` (sticky for the rest of the turn)
 * were previously dropped by `agentStore` entirely: a retry loop or a
 * mid-turn model swap happened invisibly. Renders nothing when both are null.
 *
 * `memoryRecalled` covers a third previously-invisible thing: project memory
 * is injected into the system prompt silently by design (the model is told
 * not to narrate it) — this is the only place that ever shows it happened.
 */
export function StreamStatusLine() {
  const retryStatus = useAgentStore((s) => s.retryStatus)
  const modelFallback = useAgentStore((s) => s.modelFallback)
  const credentialRotated = useAgentStore((s) => s.credentialRotated)
  const memoryRecalled = useAgentStore((s) => s.memoryRecalled)

  if (!retryStatus && !modelFallback && !credentialRotated && !memoryRecalled) return null

  return (
    <Stack spacing={0.25} sx={{ px: 0.5 }}>
      {memoryRecalled !== null && memoryRecalled > 0 && (
        <Stack direction="row" spacing={0.75} sx={{ alignItems: 'center' }}>
          <Box sx={{ display: 'flex', color: 'text.secondary' }}>
            <ArchiveBookLinear size={12} color="currentColor" />
          </Box>
          <Typography variant="caption" color="text.secondary">
            Recalled {memoryRecalled} memor{memoryRecalled === 1 ? 'y' : 'ies'} from previous sessions
          </Typography>
        </Stack>
      )}
      {retryStatus && (
        <Stack direction="row" spacing={0.75} sx={{ alignItems: 'center' }}>
          <CircularProgress size={11} thickness={5} />
          <Typography variant="caption" color="text.secondary">
            Retrying ({retryStatus.attempt}/{retryStatus.maxAttempts}) — {retryStatus.reason}
          </Typography>
        </Stack>
      )}
      {credentialRotated && (
        <Stack direction="row" spacing={0.75} sx={{ alignItems: 'center' }}>
          <Box sx={{ display: 'flex', color: 'text.secondary' }}>
            <KeyLinear size={12} color="currentColor" />
          </Box>
          <Typography variant="caption" color="text.secondary">
            Switched key: {credentialRotated.fromLabel} → {credentialRotated.toLabel} ({credentialRotated.reason})
          </Typography>
        </Stack>
      )}
      {modelFallback && (
        <Stack direction="row" spacing={0.75} sx={{ alignItems: 'center' }}>
          <Box sx={{ display: 'flex', color: 'warning.main' }}>
            <RefreshLinear size={12} color="currentColor" />
          </Box>
          <Typography variant="caption" color="warning.main">
            Switched to {modelFallback.toModel} ({modelFallback.reason})
          </Typography>
        </Stack>
      )}
    </Stack>
  )
}
