import { useState } from 'react'
import { Box, ButtonBase, Dialog, InputBase, Stack, Typography, alpha } from '../../../ui'
import { CloseLinear } from '../../../icons'
import type { Routine } from '../store/botsStore'
import type { TeamAgent } from '../store/teamStore'
import { Avatar, colorOf, faceOf } from './bits'

type Repeat = 'once' | 'daily' | 'weekdays' | 'weekly' | 'hourly'

const REPEATS: { id: Repeat; label: string }[] = [
  { id: 'once', label: 'Once' },
  { id: 'daily', label: 'Every day' },
  { id: 'weekdays', label: 'Weekdays' },
  { id: 'weekly', label: 'Every week' },
  { id: 'hourly', label: 'Every hour' },
]

const pad = (n: number) => String(n).padStart(2, '0')
const isoDate = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`

/** The schedule as cron (local time, which is what the scheduler reads). */
function toCron(repeat: Repeat, date: string, time: string): string {
  const [h, m] = time.split(':').map(Number)
  const d = new Date(`${date}T00:00`)
  switch (repeat) {
    case 'once':
      return `${m} ${h} ${d.getDate()} ${d.getMonth() + 1} *`
    case 'daily':
      return `${m} ${h} * * *`
    case 'weekdays':
      return `${m} ${h} * * 1-5`
    case 'weekly':
      return `${m} ${h} * * ${d.getDay()}`
    case 'hourly':
      return `${m} * * * *`
  }
}

/** Read a cron this dialog wrote back into its fields; anything else is kept as is. */
function fromCron(cron: string | undefined, once: boolean | undefined): { repeat: Repeat; date: string; time: string } | null {
  if (!cron) return null
  const f = cron.trim().split(/\s+/)
  if (f.length !== 5) return null
  const [m, h, dom, mon, dow] = f
  const time = `${pad(Number(h) || 0)}:${pad(Number(m) || 0)}`
  const today = new Date()
  if (h === '*') return { repeat: 'hourly', date: isoDate(today), time: `00:${pad(Number(m) || 0)}` }
  if (once && dom !== '*' && mon !== '*') return { repeat: 'once', date: isoDate(new Date(today.getFullYear(), Number(mon) - 1, Number(dom))), time }
  if (dom === '*' && mon === '*' && dow === '*') return { repeat: 'daily', date: isoDate(today), time }
  if (dow === '1-5') return { repeat: 'weekdays', date: isoDate(today), time }
  if (/^\d$/.test(dow)) {
    const d = new Date(today)
    d.setDate(today.getDate() + ((Number(dow) - today.getDay() + 7) % 7))
    return { repeat: 'weekly', date: isoDate(d), time }
  }
  return null
}

const fieldSx = (ring: string) =>
  ({
    fontSize: '14px',
    px: '12px',
    py: '8px',
    borderRadius: '10px',
    bgcolor: 'action.hover',
    colorScheme: 'dark light',
    '&.ui-focused': { bgcolor: 'background.paper', boxShadow: `inset 0 0 0 1.5px ${ring}` },
  }) as const

function Label({ children }: { children: React.ReactNode }) {
  return <Typography sx={{ fontSize: '12px', fontWeight: 500, color: 'text.secondary' }}>{children}</Typography>
}

/** Make or change one of an agent's routines: what should happen, when, and
 * what to tell it each time. */
export function RoutineDialog({
  agent,
  initial,
  onSave,
  onClose,
}: {
  agent: TeamAgent
  initial: Partial<Routine> | null
  onSave: (r: Partial<Routine>) => Promise<void>
  onClose: () => void
}) {
  const color = colorOf(agent)
  const parsed = fromCron(initial?.cron, initial?.once)
  const soon = new Date(Date.now() + 60 * 60 * 1000)
  const [title, setTitle] = useState(initial?.title ?? '')
  const [prompt, setPrompt] = useState(initial?.prompt ?? '')
  const [repeat, setRepeat] = useState<Repeat>(parsed?.repeat ?? 'daily')
  const [date, setDate] = useState(parsed?.date ?? isoDate(soon))
  const [time, setTime] = useState(parsed?.time ?? `${pad(soon.getHours())}:00`)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  // A schedule this dialog can't show (written by hand or by the agent) is kept unless changed.
  const [custom, setCustom] = useState(initial?.cron && !parsed ? initial.cron : null)

  const save = async () => {
    if (!title.trim()) return
    setSaving(true)
    setError(null)
    try {
      await onSave({ ...initial, title: title.trim(), prompt: prompt.trim() || title.trim(), cron: custom ?? toCron(repeat, date, time), once: custom ? initial?.once : repeat === 'once' })
      onClose()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setSaving(false)
    }
  }

  return (
    <Dialog open onClose={onClose} maxWidth="sm" fullWidth slotProps={{ paper: { sx: { p: 0, borderRadius: '16px', overflow: 'hidden' } } }}>
      <Stack direction="row" sx={{ alignItems: 'center', px: '24px', pt: '20px' }}>
        <Typography sx={{ flex: 1, fontSize: '15px', fontWeight: 600 }}>{initial?.id ? 'Edit routine' : 'New routine'}</Typography>
        <ButtonBase onClick={onClose} aria-label="Close" sx={{ width: 28, height: 28, borderRadius: '8px', color: 'text.secondary', '&:hover': { bgcolor: 'action.hover' } }}>
          <CloseLinear size={16} color="currentColor" />
        </ButtonBase>
      </Stack>

      <Stack sx={{ px: '24px', py: '16px', gap: '20px' }}>
        <InputBase
          autoFocus
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          placeholder="What should happen?"
          sx={{ fontSize: '22px', fontWeight: 600, py: '6px', borderBottom: 1.5, borderColor: title ? alpha(color, 0.6) : 'divider', '&.ui-focused': { borderColor: color } }}
        />

        <Stack sx={{ gap: '10px' }}>
          <Label>When</Label>
          {custom ? (
            <Stack direction="row" sx={{ alignItems: 'center', gap: '10px' }}>
              <Typography sx={{ fontSize: '13px', fontFamily: '"Geist Mono", ui-monospace, monospace' }}>{custom}</Typography>
              <ButtonBase onClick={() => setCustom(null)} sx={{ fontSize: '12px', color: 'text.secondary', textDecoration: 'underline' }}>
                Change
              </ButtonBase>
            </Stack>
          ) : (
            <>
              <Stack direction="row" sx={{ gap: '8px', flexWrap: 'wrap' }}>
                {REPEATS.map((r) => (
                  <ButtonBase
                    key={r.id}
                    onClick={() => setRepeat(r.id)}
                    aria-pressed={repeat === r.id}
                    sx={{
                      height: 32,
                      px: '14px',
                      borderRadius: '999px',
                      fontSize: '13px',
                      fontWeight: 500,
                      color: repeat === r.id ? 'text.primary' : 'text.secondary',
                      bgcolor: repeat === r.id ? alpha(color, 0.18) : 'action.hover',
                      boxShadow: repeat === r.id ? `inset 0 0 0 1.5px ${alpha(color, 0.7)}` : 'none',
                    }}
                  >
                    {r.label}
                  </ButtonBase>
                ))}
              </Stack>
              <Stack direction="row" sx={{ gap: '8px', alignItems: 'center' }}>
                {(repeat === 'once' || repeat === 'weekly') && (
                  <InputBase type="date" value={date} onChange={(e) => setDate(e.target.value)} sx={fieldSx(color)} inputProps={{ 'aria-label': 'Date' }} />
                )}
                {repeat !== 'hourly' ? (
                  <InputBase type="time" value={time} onChange={(e) => setTime(e.target.value)} sx={fieldSx(color)} inputProps={{ 'aria-label': 'Time' }} />
                ) : (
                  <Typography sx={{ fontSize: '13px', color: 'text.secondary' }}>On the hour</Typography>
                )}
              </Stack>
            </>
          )}
        </Stack>

        <Stack sx={{ gap: '10px' }}>
          <Label>Who does it</Label>
          <Stack direction="row" sx={{ alignItems: 'center', gap: '10px', px: '12px', py: '8px', borderRadius: '12px', bgcolor: alpha(color, 0.12), width: 'fit-content' }}>
            <Avatar {...faceOf(agent)} size={24} />
            <Typography sx={{ fontSize: '13px', fontWeight: 500 }}>{agent.name}</Typography>
          </Stack>
        </Stack>

        <Stack sx={{ gap: '10px' }}>
          <Label>What to tell it each time</Label>
          <InputBase
            multiline
            minRows={4}
            maxRows={10}
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
            placeholder="Check my inbox and draft replies to anything urgent. Send me a summary."
            sx={{ ...fieldSx(color), alignItems: 'flex-start', lineHeight: 1.5 }}
          />
          <Typography sx={{ fontSize: '12px', color: 'text.disabled', lineHeight: 1.5 }}>
            Runs while Vivianne is open on this computer; a run missed while it was closed happens when it is back. Results arrive in {agent.name}'s chat.
          </Typography>
        </Stack>
      </Stack>

      <Stack direction="row" sx={{ alignItems: 'center', gap: '8px', px: '24px', py: '16px', borderTop: 1, borderColor: 'divider' }}>
        <Typography sx={{ flex: 1, minWidth: 0, fontSize: '12px', color: 'error.main' }}>{error}</Typography>
        <ButtonBase onClick={onClose} sx={{ height: 36, px: '16px', borderRadius: '10px', fontSize: '13px', color: 'text.secondary', '&:hover': { bgcolor: 'action.hover', color: 'text.primary' } }}>
          Cancel
        </ButtonBase>
        <ButtonBase
          onClick={() => void save()}
          disabled={saving || !title.trim()}
          sx={{ height: 36, minWidth: 88, px: '18px', borderRadius: '10px', fontSize: '13px', fontWeight: 600, bgcolor: color, color: (t) => t.palette.getContrastText(color), '&:disabled': { opacity: 0.45 } }}
        >
          <Box component="span">Save</Box>
        </ButtonBase>
      </Stack>
    </Dialog>
  )
}
