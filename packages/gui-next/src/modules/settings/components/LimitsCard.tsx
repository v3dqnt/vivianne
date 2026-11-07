import { useEffect, useState } from 'react'
import { Button, InputBase, Stack, Typography } from '../../../ui'
import { call } from '../../../ipc'
import { LIMITS, changedKeys, isOverridden, limitProblem, savedLimits, type LimitDef, type LimitValues } from '../lib/limits'

type Staged = Record<LimitDef['key'], string>

/**
 * Agent limits. Edits wait here until Save; a limit that differs from its default carries a marker, and Reset puts the default back in the
 * box (still unsaved, so it can be undone by leaving).
 */
export function LimitsCard() {
  const [saved, setSaved] = useState<LimitValues | null>(null)
  const [staged, setStaged] = useState<Staged | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    void call<Record<string, unknown>>('cmd_get_settings').then((s) => {
      const values = savedLimits(s)
      setSaved(values)
      setStaged({ shellTimeoutSecs: String(values.shellTimeoutSecs), maxActiveSubagents: String(values.maxActiveSubagents) })
    })
  }, [])

  if (!saved || !staged) return null

  const problems = LIMITS.map((d) => limitProblem(d, staged[d.key]))
  const dirty = changedKeys(saved, staged).length > 0
  const save = async () => {
    setError(null)
    try {
      const current = (await call<Record<string, unknown>>('cmd_get_settings')) ?? {}
      const next = { ...current, ...Object.fromEntries(LIMITS.map((d) => [d.key, Number(staged[d.key])])) }
      await call('cmd_save_settings', { settings: next })
      setSaved(savedLimits(next))
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e))
    }
  }

  return (
    <Stack sx={{ gap: 2 }}>
      <Stack sx={{ gap: 0.5 }}>
        <Typography variant="subtitle1" sx={{ fontWeight: 600 }}>
          Limits
        </Typography>
        <Typography variant="body2" color="text.secondary">
          They apply to the next command or subagent, with no restart.
        </Typography>
      </Stack>
      {LIMITS.map((d, i) => {
        const value = Number(staged[d.key])
        return (
          <Stack key={d.key} sx={{ gap: 0.5 }}>
            <Stack direction="row" sx={{ alignItems: 'center', gap: 1.5 }}>
              <Typography variant="body2" sx={{ fontWeight: 500, flex: 1 }}>
                {d.label}
                {isOverridden(d, saved[d.key]) && (
                  <Typography component="span" variant="caption" color="text.secondary" sx={{ ml: 1 }}>
                    changed from {d.fallback}
                  </Typography>
                )}
              </Typography>
              <InputBase
                value={staged[d.key]}
                onChange={(e) => setStaged({ ...staged, [d.key]: e.target.value })}
                inputProps={{ inputMode: 'numeric', 'aria-label': d.label }}
                sx={{ width: 80, fontSize: '0.8125rem', px: 1.25, py: 0.5, border: 1, borderColor: problems[i] ? 'error.main' : 'divider', borderRadius: '8px' }}
              />
              <Typography variant="caption" color="text.secondary" sx={{ width: 56 }}>
                {d.unit}
              </Typography>
              <Button size="small" disabled={value === d.fallback} onClick={() => setStaged({ ...staged, [d.key]: String(d.fallback) })} sx={{ textTransform: 'none' }}>
                Reset
              </Button>
            </Stack>
            <Typography variant="caption" color={problems[i] ? 'error.main' : 'text.secondary'}>
              {problems[i] ?? d.help}
            </Typography>
          </Stack>
        )
      })}
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1.5 }}>
        <Button size="small" variant="contained" disabled={!dirty || problems.some(Boolean)} onClick={() => void save()} sx={{ borderRadius: '8px', textTransform: 'none' }}>
          Save
        </Button>
        {error && <Typography variant="caption" color="error.main">{error}</Typography>}
      </Stack>
    </Stack>
  )
}
