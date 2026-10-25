import { useEffect, useState } from 'react'
import { Button, CircularProgress, Link, Stack, Switch, Typography, Alert } from '../../../ui'
import { call, on } from '../../../ipc'

interface LayaStatus {
  installed: boolean
  enabled: boolean
  running: boolean
  installing: boolean
}

function Row({ title, children, control }: { title: string; children: React.ReactNode; control: React.ReactNode }) {
  return (
    <Stack direction="row" sx={{ gap: 2, alignItems: 'flex-start' }}>
      <Stack sx={{ gap: 0.25, flex: 1, minWidth: 0 }}>
        <Typography variant="body2" sx={{ fontWeight: 500 }}>
          {title}
        </Typography>
        <Typography variant="caption" color="text.secondary" sx={{ lineHeight: 1.5 }}>
          {children}
        </Typography>
      </Stack>
      {control}
    </Stack>
  )
}

/**
 * The notch: whether it stays up, and the decision engine (Laya) that tells
 * what a spoken request is about.
 */
export function NotchSubsection() {
  const [alwaysOn, setAlwaysOn] = useState(true)
  const [laya, setLaya] = useState<LayaStatus | null>(null)
  const [line, setLine] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  const refresh = () => void call<LayaStatus>('cmd_laya_status').then((s) => s && setLaya(s)).catch(() => {})

  useEffect(() => {
    void call<Record<string, unknown>>('cmd_get_settings').then((s) => setAlwaysOn(s?.notchAlwaysOn !== false))
    refresh()
    const off = on<string>('laya:install', (l) => setLine(l))
    // The engine takes a while to load after a start; keep the status honest.
    const t = window.setInterval(refresh, 5000)
    return () => {
      void off.then((f) => f())
      window.clearInterval(t)
    }
  }, [])

  const install = async () => {
    setError(null)
    setLaya((s) => (s ? { ...s, installing: true } : s))
    try {
      await call('cmd_laya_install')
    } catch (e) {
      setError(typeof e === 'string' ? e : e instanceof Error ? e.message : String(e))
    } finally {
      setLine(null)
      refresh()
    }
  }

  return (
    <Stack sx={{ gap: 2 }}>
      <Row
        title="Always show the notch"
        control={
          <Switch
            size="small"
            checked={alwaysOn}
            onChange={(e) => {
              setAlwaysOn(e.target.checked)
              void call('cmd_notch_set_always_on', { on: e.target.checked })
            }}
          />
        }
      >
        A small tab at the top of the screen that shows what every agent is doing and when one needs you. Hover it
        for the details. Closing Vivianne's window keeps her running in the background, with the notch and the tray
        icon.
      </Row>

      <Row
        title="Understand what I'm asking"
        control={
          laya?.installed ? (
            <Switch
              size="small"
              checked={laya.enabled}
              onChange={(e) => {
                setLaya({ ...laya, enabled: e.target.checked })
                void call('cmd_laya_set_enabled', { enabled: e.target.checked }).then(refresh)
              }}
            />
          ) : (
            <Button size="small" variant="outlined" disabled={!laya || laya.installing} onClick={() => void install()} sx={{ borderRadius: '8px', flexShrink: 0 }}>
              {laya?.installing ? 'Installing…' : 'Install'}
            </Button>
          )
        }
      >
        Tells a question from a command, and which of your projects you mean, so the notch answers out loud, asks
        the right project, or acts on your computer. Uses{' '}
        <Link href="https://github.com/NandhaKishorM/laya" target="_blank" rel="noreferrer">
          Laya
        </Link>
        , an open decision model that runs on this machine (about 1 GB with PyTorch; needs Python 3.10+). Without it,
        simpler rules decide.
        {laya?.installed && (
          <>
            {' '}
            {laya.enabled ? (laya.running ? 'Running.' : 'Starting…') : 'Off.'}
          </>
        )}
      </Row>
      {(laya?.installing || line) && (
        <Stack direction="row" sx={{ gap: 1, alignItems: 'center', color: 'text.secondary' }}>
          <CircularProgress size={12} />
          <Typography variant="caption" noWrap sx={{ fontFamily: '"Geist Mono", ui-monospace, monospace' }}>
            {line ?? 'Installing…'}
          </Typography>
        </Stack>
      )}
      {error && (
        <Alert severity="error">
          {error}
        </Alert>
      )}
    </Stack>
  )
}
