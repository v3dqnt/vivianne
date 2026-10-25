import { useMemo, useState } from 'react'
import { Alert, Box, ButtonBase, Stack, Tooltip, Typography, alpha, useTheme } from '../../ui'
import { UndoLinear, Hierarchy2Linear, DocumentTextLinear, ClockLinear } from '../../icons'
import { call } from '../../ipc'
import { useAgentStore } from '../ai/store/agentStore'
import { useDaysStore } from '../sessions/store/daysStore'
import { contentToText } from '../ai/lib/types'
import type { AgentMessage } from '../ai/lib/types'
import { isHandoffText, isSummaryText, isPairActivityText, isResumeText, isPlanNoteText } from '../ai/components/HandoffDivider'
import { baseName } from '../ai/components/FileLink'

/** One prompt and everything the agent did in answer to it. */
interface Step {
  /** Index of the user's message. */
  at: number
  /** Index just past the last message of this step. */
  end: number
  prompt: string
  time: number
  tools: Record<string, number>
  files: string[]
  toolCallIds: string[]
  failed: number
}

const EDIT_TOOLS = new Set(['write', 'edit', 'apply_patch', 'notebook_edit', 'edit_document', 'create_document'])

/** The transcript as steps: a real prompt and what followed it. Synthetic
 * notes that ride in user messages (handoffs, summaries) aren't steps. */
export function stepsOf(messages: AgentMessage[]): Step[] {
  const out: Step[] = []
  messages.forEach((m, i) => {
    if (m.type === 'user') {
      const text = contentToText(m.content).trim()
      const synthetic = isHandoffText(text) || isSummaryText(text) || isPairActivityText(text) || isResumeText(text) || isPlanNoteText(text)
      if (text && !synthetic) {
        out.push({ at: i, end: messages.length, prompt: text, time: m.timestamp, tools: {}, files: [], toolCallIds: [], failed: 0 })
      }
      return
    }
    const cur = out[out.length - 1]
    if (!cur) return
    if (m.type === 'toolResult') {
      cur.tools[m.toolName] = (cur.tools[m.toolName] ?? 0) + 1
      cur.toolCallIds.push(m.toolCallId)
      if (m.isError === true) cur.failed++
      if (EDIT_TOOLS.has(m.toolName) && m.isError === false) {
        const a = m.arguments ?? {}
        const p = ['path', 'file_path', 'notebook_path'].map((k) => a[k]).find((v) => typeof v === 'string') as string | undefined
        if (p && !cur.files.includes(p)) cur.files.push(p)
      }
    }
  })
  for (let k = 0; k < out.length - 1; k++) out[k].end = out[k + 1].at
  return out
}

function ago(ms: number): string {
  const d = new Date(ms)
  return d.toLocaleString(undefined, { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' })
}

function toolSummary(t: Record<string, number>): string {
  const n = Object.values(t).reduce((a, b) => a + b, 0)
  if (n === 0) return 'Answered'
  const top = Object.entries(t)
    .sort((a, b) => b[1] - a[1])
    .slice(0, 3)
    .map(([k, v]) => `${v} ${k.replace(/_/g, ' ')}`)
  return top.join(' · ') + (Object.keys(t).length > 3 ? ' …' : '')
}

/** The run so far, one row per prompt, with ways back: rewind the
 * conversation to before it, start a new thread from just after it, or put
 * back the files it changed. */
export function TimelinePanel() {
  const theme = useTheme()
  const messages = useAgentStore((s) => s.messages)
  const busy = useAgentStore((s) => s.isBusy)
  const revertToIndex = useAgentStore((s) => s.revertToIndex)
  const steps = useMemo(() => stepsOf(messages), [messages])
  const [note, setNote] = useState<{ tone: 'success' | 'error' | 'info'; text: string } | null>(null)

  const rewind = async (s: Step) => {
    setNote(null)
    await revertToIndex(s.at)
    setNote({ tone: 'info', text: `Rewound to before “${s.prompt.slice(0, 50)}”.` })
  }

  const fork = async (s: Step) => {
    setNote(null)
    try {
      const id = await call<string>('cmd_fork_session_at', { index: s.end })
      await useDaysStore.getState().refreshThreads()
      if (id) await useDaysStore.getState().openThread(id)
      setNote({ tone: 'success', text: 'Started a new thread from that point.' })
    } catch (e) {
      setNote({ tone: 'error', text: e instanceof Error ? e.message : String(e) })
    }
  }

  const restoreFiles = async (s: Step) => {
    setNote(null)
    try {
      const all = (await call<{ id: string; toolCallId: string; createdAtMs: number }[]>('cmd_list_checkpoints', {})) ?? []
      const mine = all.filter((c) => s.toolCallIds.includes(c.toolCallId)).sort((a, b) => b.createdAtMs - a.createdAtMs)
      if (mine.length === 0) {
        setNote({ tone: 'info', text: 'No file snapshots were kept for this step.' })
        return
      }
      // Newest first, so the oldest snapshot (the files as they were) wins.
      let written = 0
      for (const c of mine) {
        const r = await call<{ restored?: string[]; written?: string[] }>('cmd_restore_checkpoint', { checkpointId: c.id })
        written += (r?.restored ?? r?.written ?? []).length
      }
      setNote({ tone: 'success', text: `Put ${written || 'the'} file${written === 1 ? '' : 's'} back as they were before this step.` })
    } catch (e) {
      setNote({ tone: 'error', text: e instanceof Error ? e.message : String(e) })
    }
  }

  const act = { height: 24, px: 0.875, borderRadius: '7px', fontSize: '0.75rem', gap: 0.5, color: 'text.secondary', '&:hover': { bgcolor: 'action.hover', color: 'text.primary' }, '&:disabled': { opacity: 0.4 } } as const

  return (
    <Stack sx={{ height: '100%', minHeight: 0 }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1, px: 2, py: 1.25, borderBottom: 1, borderColor: 'divider', flexShrink: 0 }}>
        <ClockLinear size={15} color="currentColor" />
        <Typography sx={{ fontSize: '0.875rem', fontWeight: 600 }}>Timeline</Typography>
        <Typography sx={{ fontSize: '0.75rem', color: 'text.secondary' }}>
          {steps.length} step{steps.length === 1 ? '' : 's'}
        </Typography>
      </Stack>
      {note && (
        <Box sx={{ px: 1.5, pt: 1 }}>
          <Alert severity={note.tone}>{note.text}</Alert>
        </Box>
      )}
      <Box sx={{ flex: 1, minHeight: 0, overflowY: 'auto', px: 1.5, py: 1.25 }}>
        {steps.length === 0 ? (
          <Typography sx={{ fontSize: '0.8125rem', color: 'text.secondary', p: 1 }}>Nothing yet. Each prompt you send shows up here, with ways to go back to it.</Typography>
        ) : (
          <Box sx={{ position: 'relative', pl: 2.25 }}>
            <Box sx={{ position: 'absolute', left: 5, top: 8, bottom: 8, width: '2px', bgcolor: 'divider', borderRadius: 1 }} />
            {steps.map((s, i) => (
              <Box key={s.at} sx={{ position: 'relative', pb: 1.75 }}>
                <Box sx={{ position: 'absolute', left: -20, top: 5, width: 10, height: 10, borderRadius: '50%', bgcolor: i === steps.length - 1 ? 'primary.main' : 'text.disabled', border: 2, borderColor: 'background.paper' }} />
                <Typography sx={{ fontSize: '0.6875rem', color: 'text.disabled' }}>{ago(s.time)}</Typography>
                <Typography sx={{ fontSize: '0.8125rem', fontWeight: 600, lineHeight: 1.45, display: '-webkit-box', WebkitLineClamp: 3, WebkitBoxOrient: 'vertical', overflow: 'hidden', whiteSpace: 'pre-wrap' }}>
                  {s.prompt}
                </Typography>
                <Typography sx={{ fontSize: '0.75rem', color: 'text.secondary', mt: 0.25 }}>
                  {toolSummary(s.tools)}
                  {s.failed > 0 ? <Box component="span" sx={{ color: 'error.main' }}> · {s.failed} failed</Box> : null}
                </Typography>
                {s.files.length > 0 && (
                  <Stack direction="row" sx={{ gap: 0.5, flexWrap: 'wrap', mt: 0.5 }}>
                    {s.files.slice(0, 6).map((f) => (
                      <Box key={f} component="span" title={f} sx={{ display: 'inline-flex', alignItems: 'center', gap: 0.5, px: 0.75, height: 20, borderRadius: '6px', fontSize: '0.6875rem', bgcolor: alpha(theme.palette.primary.main, 0.1), color: 'primary.main' }}>
                        <DocumentTextLinear size={11} color="currentColor" />
                        {baseName(f)}
                      </Box>
                    ))}
                    {s.files.length > 6 && <Typography sx={{ fontSize: '0.6875rem', color: 'text.disabled', alignSelf: 'center' }}>+{s.files.length - 6}</Typography>}
                  </Stack>
                )}
                <Stack direction="row" sx={{ gap: 0.25, mt: 0.5, ml: -0.5, flexWrap: 'wrap' }}>
                  <Tooltip title="Put the conversation back to just before this prompt">
                    <span>
                      <ButtonBase disabled={busy} onClick={() => void rewind(s)} sx={{ ...act, display: 'inline-flex' }}>
                        <UndoLinear size={12} color="currentColor" />
                        Rewind here
                      </ButtonBase>
                    </span>
                  </Tooltip>
                  <Tooltip title="Start a new thread that keeps everything up to and including this step">
                    <span>
                      <ButtonBase disabled={busy} onClick={() => void fork(s)} sx={{ ...act, display: 'inline-flex' }}>
                        <Hierarchy2Linear size={12} color="currentColor" />
                        Fork after this
                      </ButtonBase>
                    </span>
                  </Tooltip>
                  {s.files.length > 0 && (
                    <Tooltip title="Put the files this step changed back as they were">
                      <span>
                        <ButtonBase disabled={busy} onClick={() => void restoreFiles(s)} sx={{ ...act, display: 'inline-flex' }}>
                          <DocumentTextLinear size={12} color="currentColor" />
                          Restore files
                        </ButtonBase>
                      </span>
                    </Tooltip>
                  )}
                </Stack>
              </Box>
            ))}
          </Box>
        )}
      </Box>
    </Stack>
  )
}
