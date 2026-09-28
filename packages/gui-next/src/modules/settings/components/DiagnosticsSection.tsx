import { useCallback, useEffect, useState } from 'react'
import { Stack, Box, Typography, Switch, TextField, Button, Divider, Alert, Collapse, CircularProgress } from '../../../ui'
import { call } from '../../../ipc'

interface CrashRecord {
  ts: number
  message: string
  location: string
  backtrace: string
  appVersion: string
  os: string
}

interface TelemetrySummary {
  eventCount: number
  firstEventTs: number | null
  lastEventTs: number | null
  kinds: { kind: string; count: number }[]
  crashes: CrashRecord[]
}

function formatWhen(ts: number | null | undefined): string {
  if (!ts) return '—'
  return new Date(ts).toLocaleString()
}

/** Diagnostics — local telemetry, crash records, and bug reporting.
 *
 * The framing here is deliberate: this is presented as *your* diagnostic data
 * that happens to be shareable, not as an analytics feature that happens to be
 * disableable. Everything is on disk and readable here; the upload toggle is
 * off until switched on, and the copy says plainly where data goes. */
export function DiagnosticsSection() {
  const [summary, setSummary] = useState<TelemetrySummary | null>(null)
  const [settings, setSettings] = useState<Record<string, unknown> | null>(null)
  const [enabled, setEnabled] = useState(false)
  const [endpoint, setEndpoint] = useState('')
  const [description, setDescription] = useState('')
  const [includeDiagnostics, setIncludeDiagnostics] = useState(true)
  const [sending, setSending] = useState(false)
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null)
  const [expandedCrash, setExpandedCrash] = useState<number | null>(null)

  const refresh = useCallback(async () => {
    const [s, cfg] = await Promise.all([
      call<TelemetrySummary>('cmd_get_telemetry_summary'),
      call<Record<string, unknown> & { telemetryEnabled?: boolean; telemetryEndpoint?: string }>('cmd_get_settings'),
    ])
    if (s) setSummary(s)
    if (cfg) {
      setSettings(cfg)
      setEnabled(cfg.telemetryEnabled ?? false)
      setEndpoint(cfg.telemetryEndpoint ?? '')
    }
  }, [])

  useEffect(() => {
    void refresh()
  }, [refresh])

  const saveConfig = async (next: { telemetryEnabled?: boolean; telemetryEndpoint?: string }) => {
    await call('cmd_save_settings', { settings: { ...(settings ?? {}), ...next } })
    setSettings((prev) => ({ ...(prev ?? {}), ...next }))
  }

  const submit = async () => {
    setSending(true)
    setResult(null)
    try {
      const res = await call<{ path: string; uploaded: boolean }>('cmd_submit_bug_report', {
        description,
        includeDiagnostics,
      })
      setDescription('')
      setResult({
        ok: true,
        text: res?.uploaded
          ? 'Report sent, and saved locally as a copy.'
          : `Report saved to ${res?.path}. Sharing is off, so nothing was uploaded — attach that file to an issue.`,
      })
    } catch (e) {
      setResult({ ok: false, text: e instanceof Error ? e.message : String(e) })
    } finally {
      setSending(false)
    }
  }

  const clearAll = async () => {
    await call('cmd_clear_telemetry')
    await refresh()
  }

  return (
    <Stack sx={{ gap: 2 }}>
      <Stack sx={{ gap: 0.25 }}>
        <Typography variant="subtitle2" sx={{ fontWeight: 600 }}>
          Diagnostics
        </Typography>
        <Typography variant="caption" color="text.secondary">
          Vivianne records what it does and any crashes to <code>~/.viv/agent/</code> on this machine. None of it is sent
          anywhere unless you turn on sharing below.
        </Typography>
      </Stack>

      <Stack direction="row" sx={{ alignItems: 'center', justifyContent: 'space-between', gap: 2 }}>
        <Stack sx={{ minWidth: 0 }}>
          <Typography variant="body2" sx={{ fontWeight: 500 }}>
            Share anonymous diagnostics
          </Typography>
          <Typography variant="caption" color="text.secondary">
            Off by default. When on, events and crash reports POST to the endpoint below.
          </Typography>
        </Stack>
        <Switch
          checked={enabled}
          onChange={(e) => {
            setEnabled(e.target.checked)
            void saveConfig({ telemetryEnabled: e.target.checked })
          }}
        />
      </Stack>

      <Collapse in={enabled}>
        <TextField
          label="Endpoint"
          size="small"
          fullWidth
          placeholder="https://example.com/ingest"
          value={endpoint}
          onChange={(e) => setEndpoint(e.target.value)}
          onBlur={() => void saveConfig({ telemetryEndpoint: endpoint })}
          helperText="Nothing is sent while this is empty, even with sharing on."
        />
      </Collapse>

      <Divider />

      <Stack sx={{ gap: 0.75 }}>
        <Typography variant="subtitle2" sx={{ fontWeight: 600 }}>
          Recorded locally
        </Typography>
        <Stack direction="row" sx={{ gap: 3, flexWrap: 'wrap' }}>
          <Stack>
            <Typography variant="caption" color="text.secondary">
              Events
            </Typography>
            <Typography variant="body2" sx={{ fontWeight: 600 }}>
              {summary?.eventCount ?? 0}
            </Typography>
          </Stack>
          <Stack>
            <Typography variant="caption" color="text.secondary">
              Since
            </Typography>
            <Typography variant="body2">{formatWhen(summary?.firstEventTs)}</Typography>
          </Stack>
          <Stack>
            <Typography variant="caption" color="text.secondary">
              Crashes
            </Typography>
            <Typography variant="body2" sx={{ fontWeight: 600, color: summary?.crashes.length ? 'error.main' : undefined }}>
              {summary?.crashes.length ?? 0}
            </Typography>
          </Stack>
        </Stack>

        {!!summary?.kinds.length && (
          <Stack sx={{ gap: 0.25, mt: 0.5 }}>
            {summary.kinds.slice(0, 6).map((k) => (
              <Stack key={k.kind} direction="row" sx={{ justifyContent: 'space-between' }}>
                <Typography variant="caption" color="text.secondary" sx={{ fontFamily: '"Geist Mono", ui-monospace, monospace' }}>
                  {k.kind}
                </Typography>
                <Typography variant="caption" color="text.secondary">
                  {k.count}
                </Typography>
              </Stack>
            ))}
          </Stack>
        )}

        {!!summary?.crashes.length && (
          <Stack sx={{ gap: 0.5, mt: 1 }}>
            {summary.crashes.slice(0, 5).map((c) => (
              <Box
                key={c.ts}
                onClick={() => setExpandedCrash(expandedCrash === c.ts ? null : c.ts)}
                sx={{ border: 1, borderColor: 'divider', borderRadius: 2, p: 1, cursor: 'pointer' }}
              >
                <Typography variant="caption" sx={{ fontWeight: 600, display: 'block' }} noWrap>
                  {c.message}
                </Typography>
                <Typography variant="caption" color="text.secondary" sx={{ fontFamily: '"Geist Mono", ui-monospace, monospace' }}>
                  {c.location} · v{c.appVersion} · {formatWhen(c.ts)}
                </Typography>
                <Collapse in={expandedCrash === c.ts}>
                  <Box
                    component="pre"
                    sx={{
                      mt: 1,
                      p: 1,
                      bgcolor: 'action.hover',
                      borderRadius: 1,
                      fontSize: '0.6875rem',
                      overflowX: 'auto',
                      maxHeight: 200,
                    }}
                  >
                    {c.backtrace}
                  </Box>
                </Collapse>
              </Box>
            ))}
          </Stack>
        )}

        <Box>
          <Button size="small" onClick={() => void clearAll()} sx={{ textTransform: 'none', mt: 0.5 }}>
            Delete local diagnostics
          </Button>
        </Box>
      </Stack>

      <Divider />

      <Stack sx={{ gap: 1 }}>
        <Stack sx={{ gap: 0.25 }}>
          <Typography variant="subtitle2" sx={{ fontWeight: 600 }}>
            Report a bug
          </Typography>
          <Typography variant="caption" color="text.secondary">
            Saved as a file you can attach to an issue. Also uploaded if sharing is on.
          </Typography>
        </Stack>
        <TextField
          multiline
          minRows={3}
          size="small"
          fullWidth
          placeholder="What happened, and what did you expect instead?"
          value={description}
          onChange={(e) => setDescription(e.target.value)}
        />
        <Stack direction="row" sx={{ alignItems: 'center', justifyContent: 'space-between', gap: 2 }}>
          <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
            <Switch size="small" checked={includeDiagnostics} onChange={(e) => setIncludeDiagnostics(e.target.checked)} />
            <Typography variant="caption" color="text.secondary">
              Attach recent events and the latest crash
            </Typography>
          </Stack>
          <Button
            variant="contained"
            size="small"
            disableElevation
            disabled={sending || !description.trim()}
            onClick={() => void submit()}
            sx={{ textTransform: 'none' }}
          >
            {sending ? <CircularProgress size={14} /> : 'Submit report'}
          </Button>
        </Stack>
        {result && (
          <Alert severity={result.ok ? 'success' : 'error'} sx={{ fontSize: '0.8125rem' }}>
            {result.text}
          </Alert>
        )}
      </Stack>
    </Stack>
  )
}
