import { useEffect, useState } from 'react'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import TextField from '@mui/material/TextField'
import Button from '@mui/material/Button'
import IconButton from '@mui/material/IconButton'
import CircularProgress from '@mui/material/CircularProgress'
import Divider from '@mui/material/Divider'
import Box from '@mui/material/Box'
import Chip from '@mui/material/Chip'
import Tooltip from '@mui/material/Tooltip'
import { Link2Linear, SendLinear, CodeLinear } from '../../../icons'
import { useMultiplayerStore } from '../store/multiplayerStore'
import { useProjectsStore } from '../../sessions/store/projectsStore'
import { Markdown } from '../../ai/components/Markdown'
import { contentToText } from '../../ai/lib/types'

/** The turn-by-turn feed of the peer's agent work, mirrored live once
 * connected — the "shared context" half of the panel. */
function SharedContext() {
  const peerMessages = useMultiplayerStore((s) => s.peerMessages)
  if (peerMessages.length === 0) {
    return (
      <Typography variant="caption" color="text.secondary">
        Nothing from your pair yet — their agent's work shows up here live as they go.
      </Typography>
    )
  }
  return (
    <Stack sx={{ gap: 0.75, maxHeight: 260, overflowY: 'auto' }}>
      {peerMessages.map((m, i) => {
        const preview = contentToText(m.content).trim().slice(0, 160) || `[${m.type}]`
        return (
          <Box key={i} sx={{ px: 1.25, py: 0.75, borderRadius: 1.5, bgcolor: 'action.hover' }}>
            <Typography variant="caption" sx={{ fontWeight: 600, textTransform: 'capitalize' }}>
              {m.type}
            </Typography>
            <Typography variant="caption" color="text.secondary" sx={{ display: 'block' }} noWrap>
              {preview}
            </Typography>
          </Box>
        )
      })}
    </Stack>
  )
}

/** Chat + snippet sharing in one stream. A snippet is just a chat message
 * whose text is a fenced code block — no separate wire type, rendered
 * through the same `Markdown` component the main thread already uses, so
 * a shared snippet gets real syntax highlighting on both ends for free. */
function ChatAndSnippets() {
  const chatLog = useMultiplayerStore((s) => s.chatLog)
  const sendChat = useMultiplayerStore((s) => s.sendChat)
  const [text, setText] = useState('')
  const [snippetMode, setSnippetMode] = useState(false)

  const submit = () => {
    const trimmed = text.trim()
    if (!trimmed) return
    void sendChat(snippetMode ? `\`\`\`\n${trimmed}\n\`\`\`` : trimmed)
    setText('')
  }

  return (
    <Stack sx={{ gap: 1 }}>
      <Typography variant="caption" sx={{ fontWeight: 600 }}>
        Chat
      </Typography>
      <Stack sx={{ gap: 1, maxHeight: 260, overflowY: 'auto' }}>
        {chatLog.map((entry, i) => (
          <Box key={i} sx={{ alignSelf: entry.fromPeer ? 'flex-start' : 'flex-end', maxWidth: '85%' }}>
            <Box
              sx={{
                px: 1.25,
                py: 0.5,
                borderRadius: 1.5,
                bgcolor: entry.fromPeer ? 'action.hover' : 'primary.main',
                color: entry.fromPeer ? 'text.primary' : 'primary.contrastText',
                fontSize: '0.75rem',
                '& p': { m: 0 },
                '& pre': { fontSize: '0.7rem' },
              }}
            >
              <Markdown text={entry.text} />
            </Box>
          </Box>
        ))}
      </Stack>
      <Stack direction="row" sx={{ gap: 1, alignItems: 'flex-end' }}>
        <Tooltip title={snippetMode ? 'Sending as a code snippet' : 'Send as a code snippet'}>
          <IconButton
            size="small"
            onClick={() => setSnippetMode((v) => !v)}
            sx={{ color: snippetMode ? 'primary.main' : 'text.secondary', bgcolor: snippetMode ? 'action.selected' : 'transparent' }}
          >
            <CodeLinear size={16} color="currentColor" />
          </IconButton>
        </Tooltip>
        <TextField
          size="small"
          fullWidth
          multiline={snippetMode}
          minRows={snippetMode ? 3 : 1}
          placeholder={snippetMode ? 'Paste the snippet' : 'Message your pair, or divide up the work'}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey && !snippetMode) {
              e.preventDefault()
              submit()
            }
          }}
          slotProps={snippetMode ? { input: { sx: { fontFamily: '"JetBrains Mono", ui-monospace, monospace', fontSize: '0.75rem' } } } : undefined}
        />
        <IconButton onClick={submit} disabled={!text.trim()}>
          <SendLinear size={16} color="currentColor" />
        </IconButton>
      </Stack>
    </Stack>
  )
}

/** Shown when the active project is multiplayer but this machine isn't
 * currently connected to the peer that project authorized (could be
 * disconnected entirely, or connected to a *different* peer). */
function ConnectToProjectPeer({ peerId, peerDisplayName }: { peerId: string; peerDisplayName: string | null }) {
  const connecting = useMultiplayerStore((s) => s.connecting)
  const error = useMultiplayerStore((s) => s.error)
  const connect = useMultiplayerStore((s) => s.connect)

  return (
    <Stack sx={{ gap: 1.5, alignItems: 'flex-start' }}>
      <Typography variant="caption" color="text.secondary">
        This project is paired with {peerDisplayName || `${peerId.slice(0, 12)}…`}, but you're not connected to them right
        now.
      </Typography>
      <Button
        variant="contained"
        size="small"
        disabled={connecting}
        onClick={() => void connect(peerId)}
        startIcon={connecting ? <CircularProgress size={14} /> : <Link2Linear size={14} color="currentColor" />}
      >
        Connect
      </Button>
      {error && (
        <Typography variant="caption" color="error.main">
          {error}
        </Typography>
      )}
    </Stack>
  )
}

export function MultiplayerWorkspacePanel() {
  const projects = useProjectsStore((s) => s.projects)
  const activeProjectId = useProjectsStore((s) => s.activeProjectId)
  const status = useMultiplayerStore((s) => s.status)
  const initMultiplayer = useMultiplayerStore((s) => s.init)

  useEffect(() => {
    void initMultiplayer()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const project = projects.find((p) => p.id === activeProjectId)

  if (!project?.multiplayer) {
    return (
      <Stack sx={{ gap: 1, p: 2 }}>
        <Typography variant="caption" color="text.secondary">
          {project ? `"${project.name}" isn't a multiplayer project.` : 'No project selected.'} Create a new project with
          the multiplayer toggle on to share context, chat, and snippets with a pair here — see Settings → Multiplayer
          for your pairing code.
        </Typography>
      </Stack>
    )
  }

  const connectedToRightPeer = status.state === 'connected' && status.peerId === project.multiplayer.peer_id

  return (
    <Stack sx={{ gap: 2, p: 2, overflowY: 'auto' }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
        <Chip
          size="small"
          color={connectedToRightPeer ? 'success' : 'default'}
          variant={connectedToRightPeer ? 'filled' : 'outlined'}
          label={connectedToRightPeer ? 'Live' : 'Not connected'}
        />
        <Typography variant="caption" color="text.secondary" noWrap>
          Paired with {project.multiplayer.peer_display_name || `${project.multiplayer.peer_id.slice(0, 12)}…`}
        </Typography>
      </Stack>

      {!connectedToRightPeer ? (
        <ConnectToProjectPeer peerId={project.multiplayer.peer_id} peerDisplayName={project.multiplayer.peer_display_name} />
      ) : (
        <>
          <Divider />
          <Stack sx={{ gap: 1 }}>
            <Typography variant="caption" sx={{ fontWeight: 600 }}>
              Shared context
            </Typography>
            <SharedContext />
          </Stack>
          <Divider />
          <ChatAndSnippets />
        </>
      )}
    </Stack>
  )
}
