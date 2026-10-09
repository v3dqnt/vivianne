import { useEffect, useState } from 'react'
import { Button, InputBase, Link, Stack, Typography } from '../../../ui'
import { call } from '../../../ipc'

const DEFAULT_REPO = 'https://github.com/v3dqnt/vivianne-guides'

/**
 * Where Vivianne's guides come from (`agent::guides`): how her documents
 * should look and how she works on projects. A fork with your own brand and
 * way of working replaces the default.
 */
export function GuidesSubsection() {
  const [settings, setSettings] = useState<Record<string, unknown> | null>(null)
  const [draft, setDraft] = useState('')
  const [saved, setSaved] = useState(false)

  useEffect(() => {
    void call<Record<string, unknown>>('cmd_get_settings').then((s) => {
      setSettings(s ?? {})
      setDraft(((s?.guidesRepo as string | undefined) ?? '').trim())
    })
  }, [])

  const save = async (value: string) => {
    const next = { ...(settings ?? {}), guidesRepo: value.trim() || null }
    setSettings(next)
    await call('cmd_save_settings', { settings: next })
    setSaved(true)
    window.setTimeout(() => setSaved(false), 1500)
  }

  const valid = !draft.trim() || /^(https:\/\/|git@)/.test(draft.trim())

  return (
    <Stack sx={{ gap: 0.75 }}>
      <Typography variant="subtitle2" sx={{ fontWeight: 600 }}>
        Guides
      </Typography>
      <Typography variant="caption" color="text.secondary" sx={{ lineHeight: 1.5 }}>
        How Vivianne designs documents and works on projects, from{' '}
        <Link href={DEFAULT_REPO} target="_blank" rel="noreferrer">
          vivianne-guides
        </Link>
        . Fork it with your own brand, templates and way of working, and paste your fork here. Leave empty for the default.
      </Typography>
      <Stack direction="row" sx={{ gap: 0.75 }}>
        <InputBase
          placeholder={DEFAULT_REPO}
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          sx={{ fontSize: '0.8125rem', px: 1.25, py: 0.5, border: 1, borderColor: valid ? 'divider' : 'error.main', borderRadius: '8px', flex: 1 }}
        />
        <Button size="small" variant="contained" disabled={!valid} onClick={() => void save(draft)} sx={{ borderRadius: '8px' }}>
          {saved ? 'Saved' : 'Save'}
        </Button>
      </Stack>
    </Stack>
  )
}
