import { Box, Stack, Typography } from '../../../ui'
import { DangerLinear } from '../../../icons'
import { TOOL_BLOCK_SX } from './ToolCallCard'
import { CHAT_META_SX, CHAT_SURFACE_SX, CHAT_TITLE_SX } from '../lib/chatStyle'

const when = (ts: number) =>
  new Date(ts).toLocaleString(undefined, { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' })

/** Where a turn stopped because it failed (rate limit, bad key, provider down):
 * a marker in the conversation like a checkpoint, in yellow rather than red,
 * since the work stopped here and can be picked up again. */
export function StoppedCard({ text, ts }: { text: string; ts: number }) {
  return (
    <Stack sx={TOOL_BLOCK_SX}>
      <Box sx={{ display: 'flex', alignItems: 'flex-start', gap: 1, px: 1.5, py: 1, minHeight: 38, ...CHAT_SURFACE_SX }}>
        <Box sx={{ display: 'flex', color: 'warning.main', pt: 0.125, flexShrink: 0 }}>
          <DangerLinear size={16} color="currentColor" />
        </Box>
        <Box sx={{ minWidth: 0, flex: 1 }}>
          <Stack direction="row" sx={{ alignItems: 'baseline', gap: 1, flexWrap: 'wrap' }}>
            <Typography variant="caption" sx={{ ...CHAT_TITLE_SX, color: 'warning.main' }}>
              Stopped here
            </Typography>
            <Typography variant="caption" color="text.disabled" sx={CHAT_META_SX}>
              {when(ts)}
            </Typography>
          </Stack>
          <Typography variant="caption" color="text.secondary" sx={{ display: 'block', lineHeight: 1.5, wordBreak: 'break-word' }}>
            {text || 'The model stopped without saying why.'}
          </Typography>
        </Box>
      </Box>
    </Stack>
  )
}
