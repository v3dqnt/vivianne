import { useEffect, useState } from 'react'
import Dialog from '@mui/material/Dialog'
import DialogTitle from '@mui/material/DialogTitle'
import DialogContent from '@mui/material/DialogContent'
import DialogActions from '@mui/material/DialogActions'
import Button from '@mui/material/Button'
import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import Chip from '@mui/material/Chip'
import { MessageQuestionLinear } from '../../../icons'
import { useAgentStore } from '../store/agentStore'

/** Backend-driven structured clarification — mirrors the same "ask user
 * with options" pattern used elsewhere: the model pauses mid-turn instead of
 * guessing, the frontend renders 1-4 questions each with selectable options. */
export function AskQuestionDialog() {
  const pending = useAgentStore((s) => s.pendingQuestion)
  const answerQuestion = useAgentStore((s) => s.answerQuestion)

  // One selected-labels array per question, indexed the same as `pending.questions`.
  const [selections, setSelections] = useState<string[][]>([])

  useEffect(() => {
    setSelections(pending ? pending.questions.map(() => []) : [])
  }, [pending])

  if (!pending) return null

  const toggle = (qIndex: number, label: string, multiSelect: boolean) => {
    setSelections((prev) => {
      const next = prev.map((s) => [...s])
      const current = next[qIndex] ?? []
      if (multiSelect) {
        next[qIndex] = current.includes(label) ? current.filter((l) => l !== label) : [...current, label]
      } else {
        next[qIndex] = current.includes(label) ? [] : [label]
      }
      return next
    })
  }

  const canSubmit = selections.length > 0 && selections.every((s) => s.length > 0)

  return (
    <Dialog open maxWidth="sm" fullWidth>
      <DialogTitle>
        <Stack direction="row" spacing={1} sx={{ alignItems: 'center' }}>
          <Box sx={{ display: 'flex', color: 'info.main' }}>
            <MessageQuestionLinear size={20} color="currentColor" />
          </Box>
          <Typography variant="subtitle1" sx={{ fontWeight: 600 }}>
            {pending.questions.length === 1 ? pending.questions[0].header : 'A few questions'}
          </Typography>
        </Stack>
      </DialogTitle>
      <DialogContent>
        <Stack spacing={2.5}>
          {pending.questions.map((q, qIndex) => (
            <Box key={qIndex}>
              {pending.questions.length > 1 && (
                <Typography variant="overline" color="text.secondary">
                  {q.header}
                </Typography>
              )}
              <Typography variant="body2" sx={{ mb: 1, fontWeight: 500 }}>
                {q.question}
              </Typography>
              <Stack spacing={1}>
                {q.options.map((opt) => {
                  const selected = (selections[qIndex] ?? []).includes(opt.label)
                  return (
                    <Box
                      key={opt.label}
                      onClick={() => toggle(qIndex, opt.label, q.multiSelect)}
                      sx={{
                        display: 'flex',
                        alignItems: 'center',
                        gap: 1,
                        px: 1.5,
                        py: 1,
                        borderRadius: 2,
                        border: 1,
                        borderColor: selected ? 'primary.main' : 'divider',
                        bgcolor: selected ? 'action.selected' : 'transparent',
                        cursor: 'pointer',
                        '&:hover': { bgcolor: selected ? 'action.selected' : 'action.hover' },
                      }}
                    >
                      <Box sx={{ flex: 1, minWidth: 0 }}>
                        <Typography variant="body2" sx={{ fontWeight: 500 }}>
                          {opt.label}
                        </Typography>
                        {opt.description && (
                          <Typography variant="caption" color="text.secondary">
                            {opt.description}
                          </Typography>
                        )}
                      </Box>
                      {selected && <Chip size="small" color="primary" label="Selected" />}
                    </Box>
                  )
                })}
              </Stack>
            </Box>
          ))}
        </Stack>
      </DialogContent>
      <DialogActions>
        <Button disabled={!canSubmit} onClick={() => void answerQuestion(selections)} variant="contained">
          Submit
        </Button>
      </DialogActions>
    </Dialog>
  )
}
