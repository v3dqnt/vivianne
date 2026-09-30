import { Box, Stack, Typography } from '../../../ui'
import type { RunLedger } from '../lib/types'
import { formatDuration, formatTokens } from './RunStatusBar'

const ENDED: Partial<Record<RunLedger['status'], string>> = {
  done: 'Done',
  stopped: 'Stopped',
  paused: 'Paused',
  blocked: 'Needs you',
}

/**
 * What the last task took, under its final message: how long, how many
 * tokens, what it cost, how many steps and on which model. Shown once the
 * task has ended, so a finished piece of work reads with its receipt.
 */
export function RunSummary({ ledger }: { ledger: RunLedger }) {
  const tokensIn = ledger.inputTokens ?? 0
  const tokensOut = ledger.outputTokens ?? 0
  const parts = [
    `${ENDED[ledger.status] ?? 'Ended'} in ${formatDuration(Math.max(0, ledger.updatedAt - ledger.startedAt))}`,
    tokensIn + tokensOut > 0 ? `${formatTokens(tokensIn)} in · ${formatTokens(tokensOut)} out` : null,
    ledger.spentUsd > 0 ? `$${ledger.spentUsd.toFixed(ledger.spentUsd < 0.1 ? 3 : 2)}` : null,
    `${ledger.steps} step${ledger.steps === 1 ? '' : 's'}`,
    ledger.milestones.length ? `${ledger.milestones.length} checkpoint${ledger.milestones.length === 1 ? '' : 's'}` : null,
    ledger.model ? (ledger.model.split('/').pop() ?? ledger.model) : null,
  ].filter(Boolean)
  return (
    <Stack direction="row" sx={{ alignItems: 'center', gap: 1, px: 0.5 }}>
      <Box sx={{ width: 16, height: '1px', bgcolor: 'divider' }} />
      <Typography variant="caption" color="text.disabled" sx={{ fontSize: '0.6875rem', fontFamily: '"Geist Mono", ui-monospace, monospace' }}>
        {parts.join('  ·  ')}
      </Typography>
    </Stack>
  )
}
