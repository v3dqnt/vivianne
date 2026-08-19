import { useEffect, useState } from 'react'
import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import IconButton from '@mui/material/IconButton'
import Collapse from '@mui/material/Collapse'
import CircularProgress from '@mui/material/CircularProgress'
import Dialog from '@mui/material/Dialog'
import DialogTitle from '@mui/material/DialogTitle'
import DialogContent from '@mui/material/DialogContent'
import { UserOctagonLinear, StopCircleLinear, ArrowDownLinear } from '../../../icons'
import { useSubagentStore, type SubagentSummary } from '../store/subagentStore'
import type { AgentMessage } from '../lib/types'
import { MessageBubble } from './MessageBubble'

function formatElapsed(ms: number): string {
  const s = Math.round(ms / 1000)
  if (s < 60) return `${s}s`
  return `${Math.floor(s / 60)}m ${s % 60}s`
}

/** Read-only transcript of one subagent — same message rendering as the main
 * chat, no composer. Polls while the subagent is still running so its
 * transcript-so-far fills in live. */
function SubagentTranscript({ agent, onClose }: { agent: SubagentSummary; onClose: () => void }) {
  const getMessages = useSubagentStore((s) => s.getMessages)
  const [messages, setMessages] = useState<AgentMessage[]>([])
  const [loaded, setLoaded] = useState(false)
  const running = agent.status === 'running'

  useEffect(() => {
    let cancelled = false
    const load = async () => {
      const msgs = await getMessages(agent.id)
      if (!cancelled) {
        setMessages(msgs)
        setLoaded(true)
      }
    }
    void load()
    if (!running) return () => { cancelled = true }
    const id = setInterval(() => void load(), 2000)
    return () => {
      cancelled = true
      clearInterval(id)
    }
  }, [agent.id, running, getMessages])

  return (
    <Dialog open onClose={onClose} maxWidth="md" fullWidth slotProps={{ paper: { sx: { height: 620, borderRadius: 2 } } }}>
      <DialogTitle sx={{ pb: 1 }}>
        <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
          {running && <CircularProgress size={13} thickness={5} />}
          <Typography variant="subtitle2" sx={{ fontWeight: 600, flex: 1, minWidth: 0 }} noWrap>
            {agent.description}
          </Typography>
          <Typography variant="caption" color="text.secondary">
            {agent.status} · {agent.turns} turns · {agent.toolCalls} tools
          </Typography>
        </Stack>
      </DialogTitle>
      <DialogContent dividers sx={{ px: 2, py: 1.5 }}>
        <Typography variant="caption" color="text.secondary" sx={{ display: 'block', mb: 1.5 }}>
          Read-only — this is the subagent's own conversation, not yours.
        </Typography>
        {!loaded ? (
          <Typography variant="caption" color="text.secondary">Loading…</Typography>
        ) : messages.length === 0 ? (
          <Typography variant="caption" color="text.secondary">
            No messages recorded yet.
          </Typography>
        ) : (
          <Stack spacing={1}>
            {messages.map((m, i) => (
              <MessageBubble key={i} message={m} />
            ))}
          </Stack>
        )}
        {agent.error && (
          <Typography variant="caption" color="error.main" sx={{ display: 'block', mt: 1.5 }}>
            {agent.error}
          </Typography>
        )}
      </DialogContent>
    </Dialog>
  )
}

function SubagentRow({ agent }: { agent: SubagentSummary }) {
  const kill = useSubagentStore((s) => s.kill)
  const running = agent.status === 'running'
  const [showTranscript, setShowTranscript] = useState(false)

  return (
    <>
      <Stack
        direction="row"
        onClick={() => setShowTranscript(true)}
        sx={{ alignItems: 'center', gap: 1, py: 0.75, px: 0.5, borderRadius: 1, cursor: 'pointer', '&:hover': { bgcolor: 'action.hover' } }}
      >
        {running ? <CircularProgress size={12} thickness={5} /> : null}
        <Stack sx={{ flex: 1, minWidth: 0 }}>
          <Typography variant="caption" sx={{ fontWeight: 500 }} noWrap>
            {agent.subagentType}: {agent.description}
          </Typography>
          <Typography variant="caption" color="text.secondary">
            {agent.status} · {formatElapsed(agent.elapsedMs)}
            {agent.turns > 0 && ` · ${agent.turns} turns`}
            {agent.toolCalls > 0 && ` · ${agent.toolCalls} tools`}
          </Typography>
        </Stack>
        {running && (
          <IconButton
            size="small"
            onClick={(e) => {
              e.stopPropagation()
              void kill(agent.id)
            }}
            title="Cancel"
          >
            <StopCircleLinear size={14} color="currentColor" />
          </IconButton>
        )}
      </Stack>
      {showTranscript && <SubagentTranscript agent={agent} onClose={() => setShowTranscript(false)} />}
    </>
  )
}

/** Purely a visibility panel — `agentStore` already refreshes this store on
 * the `task` tool's start/end, so there's nothing to wire here beyond render. */
export function SubagentPanel() {
  const subagents = useSubagentStore((s) => s.subagents)
  const [open, setOpen] = useState(true)
  const running = subagents.filter((a) => a.status === 'running').length

  if (subagents.length === 0) return null

  return (
    <Box sx={{ border: 1, borderColor: 'divider', borderRadius: 2, overflow: 'hidden', bgcolor: 'background.paper' }}>
      <Stack
        direction="row"
        onClick={() => setOpen((v) => !v)}
        sx={{ alignItems: 'center', gap: 1, px: 1.5, py: 0.75, cursor: 'pointer', '&:hover': { bgcolor: 'action.hover' } }}
      >
        <UserOctagonLinear size={14} color="currentColor" />
        <Typography variant="caption" sx={{ fontWeight: 600 }}>
          {running > 0 ? `${running} subagent${running === 1 ? '' : 's'} running` : `${subagents.length} subagent${subagents.length === 1 ? '' : 's'}`}
        </Typography>
        <Box sx={{ flex: 1 }} />
        <Box sx={{ display: 'flex', color: 'text.secondary', transform: open ? 'rotate(180deg)' : 'none', transition: 'transform 0.15s' }}>
          <ArrowDownLinear size={14} color="currentColor" />
        </Box>
      </Stack>
      <Collapse in={open}>
        <Stack sx={{ px: 1.5, pb: 0.5, borderTop: 1, borderColor: 'divider' }}>
          {subagents.map((a) => (
            <SubagentRow key={a.id} agent={a} />
          ))}
        </Stack>
      </Collapse>
    </Box>
  )
}
