import { useEffect, useState } from 'react'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import TextField from '@mui/material/TextField'
import Button from '@mui/material/Button'
import IconButton from '@mui/material/IconButton'
import CircularProgress from '@mui/material/CircularProgress'
import Divider from '@mui/material/Divider'
import Chip from '@mui/material/Chip'
import Tooltip from '@mui/material/Tooltip'
import { CopyLinear, Link2Linear, CloseCircleLinear } from '../../../icons'
import { useMultiplayerStore } from '../store/multiplayerStore'

function StatusChip() {
  const status = useMultiplayerStore((s) => s.status)
  switch (status.state) {
    case 'connected':
      return <Chip size="small" color="success" label={`Connected to ${status.peerDisplayName || status.peerId.slice(0, 8)}`} />
    case 'connecting':
      return <Chip size="small" label="Connecting…" />
    case 'failed':
      return <Chip size="small" color="error" label={status.reason} />
    default:
      return <Chip size="small" variant="outlined" label="Not connected" />
  }
}

function MyCode() {
  const myId = useMultiplayerStore((s) => s.myId)
  const [copied, setCopied] = useState(false)
  if (!myId) return null
  return (
    <Stack sx={{ gap: 1 }}>
      <Typography variant="caption" sx={{ fontWeight: 600 }}>
        Your pairing code
      </Typography>
      <Stack direction="row" sx={{ gap: 1 }}>
        <TextField
          size="small"
          fullWidth
          value={myId}
          slotProps={{ input: { readOnly: true, sx: { fontFamily: '"JetBrains Mono", ui-monospace, monospace', fontSize: '0.75rem' } } }}
        />
        <Tooltip title={copied ? 'Copied' : 'Copy'}>
          <IconButton
            onClick={() => {
              void navigator.clipboard.writeText(myId)
              setCopied(true)
              setTimeout(() => setCopied(false), 1500)
            }}
          >
            <CopyLinear size={16} color="currentColor" />
          </IconButton>
        </Tooltip>
      </Stack>
      <Typography variant="caption" color="text.secondary">
        Share this with whoever you want to pair with — they paste it below on their end.
      </Typography>
    </Stack>
  )
}

function ConnectForm() {
  const connecting = useMultiplayerStore((s) => s.connecting)
  const error = useMultiplayerStore((s) => s.error)
  const connect = useMultiplayerStore((s) => s.connect)
  const [peerId, setPeerId] = useState('')

  const submit = () => {
    if (!peerId.trim()) return
    void connect(peerId.trim())
  }

  return (
    <Stack sx={{ gap: 1 }}>
      <Typography variant="caption" sx={{ fontWeight: 600 }}>
        Connect to a peer
      </Typography>
      <Stack direction="row" sx={{ gap: 1 }}>
        <TextField
          size="small"
          fullWidth
          placeholder="Paste their pairing code"
          value={peerId}
          onChange={(e) => setPeerId(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && submit()}
          slotProps={{ input: { sx: { fontFamily: '"JetBrains Mono", ui-monospace, monospace', fontSize: '0.75rem' } } }}
        />
        <Button
          variant="contained"
          disabled={connecting || !peerId.trim()}
          onClick={submit}
          startIcon={connecting ? <CircularProgress size={14} /> : <Link2Linear size={15} color="currentColor" />}
        >
          Connect
        </Button>
      </Stack>
      {error && (
        <Typography variant="caption" color="error.main">
          {error}
        </Typography>
      )}
    </Stack>
  )
}

/** Machine-level P2P identity + connection — infrastructure, not a
 * conversation. What gets *shared* once connected (session activity, chat,
 * snippets) is scoped per multiplayer project instead, in the "Multiplayer"
 * dock panel (`MultiplayerWorkspacePanel`) — a live connection here doesn't
 * by itself expose anything; a project has to explicitly authorize this
 * peer first. See `docs/multiplayer-update-plan.md`. */
export function MultiplayerSection() {
  const status = useMultiplayerStore((s) => s.status)
  const init = useMultiplayerStore((s) => s.init)
  const disconnect = useMultiplayerStore((s) => s.disconnect)

  useEffect(() => {
    void init()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const connected = status.state === 'connected'

  return (
    <Stack sx={{ gap: 2.5, maxWidth: 420 }}>
      <Typography variant="caption" color="text.secondary">
        Connect directly to another Vivianne, computer to computer. A live connection here doesn't share anything by
        itself — create a multiplayer project (or add this peer to one) to actually share work.
      </Typography>

      <Stack direction="row" sx={{ alignItems: 'center', justifyContent: 'space-between' }}>
        <StatusChip />
        {connected && (
          <Button
            size="small"
            color="error"
            variant="outlined"
            startIcon={<CloseCircleLinear size={14} color="currentColor" />}
            onClick={() => void disconnect()}
          >
            Disconnect
          </Button>
        )}
      </Stack>

      <Divider />
      <MyCode />
      {!connected && (
        <>
          <Divider />
          <ConnectForm />
        </>
      )}
    </Stack>
  )
}
