import { useEffect, useRef, useState } from 'react'
import { Box, Stack, Typography, Button, ButtonBase, InputBase, Tooltip } from '../../../ui'
import { CloseLinear } from '../../../icons'
import { call } from '../../../ipc'
import { commentDraft, type PickResult } from './picker'

/**
 * The comment box that opens once an element is picked: what was picked, the
 * comment itself, and Send, which drops the whole thing into the chat composer
 * as a draft.
 */
export function PickerComposer({ pick, label, onDone }: { pick: PickResult; label: string; onDone: () => void }) {
  const [text, setText] = useState('')
  const [sending, setSending] = useState(false)
  const box = useRef<HTMLInputElement | null>(null)

  useEffect(() => box.current?.focus(), [])

  const send = async () => {
    if (!text.trim() || sending) return
    setSending(true)
    // Best effort: a driver that refuses the capture still leaves a comment
    // that names the element and the page.
    const shot = await call<string>('cmd_capture_region', { rect: pick.rect }).catch(() => null)
    window.dispatchEvent(new CustomEvent('viv:comment', { detail: commentDraft(pick, text, shot) }))
    setSending(false)
    onDone()
  }

  return (
    <Stack sx={{ gap: 0.75, p: 1, borderTop: 1, borderColor: 'divider', bgcolor: 'background.chrome', flexShrink: 0 }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: 0.5 }}>
        <Tooltip title={pick.url}>
          <ButtonBase
            onClick={() => void navigator.clipboard.writeText(pick.selector)}
            sx={{ gap: 0.5, px: 0.75, height: 20, borderRadius: '6px', bgcolor: 'action.selected', fontSize: '0.6875rem', fontFamily: '"Geist Mono", ui-monospace, monospace', color: 'text.secondary', maxWidth: 320 }}
          >
            <Box component="span" sx={{ whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
              {pick.selector}
            </Box>
            {pick.text && (
              <Box component="span" sx={{ whiteSpace: 'nowrap', color: 'text.disabled', overflow: 'hidden', textOverflow: 'ellipsis', maxWidth: 120 }}>
                “{pick.text}”
              </Box>
            )}
          </ButtonBase>
        </Tooltip>
        <Box sx={{ flex: 1 }} />
        <Tooltip title="Cancel">
          <Box component="button" type="button" aria-label="Cancel comment" onClick={onDone} sx={{ border: 0, bgcolor: 'transparent', color: 'text.secondary', p: 0.25, borderRadius: '4px', cursor: 'pointer', display: 'grid', '&:hover': { bgcolor: 'action.hover', color: 'text.primary' } }}>
            <CloseLinear size={11} color="currentColor" />
          </Box>
        </Tooltip>
      </Stack>
      <Stack direction="row" sx={{ alignItems: 'flex-end', gap: 0.75 }}>
        <InputBase
          inputRef={box}
          multiline
          maxRows={5}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey) {
              e.preventDefault()
              void send()
            }
            if (e.key === 'Escape') onDone()
          }}
          placeholder={`Comment on this element in ${label}…`}
          sx={{ flex: 1, fontSize: '0.75rem', bgcolor: 'action.hover', borderRadius: '8px', px: 1, py: 0.5 }}
        />
        <Button size="small" variant="contained" disableElevation disabled={!text.trim() || sending} onClick={() => void send()} sx={{ fontSize: '0.75rem', borderRadius: '8px', minWidth: 0 }}>
          Send
        </Button>
      </Stack>
      <Typography variant="caption" color="text.disabled" sx={{ fontSize: '0.625rem' }}>
        Goes to the chat composer as a draft — nothing is sent until you send it.
      </Typography>
    </Stack>
  )
}
