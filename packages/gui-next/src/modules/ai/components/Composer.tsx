import { useState, type KeyboardEvent } from 'react'
import Box from '@mui/material/Box'
import Paper from '@mui/material/Paper'
import Stack from '@mui/material/Stack'
import TextField from '@mui/material/TextField'
import IconButton from '@mui/material/IconButton'
import CircularProgress from '@mui/material/CircularProgress'
import { SendLinear, CloseCircleLinear } from 'mx-icons'
import { useAgentStore } from '../store/agentStore'
import type { ImageAttachment } from '../lib/types'
import { ModelSelector } from './ModelSelector'
import { ThinkingLevelControl } from './ThinkingLevelControl'
import { WorkspaceSelector } from './WorkspaceSelector'
import { BranchIndicator } from './BranchIndicator'
import { PermissionModePill } from './PermissionModePill'
import { AttachmentButton } from './AttachmentButton'
import { ContextUsageBar } from './ContextUsageBar'

export function Composer() {
  const [text, setText] = useState('')
  const [pending, setPending] = useState<ImageAttachment[]>([])
  const isBusy = useAgentStore((s) => s.isBusy)
  const sendMessage = useAgentStore((s) => s.sendMessage)

  const submit = () => {
    if ((!text.trim() && pending.length === 0) || isBusy) return
    const toSend = text
    const images = pending
    setText('')
    setPending([])
    void sendMessage(toSend, images)
  }

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      submit()
    }
  }

  return (
    // The gap on all sides (not just top) plus each panel's own radius/shadow
    // is what reads as "floating" rather than a bar glued to the window edge.
    <Box sx={{ flexShrink: 0, px: 2, pb: 2, pt: 0.5 }}>
      <Stack sx={{ gap: 0.5 }}>
        {/* Context strip — informational, sits above the input as its own
            small surface rather than crowding the composer's own toolbar. */}
        <Stack
          direction="row"
          sx={{
            alignSelf: 'flex-start',
            alignItems: 'center',
            gap: 0.5,
            px: 0.5,
            py: 0.25,
            borderRadius: 999,
            bgcolor: 'rgba(255,255,255,0.04)',
          }}
        >
          <WorkspaceSelector />
          <BranchIndicator />
        </Stack>

        <Paper
          elevation={0}
          sx={{
            borderRadius: 3,
            border: 1,
            borderColor: 'rgba(255,255,255,0.08)',
            bgcolor: 'rgba(255,255,255,0.045)',
            backdropFilter: 'blur(20px) saturate(150%)',
            WebkitBackdropFilter: 'blur(20px) saturate(150%)',
            boxShadow: '0 12px 32px rgba(0,0,0,0.4), inset 0 1px 0 rgba(255,255,255,0.06)',
            overflow: 'hidden',
          }}
        >
          {pending.length > 0 && (
            <Stack direction="row" spacing={1} sx={{ px: 2, pt: 1.5, flexWrap: 'wrap' }}>
              {pending.map((img, i) => (
                <Box key={i} sx={{ position: 'relative' }}>
                  <Box
                    component="img"
                    src={`data:${img.media_type};base64,${img.base64}`}
                    sx={{ width: 56, height: 56, objectFit: 'cover', borderRadius: 2, border: 1, borderColor: 'rgba(255,255,255,0.1)' }}
                  />
                  <IconButton
                    size="small"
                    onClick={() => setPending((p) => p.filter((_, j) => j !== i))}
                    sx={{ position: 'absolute', top: -8, right: -8, bgcolor: 'background.paper', p: 0.25, '&:hover': { bgcolor: 'background.paper' } }}
                  >
                    <CloseCircleLinear size={16} color="currentColor" />
                  </IconButton>
                </Box>
              ))}
            </Stack>
          )}

          <TextField
            fullWidth
            multiline
            minRows={2}
            maxRows={8}
            placeholder="Do anything"
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={onKeyDown}
            variant="standard"
            disabled={isBusy}
            slotProps={{ input: { disableUnderline: true } }}
            sx={{ px: 2, pt: 1.75, pb: 0.5, '& .MuiInputBase-root': { fontSize: '0.9375rem' } }}
          />

          <Stack direction="row" sx={{ alignItems: 'center', gap: 0.75, px: 1.5, pb: 1.25, pt: 0.5 }}>
            <AttachmentButton onAdd={(imgs) => setPending((p) => [...p, ...imgs])} />
            <PermissionModePill />

            <Box sx={{ flex: 1 }} />

            <ContextUsageBar />
            <ThinkingLevelControl />
            <ModelSelector />

            <IconButton
              onClick={submit}
              disabled={isBusy || (!text.trim() && pending.length === 0)}
              sx={{
                bgcolor: 'action.selected',
                color: 'text.primary',
                width: 32,
                height: 32,
                flexShrink: 0,
                transition: 'transform 0.1s ease',
                '&:hover': { bgcolor: 'primary.main', color: 'primary.contrastText' },
                '&:active': { transform: 'scale(0.92)' },
                '&.Mui-disabled': { bgcolor: 'action.disabledBackground', color: 'text.disabled' },
              }}
            >
              {isBusy ? <CircularProgress size={15} sx={{ color: 'inherit' }} /> : <SendLinear size={15} color="currentColor" />}
            </IconButton>
          </Stack>
        </Paper>
      </Stack>
    </Box>
  )
}
