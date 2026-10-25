import { useEffect, useMemo, useState } from 'react'
import { Stack, Typography, Button, Dialog, DialogTitle, DialogContent, DialogActions, TextField, Box, ButtonGroup, getContrastRatio, Alert } from '../../../ui'
import { allPalettes, themeTokens, type PaletteDef, type TokenSet } from '../../../theme'
import { useThemeStore, type ThemeMode } from '../store/themeStore'

const GROUPS: { title: string; tokens: (keyof TokenSet)[] }[] = [
  { title: 'Surfaces', tokens: ['background', 'card', 'popover', 'secondary', 'muted', 'accent'] },
  { title: 'Text', tokens: ['foreground', 'mutedForeground'] },
  { title: 'Accent', tokens: ['primary', 'primaryForeground', 'ring'] },
  { title: 'Borders', tokens: ['border', 'input'] },
  { title: 'Status', tokens: ['destructive'] },
]

const LABELS: Record<keyof TokenSet, string> = {
  background: 'Panes',
  card: 'Cards',
  popover: 'Menus',
  secondary: 'Secondary buttons',
  muted: 'Muted surfaces',
  accent: 'Hover and selection',
  foreground: 'Text',
  mutedForeground: 'Secondary text',
  primary: 'Accent',
  primaryForeground: 'Text on accent',
  ring: 'Focus ring',
  border: 'Dividers',
  input: 'Field borders',
  destructive: 'Danger',
}

/** The contrast checks a theme must pass for text to stay readable. */
const CHECKS: { fg: keyof TokenSet; bg: keyof TokenSet; label: string }[] = [
  { fg: 'foreground', bg: 'background', label: 'Text on panes' },
  { fg: 'mutedForeground', bg: 'background', label: 'Secondary text on panes' },
  { fg: 'foreground', bg: 'card', label: 'Text on cards' },
  { fg: 'primaryForeground', bg: 'primary', label: 'Text on the accent' },
]

function ratio(fg: string, bg: string): number | null {
  try {
    return getContrastRatio(fg, bg)
  } catch {
    return null
  }
}

const isHex = (c: string) => /^#[0-9a-f]{6}$/i.test(c.trim())

function idFrom(label: string, taken: string[]): string {
  const base = label.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 34) || 'theme'
  let id = base.length < 2 ? `${base}-theme` : base
  for (let n = 2; taken.includes(id); n++) id = `${base}-${n}`
  return id
}

/** A theme made complete: every token for both modes, from its base. */
function complete(base: PaletteDef, custom: PaletteDef[]): PaletteDef {
  return {
    ...base,
    light: themeTokens('light', base.id, { custom: [...custom.filter((c) => c.id !== base.id), base] }),
    dark: themeTokens('dark', base.id, { custom: [...custom.filter((c) => c.id !== base.id), base] }),
  }
}

/**
 * Make or edit a theme: every colour for light and dark, with the whole app
 * previewing it as you go and contrast checked. Cancel puts everything back.
 */
export function ThemeBuilder({ base, editing, onClose }: { base: PaletteDef; editing: boolean; onClose: () => void }) {
  const customThemes = useThemeStore((s) => s.customThemes)
  const mode = useThemeStore((s) => s.mode)
  const setMode = useThemeStore((s) => s.setMode)
  const setPreview = useThemeStore((s) => s.setPreview)
  const saveTheme = useThemeStore((s) => s.saveTheme)
  const setPalette = useThemeStore((s) => s.setPalette)
  const startMode = useMemo(() => useThemeStore.getState().modePref, [])

  const [draft, setDraft] = useState<PaletteDef>(() => {
    const full = complete(base, customThemes)
    if (editing) return full
    const taken = allPalettes(customThemes).map((p) => p.id)
    const label = `${base.label} (mine)`
    return { ...full, id: idFrom(label, taken), label, symbol: '✎' }
  })
  const [error, setError] = useState<string | null>(null)

  // Preview: paint the draft while the builder is open.
  useEffect(() => {
    setPreview({ ...draft, swatches: [draft.dark.primary!, draft.dark.background!, draft.light.primary!, draft.light.background!] })
  }, [draft, setPreview])
  useEffect(() => () => setPreview(null), [setPreview])

  const tokens = draft[mode] as TokenSet
  const set = (k: keyof TokenSet, v: string) => setDraft((d) => ({ ...d, [mode]: { ...d[mode], [k]: v } }))

  const cancel = () => {
    setPreview(null)
    setMode(startMode)
    onClose()
  }

  const save = async () => {
    setError(null)
    if (!draft.label.trim()) return setError('Give the theme a name.')
    const theme: PaletteDef = { ...draft, swatches: [draft.dark.primary!, draft.dark.background!, draft.light.primary!, draft.light.background!] }
    try {
      await saveTheme(theme)
      setPalette(theme.id)
      setPreview(null)
      onClose()
    } catch (e) {
      setError(String(e))
    }
  }

  return (
    <Dialog open onClose={cancel} maxWidth="sm" fullWidth>
      <DialogTitle>{editing ? `Edit ${base.label}` : 'New theme'}</DialogTitle>
      <DialogContent>
        <Stack sx={{ gap: 1.5, pt: 0.5 }}>
          <Stack direction="row" sx={{ gap: 1, alignItems: 'center' }}>
            <TextField
              size="small"
              label="Name"
              value={draft.label}
              onChange={(e) => setDraft((d) => ({ ...d, label: e.target.value }))}
              sx={{ flex: 1 }}
            />
            <ButtonGroup size="small">
              {(['dark', 'light'] as ThemeMode[]).map((m) => (
                <Button
                  key={m}
                  onClick={() => setMode(m)}
                  sx={{ textTransform: 'capitalize', bgcolor: mode === m ? 'action.selected' : 'transparent' }}
                >
                  {m}
                </Button>
              ))}
            </ButtonGroup>
          </Stack>
          <Typography variant="caption" color="text.secondary">
            The whole app shows this theme while you edit. Editing the {mode} variant; switch above for the other.
          </Typography>

          {GROUPS.map((g) => (
            <Stack key={g.title} sx={{ gap: 0.5 }}>
              <Typography variant="caption" sx={{ fontWeight: 600, color: 'text.secondary', textTransform: 'uppercase', letterSpacing: 0.4 }}>
                {g.title}
              </Typography>
              <Box sx={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(230px, 1fr))', gap: 0.75 }}>
                {g.tokens.map((k) => {
                  const v = tokens[k] ?? ''
                  return (
                    <Stack key={k} direction="row" sx={{ alignItems: 'center', gap: 1 }}>
                      <Box
                        component="input"
                        type="color"
                        value={isHex(v) ? v : '#888888'}
                        onChange={(e: React.ChangeEvent<HTMLInputElement>) => set(k, e.target.value)}
                        aria-label={LABELS[k]}
                        sx={{ width: 30, height: 26, p: 0, border: 1, borderColor: 'divider', borderRadius: 1, bgcolor: 'transparent', cursor: 'pointer', flexShrink: 0 }}
                      />
                      <TextField size="small" label={LABELS[k]} value={v} onChange={(e) => set(k, e.target.value)} sx={{ flex: 1 }} />
                    </Stack>
                  )
                })}
              </Box>
            </Stack>
          ))}

          <Stack sx={{ gap: 0.25 }}>
            <Typography variant="caption" sx={{ fontWeight: 600, color: 'text.secondary', textTransform: 'uppercase', letterSpacing: 0.4 }}>
              Readability ({mode})
            </Typography>
            {CHECKS.map((c) => {
              const r = ratio(tokens[c.fg] ?? '', tokens[c.bg] ?? '')
              const ok = r !== null && r >= 4.5
              return (
                <Typography key={c.label} variant="caption" color={ok ? 'text.secondary' : 'warning.main'}>
                  {ok ? '✓' : '⚠'} {c.label}: {r === null ? 'unknown' : `${r.toFixed(1)}:1`}
                  {!ok && r !== null ? ' (below 4.5:1, hard to read)' : ''}
                </Typography>
              )
            })}
          </Stack>
          {error && (
            <Alert severity="error">
              {error}
            </Alert>
          )}
        </Stack>
      </DialogContent>
      <DialogActions>
        <Button onClick={cancel} sx={{ textTransform: 'none' }}>
          Cancel
        </Button>
        <Button variant="contained" onClick={() => void save()} sx={{ textTransform: 'none' }}>
          Save theme
        </Button>
      </DialogActions>
    </Dialog>
  )
}
