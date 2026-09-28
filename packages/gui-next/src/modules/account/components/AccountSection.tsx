import { useEffect, useState } from 'react'
import { Stack, Typography, TextField, Button, CircularProgress, Divider, Box } from '../../../ui'
import { UserOctagonLinear, Link2Linear } from '../../../icons'
import { useAccountStore } from '../store/accountStore'

/** Signed-in view: display name (editable) + sign out. */
function SignedIn() {
  const user = useAccountStore((s) => s.user)
  const loading = useAccountStore((s) => s.loading)
  const error = useAccountStore((s) => s.error)
  const signOut = useAccountStore((s) => s.signOut)
  const updateDisplayName = useAccountStore((s) => s.updateDisplayName)

  const [name, setName] = useState(user?.displayName ?? '')
  const [saved, setSaved] = useState(false)
  useEffect(() => setName(user?.displayName ?? ''), [user?.displayName])

  const dirty = name.trim() !== (user?.displayName ?? '') && name.trim().length > 0

  const save = async () => {
    setSaved(false)
    await updateDisplayName(name.trim())
    setSaved(true)
  }

  return (
    <Stack sx={{ gap: 2 }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1.5 }}>
        <Box
          sx={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            width: 40,
            height: 40,
            borderRadius: '50%',
            bgcolor: 'action.selected',
            color: 'text.secondary',
            flexShrink: 0,
          }}
        >
          <UserOctagonLinear size={20} color="currentColor" />
        </Box>
        <Stack sx={{ minWidth: 0 }}>
          <Typography variant="body2" sx={{ fontWeight: 600 }} noWrap>
            {user?.displayName || user?.email || 'Signed in'}
          </Typography>
          {user?.email && (
            <Typography variant="caption" color="text.secondary" noWrap>
              {user.email}
            </Typography>
          )}
        </Stack>
      </Stack>

      <Divider />

      <Stack sx={{ gap: 1 }}>
        <Typography variant="caption" sx={{ fontWeight: 600 }}>
          Display name
        </Typography>
        <Stack direction="row" sx={{ gap: 1 }}>
          <TextField
            size="small"
            fullWidth
            value={name}
            onChange={(e) => {
              setName(e.target.value)
              setSaved(false)
            }}
            placeholder="How your pair sees you"
          />
          <Button
            variant="contained"
            disabled={!dirty || loading}
            onClick={() => void save()}
            startIcon={loading ? <CircularProgress size={14} /> : undefined}
          >
            Save
          </Button>
        </Stack>
        {saved && (
          <Typography variant="caption" color="success.main">
            Saved.
          </Typography>
        )}
        {error && (
          <Typography variant="caption" color="error.main">
            {error}
          </Typography>
        )}
      </Stack>

      <Divider />

      <Button
        variant="outlined"
        color="error"
        onClick={() => void signOut()}
        disabled={loading}
        sx={{ alignSelf: 'flex-start' }}
      >
        Sign out
      </Button>
    </Stack>
  )
}

/** Signed-out view: a single button that opens the sign-in/sign-up flow in
 * the system browser at vivianne.dev — see `accountStore.signInWithBrowser`. */
function SignedOut() {
  const loading = useAccountStore((s) => s.loading)
  const error = useAccountStore((s) => s.error)
  const signInWithBrowser = useAccountStore((s) => s.signInWithBrowser)
  const [waiting, setWaiting] = useState(false)

  const start = async () => {
    setWaiting(true)
    try {
      await signInWithBrowser()
    } finally {
      setWaiting(false)
    }
  }

  return (
    <Stack sx={{ gap: 2, maxWidth: 360 }}>
      <Typography variant="caption" color="text.secondary">
        Sign in to sync your account across devices and pair up for the multiplayer update.
      </Typography>

      <Button
        variant="contained"
        onClick={() => void start()}
        disabled={loading}
        startIcon={loading ? <CircularProgress size={14} color="inherit" /> : <Link2Linear size={16} color="currentColor" />}
        sx={{ alignSelf: 'flex-start' }}
      >
        Sign in with browser
      </Button>

      {waiting && (
        <Typography variant="caption" color="text.secondary">
          Waiting for you to finish in the browser — this tab will update automatically.
        </Typography>
      )}
      {error && (
        <Typography variant="caption" color="error.main">
          {error}
        </Typography>
      )}
    </Stack>
  )
}

export function AccountSection() {
  const user = useAccountStore((s) => s.user)
  const initialized = useAccountStore((s) => s.initialized)
  const init = useAccountStore((s) => s.init)

  useEffect(() => {
    void init()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  if (!initialized) {
    return (
      <Stack sx={{ alignItems: 'center', py: 4 }}>
        <CircularProgress size={20} />
      </Stack>
    )
  }

  return user ? <SignedIn /> : <SignedOut />
}
