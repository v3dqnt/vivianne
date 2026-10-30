import { useState } from 'react'
import { Box, ButtonBase, CircularProgress, Stack, Typography, alpha, useTheme } from '../../../ui'
import { CommandSquareLinear, DocumentTextLinear, FlashLinear, GlobalLinear, MailLinear, MessageLinear, MonitorLinear, WalletLinear } from '../../../icons'
import type { ApprovalWidget as Widget, WidgetKind } from '../lib/widgets'
import type { WidgetState } from '../lib/widgetLab'

const ICON: Record<WidgetKind, typeof FlashLinear> = {
  payment: WalletLinear,
  email: MailLinear,
  message: MessageLinear,
  shell: CommandSquareLinear,
  file: DocumentTextLinear,
  browser: GlobalLinear,
  screen: MonitorLinear,
  generic: FlashLinear,
}

/** What the approve button says, so the decision reads as the act itself. */
const VERB: Record<WidgetKind, string> = {
  payment: 'Pay',
  email: 'Send',
  message: 'Send',
  shell: 'Run',
  file: 'Allow',
  browser: 'Allow',
  screen: 'Allow',
  generic: 'Allow',
}

const OUTCOME: Record<Exclude<WidgetState, 'pending'>, string> = {
  approved: 'Approved',
  denied: 'Declined',
  expired: 'No answer in time',
}

/**
 * An agent asking before it acts, as a card in its chat: what it wants to
 * do, the details that decide it, and the two answers. Payment is drawn in
 * the warning colour. Once answered it stays in the chat, stamped.
 */
export function ApprovalWidget({
  widget,
  state,
  accent,
  onAnswer,
}: {
  widget: Widget
  state: WidgetState
  /** The agent's colour. */
  accent: string
  onAnswer?: (approved: boolean) => Promise<void> | void
}) {
  const theme = useTheme()
  const [busy, setBusy] = useState<null | boolean>(null)
  const tone = widget.danger === 'high' ? theme.palette.warning.main : accent
  const Icon = ICON[widget.kind]
  const pending = state === 'pending'

  const answer = async (yes: boolean) => {
    if (!onAnswer || busy !== null) return
    setBusy(yes)
    try {
      await onAnswer(yes)
    } finally {
      setBusy(null)
    }
  }

  return (
    <Box
      sx={{
        width: 'min(100%, 420px)',
        borderRadius: '18px',
        overflow: 'hidden',
        bgcolor: 'background.paper',
        border: 1,
        borderColor: pending ? alpha(tone, 0.45) : 'divider',
        boxShadow: pending ? `0 10px 30px ${alpha(tone, 0.16)}` : 'none',
        opacity: pending ? 1 : 0.82,
      }}
    >
      <Stack direction="row" sx={{ alignItems: 'center', gap: '10px', px: '16px', py: '12px', bgcolor: alpha(tone, pending ? 0.12 : 0.06) }}>
        <Box sx={{ width: 30, height: 30, borderRadius: '10px', display: 'grid', placeItems: 'center', bgcolor: alpha(tone, 0.2), color: tone, flexShrink: 0 }}>
          <Icon size={16} color="currentColor" />
        </Box>
        <Typography sx={{ flex: 1, minWidth: 0, fontSize: '14px', fontWeight: 600, lineHeight: 1.35 }}>{widget.title}</Typography>
      </Stack>

      {(widget.fields.length > 0 || widget.body) && (
        <Stack sx={{ px: '16px', pt: '12px', pb: widget.note ? '4px' : '12px', gap: '8px' }}>
          {widget.fields.map((f) => (
            <Stack key={f.label} direction="row" sx={{ gap: '12px', alignItems: 'baseline' }}>
              <Typography sx={{ width: 64, flexShrink: 0, fontSize: '12px', color: 'text.secondary' }}>{f.label}</Typography>
              <Typography
                sx={{ flex: 1, minWidth: 0, fontSize: f.mono ? '12px' : '13px', fontWeight: 500, fontFamily: f.mono ? '"Geist Mono", ui-monospace, monospace' : 'inherit', wordBreak: 'break-word', whiteSpace: 'pre-wrap' }}
              >
                {f.value}
              </Typography>
            </Stack>
          ))}
          {widget.body && (
            <Typography sx={{ fontSize: '13px', lineHeight: 1.5, color: 'text.secondary', whiteSpace: 'pre-wrap', px: '12px', py: '10px', borderRadius: '12px', bgcolor: 'action.hover', maxHeight: 160, overflow: 'auto' }}>
              {widget.body}
            </Typography>
          )}
        </Stack>
      )}
      {widget.note && <Typography sx={{ px: '16px', pb: '12px', fontSize: '12px', color: 'text.disabled' }}>{widget.note}</Typography>}

      {pending ? (
        <Stack direction="row" sx={{ gap: '8px', px: '12px', pb: '12px' }}>
          <ButtonBase
            onClick={() => void answer(false)}
            disabled={busy !== null || !onAnswer}
            sx={{ flex: 1, height: 38, borderRadius: '12px', fontSize: '13px', fontWeight: 600, bgcolor: 'action.hover', '&:hover': { bgcolor: 'action.selected' } }}
          >
            {busy === false ? <CircularProgress size={14} /> : 'Decline'}
          </ButtonBase>
          <ButtonBase
            onClick={() => void answer(true)}
            disabled={busy !== null || !onAnswer}
            sx={{ flex: 1, height: 38, borderRadius: '12px', fontSize: '13px', fontWeight: 600, bgcolor: tone, color: theme.palette.getContrastText(tone), '&:hover': { filter: 'brightness(1.08)' }, '&:active': { transform: 'scale(0.98)' } }}
          >
            {busy === true ? <CircularProgress size={14} sx={{ color: 'inherit' }} /> : VERB[widget.kind]}
          </ButtonBase>
        </Stack>
      ) : (
        <Typography
          sx={{ px: '16px', py: '10px', borderTop: 1, borderColor: 'divider', fontSize: '12px', fontWeight: 600, color: state === 'approved' ? 'success.main' : state === 'denied' ? 'text.secondary' : 'warning.main' }}
        >
          {OUTCOME[state]}
        </Typography>
      )}
    </Box>
  )
}
