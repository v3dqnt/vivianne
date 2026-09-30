import { useEffect } from 'react'
import { Stack, Typography, Button, alpha, useTheme } from '../../../ui'
import { useAgentStore } from '../store/agentStore'
import { useDaysStore, type Viewing } from '../../sessions/store/daysStore'
import { MessageList } from './MessageList'
import { Composer } from './Composer'
import { PermissionDialog } from './PermissionDialog'
import { AskQuestionDialog } from './AskQuestionDialog'
import { ConfirmationDialog } from './ConfirmationDialog'

export function ChatView() {
  const init = useAgentStore((s) => s.init)
  const viewing = useDaysStore((s) => s.viewing)
  const earlier = useDaysStore((s) => s.earlier)
  const earlierStart = useDaysStore((s) => s.earlierStart)
  const loadingOlder = useDaysStore((s) => s.loadingOlder)
  const loadOlder = useDaysStore((s) => s.loadOlder)

  useEffect(() => {
    void init()
  }, [init])

  return (
    <Stack sx={{ height: '100%', minHeight: 0 }}>
      {viewing ? (
        <HistoryView viewing={viewing} />
      ) : (
        <>
          <MessageList earlier={earlier} earlierStart={earlierStart} onLoadOlder={() => void loadOlder()} loadingOlder={loadingOlder} />
          <Composer />
        </>
      )}
      {/* Mounted either way: a permission prompt from the running turn must
          still reach the user while they are reading an old thread. */}
      <PermissionDialog />
      <AskQuestionDialog />
      <ConfirmationDialog />
    </Stack>
  )
}

/**
 * An archived thread from before the single conversation, read-only.
 *
 * Takes the Composer's place rather than sitting beside it: a live text box
 * under an old transcript would invite typing into it and send the message
 * to the current conversation with no sign of it.
 */
function HistoryView({ viewing }: { viewing: Viewing }) {
  const theme = useTheme()
  const backToToday = useDaysStore((s) => s.backToToday)
  const busy = useAgentStore((s) => s.isBusy)

  return (
    <>
      <Stack
        direction="row"
        sx={{
          alignItems: 'center',
          gap: 1.5,
          px: 2,
          py: 1,
          borderBottom: 1,
          borderColor: 'divider',
          bgcolor: alpha(theme.palette.text.primary, 0.03),
        }}
      >
        <Stack sx={{ minWidth: 0, flex: 1 }}>
          <Typography variant="body2" sx={{ fontWeight: 600 }} noWrap>
            {viewing.label}
          </Typography>
          <Typography variant="caption" color="text.secondary">
            Archived thread · read-only
            {busy ? ' · the conversation is still working' : ''}
          </Typography>
        </Stack>
        <Button
          size="small"
          variant="contained"
          disableElevation
          onClick={backToToday}
          sx={{ textTransform: 'none', flexShrink: 0 }}
        >
          Back to the conversation
        </Button>
      </Stack>
      <MessageList messages={viewing.messages} readOnly sessionId={viewing.id} />
    </>
  )
}
