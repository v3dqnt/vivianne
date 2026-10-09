import { useEffect, useState } from 'react'
import { Box, Button, Checkbox, CircularProgress, Dialog, DialogActions, DialogContent, DialogTitle, Stack, Switch, Typography } from '../../../ui'
import { FlashLinear } from '../../../icons'
import { call } from '../../../ipc'

/** Mirrors `supercharge::Candidate`. */
interface Candidate {
  id: string
  name: string
  exe: string
  title: string | null
  background: boolean
  processes: number
  memoryMb: number
  recommended: boolean
  note: string | null
}

/** Mirrors `supercharge::SuperchargeState`. */
interface SuperchargeState {
  on: boolean
  since: number | null
  previousPowerPlan: string | null
  closed: { name: string; exe: string }[]
}

/** Mirrors `supercharge::ApplyReport`. */
interface ApplyReport {
  closed: string[]
  stillOpen: string[]
  powerPlan: string
  freedMb: number
  state: SuperchargeState
}

const mb = (n: number) => (n >= 1024 ? `${(n / 1024).toFixed(1)} GB` : `${n} MB`)

/**
 * Supercharge: close what the user doesn't need right now and give Vivianne
 * the machine. Turning it on lists exactly what would be closed and asks
 * first; nothing closes without that go-ahead. See `supercharge.rs` for what
 * it will and won't touch.
 */
export function SuperchargeSubsection() {
  const [state, setState] = useState<SuperchargeState | null>(null)
  const [asking, setAsking] = useState<'on' | 'off' | null>(null)
  const [candidates, setCandidates] = useState<Candidate[] | null>(null)
  const [picked, setPicked] = useState<Set<string>>(new Set())
  const [reopen, setReopen] = useState(true)
  const [busy, setBusy] = useState(false)
  const [report, setReport] = useState<ApplyReport | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    void call<SuperchargeState>('cmd_supercharge_status').then((s) => setState(s ?? null))
  }, [])

  const startOn = async () => {
    setAsking('on')
    setReport(null)
    setError(null)
    setCandidates(null)
    let list: Candidate[] = []
    try {
      list = (await call<Candidate[]>('cmd_supercharge_scan')) ?? []
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
    setCandidates(list)
    setPicked(new Set(list.filter((c) => c.recommended).map((c) => c.id)))
  }

  const confirmOn = async () => {
    setBusy(true)
    setError(null)
    try {
      const r = await call<ApplyReport>('cmd_supercharge_apply', { ids: [...picked] })
      if (r) {
        setReport(r)
        setState(r.state)
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  const confirmOff = async () => {
    setBusy(true)
    try {
      const failed = (await call<string[]>('cmd_supercharge_restore', { reopen })) ?? []
      setState({ on: false, since: null, previousPowerPlan: null, closed: [] })
      setAsking(null)
      if (failed.length) setError(`Couldn't reopen ${failed.join(', ')}. Start ${failed.length === 1 ? 'it' : 'them'} yourself.`)
    } finally {
      setBusy(false)
    }
  }

  const toggle = (id: string) =>
    setPicked((p) => {
      const next = new Set(p)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })

  const selectedMb = (candidates ?? []).filter((c) => picked.has(c.id)).reduce((n, c) => n + c.memoryMb, 0)
  const on = !!state?.on
  const closedCount = state?.closed.length ?? 0

  return (
    <Stack sx={{ gap: 0.5 }}>
      <Typography variant="subtitle2" sx={{ fontWeight: 600 }}>
        Performance
      </Typography>
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1.5, py: 0.75, px: 1, borderRadius: 2, '&:hover': { bgcolor: 'action.hover' } }}>
        <FlashLinear size={15} color="currentColor" style={{ flexShrink: 0 }} />
        <Stack sx={{ flex: 1, minWidth: 0 }}>
          <Typography variant="body2" sx={{ fontWeight: 500 }}>
            Supercharge
          </Typography>
          <Typography variant="caption" color="text.secondary">
            {on
              ? `On${closedCount ? `, closed ${closedCount} app${closedCount === 1 ? '' : 's'}` : ''}. Turn off to restore the power plan${closedCount ? ' and reopen what it closed' : ''}.`
              : "Closes the apps and background clients you don't need right now and gives Vivianne the machine: more memory and CPU for the AI, more bandwidth for the internet. Asks you first."}
          </Typography>
        </Stack>
        <Switch size="small" checked={on} onChange={(e) => (e.target.checked ? void startOn() : setAsking('off'))} />
      </Stack>
      {error && !asking && (
        <Typography variant="caption" color="error.main" sx={{ px: 1 }}>
          {error}
        </Typography>
      )}

      <Dialog open={asking === 'on'} onClose={() => !busy && setAsking(null)} maxWidth="sm" fullWidth>
        <DialogTitle sx={{ fontSize: '1rem', fontWeight: 700 }}>{report ? 'Supercharged' : 'Supercharge this PC?'}</DialogTitle>
        <DialogContent>
          {report ? (
            <Stack sx={{ gap: 1 }}>
              <Typography variant="body2">
                {report.closed.length ? `Closed ${report.closed.join(', ')}, freeing about ${mb(report.freedMb)}.` : 'Nothing needed closing.'}
              </Typography>
              {report.stillOpen.length > 0 && (
                <Typography variant="body2" color="warning.main">
                  Still open: {report.stillOpen.join(', ')}. They may be asking whether to save. Answer them, or leave them be.
                </Typography>
              )}
              <Typography variant="body2" color="text.secondary">
                {report.powerPlan}. Vivianne now runs at above-normal priority.
              </Typography>
            </Stack>
          ) : (
            <Stack sx={{ gap: 1.5 }}>
              <Typography variant="body2" color="text.secondary" sx={{ lineHeight: 1.55 }}>
                These will be asked to close the normal way, so anything with unsaved work can ask you to save first. Background clients
                that ignore the request (sync, launchers) are ended. Vivianne then runs at above-normal priority and Windows switches to
                the High performance power plan. Turning it off undoes both, and can reopen these.
              </Typography>
              {!candidates ? (
                <Stack direction="row" sx={{ alignItems: 'center', gap: 1, py: 2 }}>
                  <CircularProgress size={16} />
                  <Typography variant="body2" color="text.secondary">
                    Looking at what's running...
                  </Typography>
                </Stack>
              ) : candidates.length === 0 ? (
                <Typography variant="body2">Nothing to close: only Vivianne and Windows are running.</Typography>
              ) : (
                <Box sx={{ maxHeight: 340, overflowY: 'auto', border: 1, borderColor: 'divider', borderRadius: '10px' }}>
                  {candidates.map((c) => (
                    <Stack
                      key={c.id}
                      component="label"
                      direction="row"
                      sx={{ alignItems: 'center', gap: 1, px: 1, py: 0.5, cursor: 'pointer', '&:hover': { bgcolor: 'action.hover' } }}
                    >
                      <Checkbox size="small" checked={picked.has(c.id)} onChange={() => toggle(c.id)} />
                      <Stack sx={{ flex: 1, minWidth: 0 }}>
                        <Typography variant="body2" sx={{ fontWeight: 500 }} noWrap>
                          {c.name}
                          <Typography component="span" variant="caption" color="text.secondary">
                            {c.background ? ' · in the background' : c.title ? ` · ${c.title}` : ''}
                          </Typography>
                        </Typography>
                        {c.note && (
                          <Typography variant="caption" color="warning.main" noWrap>
                            {c.note}
                          </Typography>
                        )}
                      </Stack>
                      <Typography variant="caption" color="text.secondary" sx={{ flexShrink: 0 }}>
                        {mb(c.memoryMb)}
                      </Typography>
                    </Stack>
                  ))}
                </Box>
              )}
              {error && (
                <Typography variant="caption" color="error.main">
                  {error}
                </Typography>
              )}
            </Stack>
          )}
        </DialogContent>
        <DialogActions>
          {report ? (
            <Button variant="contained" disableElevation onClick={() => setAsking(null)}>
              Done
            </Button>
          ) : (
            <>
              <Button color="inherit" onClick={() => setAsking(null)} disabled={busy}>
                Cancel
              </Button>
              <Button variant="contained" disableElevation onClick={() => void confirmOn()} disabled={busy || !candidates}>
                {busy
                  ? 'Closing...'
                  : picked.size
                    ? `Close ${picked.size} and supercharge${selectedMb ? ` (${mb(selectedMb)})` : ''}`
                    : 'Supercharge without closing anything'}
              </Button>
            </>
          )}
        </DialogActions>
      </Dialog>

      <Dialog open={asking === 'off'} onClose={() => !busy && setAsking(null)} maxWidth="xs" fullWidth>
        <DialogTitle sx={{ fontSize: '1rem', fontWeight: 700 }}>Turn off Supercharge?</DialogTitle>
        <DialogContent>
          <Stack sx={{ gap: 1 }}>
            <Typography variant="body2" color="text.secondary">
              The power plan goes back to what it was and Vivianne returns to normal priority.
            </Typography>
            {closedCount > 0 && (
              <Stack component="label" direction="row" sx={{ alignItems: 'center', gap: 0.5, cursor: 'pointer' }}>
                <Checkbox size="small" checked={reopen} onChange={(e) => setReopen(e.target.checked)} />
                <Typography variant="body2">Reopen {state?.closed.map((c) => c.name).join(', ')}</Typography>
              </Stack>
            )}
          </Stack>
        </DialogContent>
        <DialogActions>
          <Button color="inherit" onClick={() => setAsking(null)} disabled={busy}>
            Keep it on
          </Button>
          <Button variant="contained" disableElevation onClick={() => void confirmOff()} disabled={busy}>
            Turn off
          </Button>
        </DialogActions>
      </Dialog>
    </Stack>
  )
}
