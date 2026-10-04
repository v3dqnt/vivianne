import { useEffect, useMemo, useState } from 'react'
import { Box, Button, ButtonBase, CircularProgress, Dialog, DialogActions, DialogContent, DialogTitle, Stack, TextField, Typography, alpha, useTheme } from '../../../ui'
import { CopyLinear, FolderLinear, TickCircleLinear } from '../../../icons'
import { call } from '../../../ipc'
import { useMultiplayerStore } from '../store/multiplayerStore'
import { useProjectsStore, PROJECT_COLORS } from '../../sessions/store/projectsStore'
import { inviteMessage, parseInvite } from '../lib/invite'

function Step({ n, title, done, children }: { n: number; title: string; done: boolean; children: React.ReactNode }) {
  const theme = useTheme()
  return (
    <Stack direction="row" sx={{ gap: 1.5, alignItems: 'flex-start' }}>
      <Box
        sx={{
          width: 22,
          height: 22,
          borderRadius: '50%',
          flexShrink: 0,
          display: 'grid',
          placeItems: 'center',
          fontSize: '0.6875rem',
          fontWeight: 700,
          bgcolor: done ? alpha(theme.palette.success.main, 0.15) : 'action.hover',
          color: done ? 'success.main' : 'text.secondary',
          mt: 0.125,
        }}
      >
        {done ? <TickCircleLinear size={14} color="currentColor" /> : n}
      </Box>
      <Stack sx={{ flex: 1, minWidth: 0, gap: 1 }}>
        <Typography variant="body2" sx={{ fontWeight: 600 }}>
          {title}
        </Typography>
        {children}
      </Stack>
    </Stack>
  )
}

/**
 * Start working with someone: swap invites (each side authorizes the other),
 * then pick the shared project folder. Or try it alone with a test partner,
 * a second Vivianne on this computer.
 */
export function PairProjectDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const myId = useMultiplayerStore((s) => s.myId)
  const displayName = useMultiplayerStore((s) => s.displayName)
  const setDisplayName = useMultiplayerStore((s) => s.setDisplayName)
  const init = useMultiplayerStore((s) => s.init)
  const createMultiplayer = useProjectsStore((s) => s.createMultiplayer)

  const [name, setName] = useState('')
  const [theirs, setTheirs] = useState('')
  const [testPartner, setTestPartner] = useState<string | null>(null)
  const [project, setProject] = useState('')
  const [folder, setFolder] = useState('')
  const [remote, setRemote] = useState('')
  const [advanced, setAdvanced] = useState(false)
  const [copied, setCopied] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!open) return
    void init()
    setTheirs('')
    setTestPartner(null)
    setProject('')
    setFolder('')
    setRemote('')
    setError(null)
  }, [open, init])
  useEffect(() => {
    if (open) setName(displayName ?? '')
  }, [open, displayName])

  const partner = useMemo(() => (testPartner ? { id: testPartner, name: 'Test partner' } : parseInvite(theirs)), [theirs, testPartner])
  const badInvite = theirs.trim().length > 0 && !partner
  const ready = !!partner && !!project.trim() && !!folder.trim()

  const copy = async () => {
    if (!myId) return
    if (name.trim() !== (displayName ?? '')) await setDisplayName(name)
    await navigator.clipboard.writeText(inviteMessage(myId, name))
    setCopied(true)
    window.setTimeout(() => setCopied(false), 1800)
  }

  const pickFolder = async () => {
    const { open: openDialog } = await import('@tauri-apps/plugin-dialog')
    const picked = await openDialog({ directory: true, multiple: false })
    if (typeof picked === 'string') {
      setFolder(picked)
      if (!project.trim()) setProject(picked.split(/[\\/]/).filter(Boolean).pop() ?? '')
    }
  }

  const useTestPartner = async () => {
    setError(null)
    try {
      const id = await call<string>('cmd_multiplayer_test_partner_id')
      if (id) {
        setTestPartner(id)
        setTheirs('')
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  const create = async () => {
    if (!partner || !ready) return
    setBusy(true)
    setError(null)
    try {
      if (name.trim() !== (displayName ?? '')) await setDisplayName(name)
      const color = Object.keys(PROJECT_COLORS)[Math.floor(Math.random() * Object.keys(PROJECT_COLORS).length)]
      await createMultiplayer(project.trim(), folder.trim(), partner.id, partner.name, remote.trim() || null, color)
      if (testPartner) await call('cmd_multiplayer_launch_test_partner', { name: project.trim(), folder: folder.trim() })
      onClose()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog open={open} onClose={onClose} maxWidth="sm" fullWidth>
      <DialogTitle sx={{ fontSize: '1rem', fontWeight: 700 }}>Work together</DialogTitle>
      <DialogContent>
        <Stack sx={{ gap: 2.5, pt: 0.5 }}>
          <Typography variant="body2" color="text.secondary" sx={{ lineHeight: 1.55 }}>
            Two people, each with their own Vivianne, on one project: you chat, see what each other's agent did, keep the code in
            sync, and see what each of you spent. The connection is direct and encrypted; nothing goes through a server of ours.
          </Typography>

          <Step n={1} title="Send them your invite" done={copied}>
            <Stack direction="row" sx={{ gap: 1, alignItems: 'center' }}>
              <TextField size="small" label="Your name" value={name} onChange={(e) => setName(e.target.value)} sx={{ flex: 1 }} placeholder="How you'll appear to them" />
              <Button
                variant="contained"
                disableElevation
                disabled={!myId}
                onClick={() => void copy()}
                startIcon={copied ? <TickCircleLinear size={14} color="currentColor" /> : <CopyLinear size={14} color="currentColor" />}
                sx={{ flexShrink: 0, borderRadius: '8px' }}
              >
                {copied ? 'Copied' : 'Copy invite'}
              </Button>
            </Stack>
            <Typography variant="caption" color="text.secondary">
              Paste it to them in any chat. They do the same and send you theirs.
            </Typography>
          </Step>

          <Step n={2} title="Paste their invite" done={!!partner}>
            {testPartner ? (
              <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
                <Typography variant="body2" sx={{ flex: 1 }}>
                  Pairing with a <b>test partner</b>: a second Vivianne that opens on this computer.
                </Typography>
                <Button size="small" onClick={() => setTestPartner(null)}>
                  Undo
                </Button>
              </Stack>
            ) : (
              <>
                <TextField
                  size="small"
                  multiline
                  minRows={2}
                  value={theirs}
                  onChange={(e) => setTheirs(e.target.value)}
                  placeholder="vivianne-invite:…"
                  error={badInvite}
                  helperText={
                    badInvite
                      ? "That doesn't look like an invite. Paste the whole message they sent."
                      : partner
                        ? `Pairing with ${partner.name ?? 'your partner'}`
                        : undefined
                  }
                  slotProps={{ input: { sx: { fontFamily: '"Geist Mono", ui-monospace, monospace', fontSize: '0.75rem' } } }}
                />
                <ButtonBase onClick={() => void useTestPartner()} sx={{ alignSelf: 'flex-start', fontSize: '0.75rem', color: 'text.secondary', borderRadius: '6px', px: 0.5, '&:hover': { color: 'text.primary' } }}>
                  No one to pair with yet? Try it with a test partner on this computer
                </ButtonBase>
              </>
            )}
          </Step>

          <Step n={3} title="Choose the shared project" done={ready}>
            <TextField size="small" label="Project name" value={project} onChange={(e) => setProject(e.target.value)} />
            <Stack direction="row" sx={{ gap: 1, alignItems: 'flex-end' }}>
              <TextField size="small" label="Folder" value={folder} onChange={(e) => setFolder(e.target.value)} fullWidth />
              <Button size="small" onClick={() => void pickFolder()} startIcon={<FolderLinear size={13} color="currentColor" />} sx={{ flexShrink: 0 }}>
                Browse
              </Button>
            </Stack>
            <Typography variant="caption" color="text.secondary">
              It becomes a git repository. After each run it's committed and sent to your partner, so you both stay in step.
            </Typography>
            {advanced ? (
              <TextField
                size="small"
                label="Git remote (optional)"
                placeholder="https://github.com/you/project.git"
                value={remote}
                onChange={(e) => setRemote(e.target.value)}
                helperText="With a remote, changes go through it (pull and push) instead of directly between you."
              />
            ) : (
              <ButtonBase onClick={() => setAdvanced(true)} sx={{ alignSelf: 'flex-start', fontSize: '0.75rem', color: 'text.secondary', borderRadius: '6px', px: 0.5 }}>
                Use a git remote instead…
              </ButtonBase>
            )}
          </Step>

          {error && (
            <Typography variant="caption" color="error.main">
              {error}
            </Typography>
          )}
        </Stack>
      </DialogContent>
      <DialogActions>
        <Button color="inherit" onClick={onClose}>
          Cancel
        </Button>
        <Button
          variant="contained"
          disableElevation
          disabled={!ready || busy}
          onClick={() => void create()}
          startIcon={busy ? <CircularProgress size={14} /> : undefined}
        >
          {testPartner ? 'Create and open test partner' : 'Create shared project'}
        </Button>
      </DialogActions>
    </Dialog>
  )
}
