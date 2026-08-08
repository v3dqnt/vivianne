import { useState } from 'react'
import Box from '@mui/material/Box'
import LinearProgress from '@mui/material/LinearProgress'
import Tooltip from '@mui/material/Tooltip'
import Typography from '@mui/material/Typography'
import IconButton from '@mui/material/IconButton'
import CircularProgress from '@mui/material/CircularProgress'
import Snackbar from '@mui/material/Snackbar'
import { ArchiveLinear } from 'mx-icons'
import { useModelStore } from '../store/modelStore'
import { useAgentStore } from '../store/agentStore'

function formatTokens(n: number): string {
  if (n >= 1000) return `${(n / 1000).toFixed(1)}K`
  return String(n)
}

export function ContextUsageBar() {
  const stats = useModelStore((s) => s.stats)
  const compacting = useAgentStore((s) => s.compacting)
  const isBusy = useAgentStore((s) => s.isBusy)
  const messageCount = useAgentStore((s) => s.messages.length)
  const compactNow = useAgentStore((s) => s.compactNow)
  const [toast, setToast] = useState<string | null>(null)

  if (!stats) return null

  // `contextPercent` comes through as NaN before a model is actually
  // selected (0 tokens / 0 window on the backend's default ModelRef) — guard
  // rather than render the literal string "NaN%".
  const pct = Number.isFinite(stats.contextPercent) ? Math.min(100, stats.contextPercent) : 0
  const color = pct > 90 ? 'error' : pct > 70 ? 'warning' : 'primary'

  const runCompact = async () => {
    const folded = await compactNow()
    setToast(folded > 0 ? `Compacted ${folded} messages into a summary.` : 'Nothing to compact yet.')
  }

  return (
    <>
      <Box sx={{ display: 'flex', alignItems: 'center', gap: 0.5, px: 0.5 }}>
        <Tooltip
          title={`${formatTokens(stats.contextTokens)} / ${formatTokens(stats.contextWindow)} tokens · ${stats.inputTokens} in / ${stats.outputTokens} out this session`}
        >
          <Box sx={{ display: 'flex', alignItems: 'center', gap: 1, minWidth: 90 }}>
            <LinearProgress
              variant="determinate"
              value={pct}
              color={color}
              sx={{ flex: 1, height: 4, borderRadius: 2, bgcolor: 'action.hover' }}
            />
            <Typography variant="caption" color="text.secondary" sx={{ fontFamily: 'monospace', fontSize: '0.6875rem' }}>
              {Math.round(pct)}%
            </Typography>
          </Box>
        </Tooltip>

        <Tooltip title={isBusy ? 'Wait for the current turn to finish' : 'Compact context — fold older history into a summary'}>
          {/* Span wrapper: a disabled button fires no events, so Tooltip
              needs a non-disabled element to attach its listeners to. */}
          <Box component="span" sx={{ display: 'flex' }}>
            <IconButton
              size="small"
              onClick={() => void runCompact()}
              disabled={compacting || isBusy || messageCount === 0}
              sx={{ color: 'text.secondary', '&:hover': { color: 'text.primary' } }}
            >
              {compacting ? <CircularProgress size={13} /> : <ArchiveLinear size={14} color="currentColor" />}
            </IconButton>
          </Box>
        </Tooltip>
      </Box>

      <Snackbar
        open={!!toast}
        autoHideDuration={3000}
        onClose={() => setToast(null)}
        message={toast}
        anchorOrigin={{ vertical: 'bottom', horizontal: 'center' }}
      />
    </>
  )
}
