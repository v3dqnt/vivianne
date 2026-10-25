import { useEffect, useMemo, useState } from 'react'
import { Box, Button, ButtonBase, CircularProgress, Collapse, Dialog, InputBase, Link, Stack, TextField, Typography, Alert } from '../../../ui'
import { ArrowDownLinear, LockLinear, SearchNormalLinear, TickCircleLinear } from '../../../icons'
import { appIcon, useConnections, type ConnectionApp } from '../store/connectionsStore'
import { McpSection } from './McpSection'
import { useSettingsUi } from '../store/uiStore'

function AppIcon({ app, size = 32 }: { app: ConnectionApp; size?: number }) {
  const [src, setSrc] = useState<string | null>(null)
  useEffect(() => {
    let live = true
    void appIcon(app.domain).then((s) => live && setSrc(s))
    return () => {
      live = false
    }
  }, [app.domain])
  return (
    <Box
      sx={{
        width: size,
        height: size,
        flexShrink: 0,
        borderRadius: '8px',
        border: 1,
        borderColor: 'divider',
        bgcolor: '#fff',
        display: 'grid',
        placeItems: 'center',
        overflow: 'hidden',
        fontSize: size * 0.42,
        fontWeight: 600,
        color: '#555',
      }}
    >
      {src ? <img src={src} alt="" width={size * 0.6} height={size * 0.6} style={{ display: 'block' }} /> : app.name[0]}
    </Box>
  )
}

/** What signing in will involve, said before the user clicks. */
const HOW: Record<ConnectionApp['auth'], string> = {
  oauth: 'Sign in with your account',
  open: 'No sign-in needed',
  token: 'Paste an access token',
  native: 'Paste a bot token',
}

function AppTile({ app, onSetup }: { app: ConnectionApp; onSetup: (app: ConnectionApp) => void }) {
  const pending = useConnections((s) => s.pending)
  const error = useConnections((s) => s.errors[app.id])
  const connect = useConnections((s) => s.connect)
  const cancel = useConnections((s) => s.cancel)
  const disconnect = useConnections((s) => s.disconnect)
  const [removing, setRemoving] = useState(false)
  const waiting = pending === app.id

  const start = () => {
    if (app.fields.length > 0) onSetup(app)
    else void connect(app.id)
  }

  return (
    <Stack
      sx={{
        gap: 1,
        p: 1.5,
        border: 1,
        borderColor: 'divider',
        borderRadius: '10px',
        minWidth: 0,
        transition: 'border-color 0.15s',
        '&:hover': { borderColor: 'text.disabled' },
      }}
    >
      <Stack direction="row" sx={{ gap: 1.25, alignItems: 'center', minWidth: 0 }}>
        <AppIcon app={app} />
        <Stack sx={{ minWidth: 0, flex: 1 }}>
          <Typography variant="body2" sx={{ fontWeight: 600 }} noWrap>
            {app.name}
          </Typography>
          <Typography variant="caption" color="text.secondary" noWrap title={app.account ?? undefined}>
            {app.connected ? (app.account ? `Connected as ${app.account}` : 'Connected') : HOW[app.auth]}
          </Typography>
        </Stack>
        {app.connected && <TickCircleLinear size={16} color="var(--ui-palette-success-main, #2e7d32)" />}
      </Stack>
      <Typography
        variant="caption"
        color="text.secondary"
        sx={{ lineHeight: 1.45, display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical', overflow: 'hidden', minHeight: '2.9em' }}
      >
        {app.description}
      </Typography>
      {error && (
        <Alert severity="error">
          {error}
        </Alert>
      )}
      <Stack direction="row" sx={{ gap: 0.75, alignItems: 'center' }}>
        {app.connected ? (
          <Button
            size="small"
            color="inherit"
            disabled={removing}
            onClick={async () => {
              setRemoving(true)
              try {
                await disconnect(app.id)
              } finally {
                setRemoving(false)
              }
            }}
            sx={{ borderRadius: '8px' }}
          >
            {removing ? 'Disconnecting…' : 'Disconnect'}
          </Button>
        ) : waiting ? (
          <>
            <CircularProgress size={14} />
            <Typography variant="caption" color="text.secondary" sx={{ flex: 1 }}>
              {app.auth === 'oauth' ? 'Finish signing in in your browser…' : 'Connecting…'}
            </Typography>
            {app.auth === 'oauth' && (
              <Button size="small" color="inherit" onClick={() => void cancel()} sx={{ borderRadius: '8px' }}>
                Cancel
              </Button>
            )}
          </>
        ) : (
          <Button size="small" variant="outlined" disabled={pending !== null} onClick={start} sx={{ borderRadius: '8px' }}>
            Connect
          </Button>
        )}
      </Stack>
    </Stack>
  )
}

/** For apps that take a pasted token: the fields, and how to get them. */
function SetupDialog({ app, onClose }: { app: ConnectionApp | null; onClose: () => void }) {
  const connect = useConnections((s) => s.connect)
  const pending = useConnections((s) => s.pending)
  const error = useConnections((s) => (app ? s.errors[app.id] : undefined))
  const [values, setValues] = useState<Record<string, string>>({})

  useEffect(() => setValues({}), [app?.id])

  if (!app) return null
  const ready = app.fields.every((f) => (values[f.key] ?? '').trim())
  const busy = pending === app.id

  const submit = async () => {
    const trimmed = Object.fromEntries(Object.entries(values).map(([k, v]) => [k, v.trim()]))
    if (await connect(app.id, trimmed)) onClose()
  }

  return (
    <Dialog open onClose={busy ? undefined : onClose} maxWidth="xs" fullWidth slotProps={{ paper: { sx: { borderRadius: 3 } } }}>
      <Stack sx={{ p: 3, gap: 2 }}>
        <Stack direction="row" sx={{ gap: 1.25, alignItems: 'center' }}>
          <AppIcon app={app} size={36} />
          <Stack>
            <Typography variant="subtitle1" sx={{ fontWeight: 700 }}>
              Connect {app.name}
            </Typography>
            <Typography variant="caption" color="text.secondary">
              {app.description}
            </Typography>
          </Stack>
        </Stack>
        {app.help && (
          <Typography variant="caption" color="text.secondary" sx={{ lineHeight: 1.55 }}>
            {app.help}{' '}
            {app.helpUrl && (
              <Link href={app.helpUrl} target="_blank" rel="noreferrer">
                Open the page
              </Link>
            )}
          </Typography>
        )}
        <Stack
          component="form"
          sx={{ gap: 1.5 }}
          onSubmit={(e: React.FormEvent) => {
            e.preventDefault()
            if (ready && !busy) void submit()
          }}
        >
          {app.fields.map((f, i) => (
            <TextField
              key={f.key}
              label={f.label}
              size="small"
              type={f.secret ? 'password' : 'text'}
              placeholder={f.placeholder}
              value={values[f.key] ?? ''}
              autoFocus={i === 0}
              autoComplete="off"
              onChange={(e) => setValues((v) => ({ ...v, [f.key]: e.target.value }))}
            />
          ))}
          {error && (
            <Alert severity="error">
              {error}
            </Alert>
          )}
          <Stack direction="row" sx={{ gap: 1, alignItems: 'center' }}>
            <Button type="submit" variant="contained" disableElevation disabled={!ready || busy} startIcon={busy ? <CircularProgress size={14} /> : undefined}>
              Connect
            </Button>
            <Button color="inherit" onClick={onClose} disabled={busy}>
              Cancel
            </Button>
            <Stack direction="row" sx={{ gap: 0.5, alignItems: 'center', ml: 'auto', color: 'text.secondary' }}>
              <LockLinear size={12} color="currentColor" />
              <Typography variant="caption">Kept in your system keychain</Typography>
            </Stack>
          </Stack>
        </Stack>
      </Stack>
    </Dialog>
  )
}

export function ConnectionsSection() {
  const apps = useConnections((s) => s.apps)
  const loaded = useConnections((s) => s.loaded)
  const refresh = useConnections((s) => s.refresh)
  const [query, setQuery] = useState('')
  const [category, setCategory] = useState<string | null>(null)
  const [setup, setSetup] = useState<ConnectionApp | null>(null)
  // Opened straight away when Settings was asked for the MCP servers.
  const [advanced, setAdvanced] = useState(() => useSettingsUi.getState().section === 'mcp')

  useEffect(() => {
    void refresh()
  }, [refresh])

  const categories = useMemo(() => [...new Set(apps.map((a) => a.category))], [apps])
  const q = query.trim().toLowerCase()
  const visible = apps.filter(
    (a) =>
      (!category || a.category === category) &&
      (!q || a.name.toLowerCase().includes(q) || a.description.toLowerCase().includes(q) || a.category.toLowerCase().includes(q)),
  )
  const connected = visible.filter((a) => a.connected)
  const rest = visible.filter((a) => !a.connected)

  const grid = (list: ConnectionApp[]) => (
    <Box sx={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(190px, 1fr))', gap: 1 }}>
      {list.map((a) => (
        <AppTile key={a.id} app={a} onSetup={setSetup} />
      ))}
    </Box>
  )

  return (
    <Stack sx={{ gap: 2 }}>
      <Stack sx={{ gap: 0.5 }}>
        <Typography variant="subtitle2" sx={{ fontWeight: 600 }}>
          Connections
        </Typography>
        <Typography variant="caption" color="text.secondary" sx={{ lineHeight: 1.5 }}>
          Connect the apps you use and Vivianne can work in them: read and send messages, file issues, update docs and more.
          She asks before anything that changes something. Sign-ins stay on this machine.
        </Typography>
      </Stack>

      <Stack sx={{ gap: 1 }}>
        <Stack
          direction="row"
          sx={{ alignItems: 'center', gap: 1, px: 1.25, py: 0.5, border: 1, borderColor: 'divider', borderRadius: '8px', color: 'text.secondary' }}
        >
          <SearchNormalLinear size={14} color="currentColor" />
          <InputBase placeholder="Search apps" value={query} onChange={(e) => setQuery(e.target.value)} sx={{ flex: 1, fontSize: '0.8125rem' }} />
        </Stack>
        <Stack direction="row" sx={{ gap: 0.5, flexWrap: 'wrap' }}>
          {[null, ...categories].map((c) => (
            <ButtonBase
              key={c ?? 'all'}
              onClick={() => setCategory(c)}
              sx={{
                px: 1.25,
                py: 0.375,
                borderRadius: 999,
                fontSize: '0.75rem',
                fontWeight: 500,
                border: 1,
                borderColor: category === c ? 'text.primary' : 'divider',
                color: category === c ? 'text.primary' : 'text.secondary',
              }}
            >
              {c ?? 'All'}
            </ButtonBase>
          ))}
        </Stack>
      </Stack>

      {!loaded ? (
        <CircularProgress size={18} />
      ) : (
        <>
          {connected.length > 0 && (
            <Stack sx={{ gap: 1 }}>
              <Typography variant="caption" sx={{ fontWeight: 600, color: 'text.secondary' }}>
                Connected
              </Typography>
              {grid(connected)}
            </Stack>
          )}
          {rest.length > 0 && (
            <Stack sx={{ gap: 1 }}>
              {connected.length > 0 && (
                <Typography variant="caption" sx={{ fontWeight: 600, color: 'text.secondary' }}>
                  More apps
                </Typography>
              )}
              {grid(rest)}
            </Stack>
          )}
          {visible.length === 0 && (
            <Typography variant="caption" color="text.secondary">
              No app matches “{query}”. If it has an MCP server, add it under Advanced.
            </Typography>
          )}
        </>
      )}

      <Stack sx={{ gap: 1, pt: 1, borderTop: 1, borderColor: 'divider' }}>
        <ButtonBase
          onClick={() => setAdvanced((v) => !v)}
          sx={{ display: 'flex', alignItems: 'center', gap: 0.5, alignSelf: 'flex-start', color: 'text.secondary', fontSize: '0.8125rem', fontWeight: 600 }}
        >
          <ArrowDownLinear size={12} color="currentColor" style={{ transform: advanced ? 'rotate(180deg)' : undefined, transition: 'transform 0.15s' }} />
          Advanced: add an MCP server by hand
        </ButtonBase>
        <Collapse in={advanced}>
          <McpSection hide={apps.map((a) => a.id)} />
        </Collapse>
      </Stack>

      <SetupDialog app={setup} onClose={() => setSetup(null)} />
    </Stack>
  )
}
