import { useCallback, useEffect, useMemo, useState } from 'react'
import { Alert, Box, Button, ButtonBase, CircularProgress, Dialog, DialogActions, DialogContent, DialogTitle, InputBase, Stack, ToggleButton, ToggleButtonGroup, Typography, alpha, useTheme } from '../../../ui'
import { SearchNormalLinear, TickCircleLinear, DangerLinear } from '../../../icons'
import { call } from '../../../ipc'

type Source = 'nvidia' | 'nebius'
interface Entry {
  name: string
  dir: string
  hasCard: boolean
  files: number
  description: string
}
interface Catalog {
  commit: string
  owner: string
  repo: string
  skills: Entry[]
}
interface Finding {
  severity: 'info' | 'warn' | 'block'
  rule: string
  message: string
  file: string
  line: number
}
interface Preview {
  commit: string
  name: string
  files: { path: string; size: number }[]
  skillMd: string | null
  card: string | null
  report: { verdict: 'ok' | 'review' | 'block'; findings: Finding[] }
}
interface Installed {
  name: string
  source: string
  repo: string
  commit: string
  status: 'ok' | 'modified' | 'missing'
  changed: string[]
}

const SOURCES: { id: Source; label: string; note: string }[] = [
  { id: 'nvidia', label: 'NVIDIA', note: "NVIDIA's cataloged skills for CUDA-X, NeMo and RAG blueprints." },
  { id: 'nebius', label: 'Nebius', note: "Nebius's skills for researching its docs and running Serverless jobs and endpoints." },
]

const VERDICT: Record<Preview['report']['verdict'], { label: string; severity: 'success' | 'warning' | 'error'; text: string }> = {
  ok: { label: 'Nothing flagged', severity: 'success', text: 'The scan found nothing that looked wrong. That is not a guarantee: read the skill before you rely on it.' },
  review: { label: 'Read before installing', severity: 'warning', text: 'The scan flagged things worth reading. They may be fine, but look at each one.' },
  block: { label: 'Blocked', severity: 'error', text: 'The scan found something this app will not install.' },
}

/**
 * Skills from the NVIDIA and Nebius catalogs, installed with a record of
 * where they came from and a scan you read first. Installing adds files;
 * nothing runs, and a skill only reaches a prompt when you @mention it.
 */
export function SkillsHub({ onInstalled }: { onInstalled: () => void }) {
  const theme = useTheme()
  const [source, setSource] = useState<Source>('nvidia')
  const [catalog, setCatalog] = useState<Catalog | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [query, setQuery] = useState('')
  const [installed, setInstalled] = useState<Installed[]>([])
  const [review, setReview] = useState<{ entry: Entry; preview: Preview | null; error?: string } | null>(null)
  const [installing, setInstalling] = useState(false)
  const [described, setDescribed] = useState<Record<string, string>>({})
  const [limit, setLimit] = useState(40)

  const loadInstalled = useCallback(async () => setInstalled((await call<Installed[]>('cmd_skills_installed')) ?? []), [])

  useEffect(() => {
    void loadInstalled()
  }, [loadInstalled])

  useEffect(() => {
    let active = true
    setLoading(true)
    setError(null)
    setCatalog(null)
    setDescribed({})
    setLimit(40)
    call<Catalog>('cmd_skills_catalog', { sourceId: source })
      .then((c) => active && setCatalog(c))
      .catch((e) => active && setError(e instanceof Error ? e.message : String(e)))
      .finally(() => active && setLoading(false))
    return () => {
      active = false
    }
  }, [source])

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase()
    return (catalog?.skills ?? []).filter((s) => !q || s.name.toLowerCase().includes(q) || (described[s.dir] ?? '').toLowerCase().includes(q))
  }, [catalog, query, described])
  const visible = shown.slice(0, limit)

  // Descriptions for the rows on screen, fetched as they come into the list.
  const wanted = visible.filter((s) => described[s.dir] === undefined).map((s) => s.dir)
  const wantedKey = wanted.join('|')
  useEffect(() => {
    if (!catalog || wanted.length === 0) return
    let active = true
    call<Record<string, string>>('cmd_skills_describe', { sourceId: source, commit: catalog.commit, dirs: wanted.slice(0, 40) })
      .then((d) => active && d && setDescribed((cur) => ({ ...cur, ...d })))
      .catch(() => active && setDescribed((cur) => ({ ...cur, ...Object.fromEntries(wanted.slice(0, 40).map((d) => [d, ''])) })))
    return () => {
      active = false
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [catalog, source, wantedKey])

  const installedNames = new Map(installed.map((i) => [i.name, i]))

  const open = async (entry: Entry) => {
    setReview({ entry, preview: null })
    try {
      const preview = await call<Preview>('cmd_skills_preview', { sourceId: source, dir: entry.dir })
      setReview({ entry, preview })
    } catch (e) {
      setReview({ entry, preview: null, error: e instanceof Error ? e.message : String(e) })
    }
  }

  const install = async () => {
    if (!review) return
    setInstalling(true)
    try {
      await call('cmd_skills_install', { sourceId: source, dir: review.entry.dir })
      setReview(null)
      await loadInstalled()
      onInstalled()
    } catch (e) {
      setReview({ ...review, error: e instanceof Error ? e.message : String(e) })
    } finally {
      setInstalling(false)
    }
  }

  const remove = async (name: string) => {
    await call('cmd_skills_remove', { name }).catch((e) => setError(e instanceof Error ? e.message : String(e)))
    await loadInstalled()
    onInstalled()
  }

  const src = SOURCES.find((s) => s.id === source)!
  const verdict = review?.preview ? VERDICT[review.preview.report.verdict] : null
  const sevColor = (s: Finding['severity']) => (s === 'block' ? 'error.main' : s === 'warn' ? 'warning.main' : 'text.secondary')

  return (
    <Stack sx={{ gap: 1.25 }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1.5 }}>
        <ToggleButtonGroup size="small" exclusive value={source} onChange={(_: unknown, v: Source | null) => v && setSource(v)}>
          {SOURCES.map((s) => (
            <ToggleButton key={s.id} value={s.id} sx={{ textTransform: 'none', px: 1.5 }}>
              {s.label}
            </ToggleButton>
          ))}
        </ToggleButtonGroup>
        <Typography variant="caption" color="text.secondary" sx={{ flex: 1 }}>
          {src.note}
        </Typography>
      </Stack>

      <Stack direction="row" sx={{ alignItems: 'center', gap: 1, px: 1.5, height: 34, borderRadius: 2, border: 1, borderColor: 'divider' }}>
        <SearchNormalLinear size={14} color="currentColor" />
        <InputBase fullWidth placeholder={`Search ${src.label} skills`} value={query} onChange={(e) => setQuery(e.target.value)} sx={{ fontSize: '0.8125rem' }} />
      </Stack>

      {error && <Alert severity="error">{error}</Alert>}
      {loading && (
        <Stack direction="row" sx={{ alignItems: 'center', gap: 1, py: 1 }}>
          <CircularProgress size={14} />
          <Typography variant="caption" color="text.secondary">
            Reading the catalog…
          </Typography>
        </Stack>
      )}

      {catalog && (
        <Typography variant="caption" color="text.disabled">
          {catalog.owner}/{catalog.repo} at {catalog.commit.slice(0, 7)} · {catalog.skills.length} skills
        </Typography>
      )}

      {visible.map((s) => {
        const have = installedNames.get(s.name)
        return (
          <Stack key={s.dir} direction="row" sx={{ alignItems: 'center', gap: 1.5, py: 1, px: 1, borderRadius: 2, '&:hover': { bgcolor: 'action.hover' } }}>
            <Stack sx={{ flex: 1, minWidth: 0 }}>
              <Stack direction="row" sx={{ alignItems: 'baseline', gap: 0.75 }}>
                <Typography variant="body2" sx={{ fontWeight: 500 }} noWrap>
                  {s.name}
                </Typography>
                {s.hasCard && (
                  <Typography variant="caption" sx={{ fontSize: '0.625rem', fontWeight: 700, color: 'text.secondary', border: 1, borderColor: 'divider', borderRadius: '4px', px: 0.5 }}>
                    SKILL CARD
                  </Typography>
                )}
              </Stack>
              {described[s.dir] ? (
                <Typography variant="caption" color="text.secondary" sx={{ display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical', overflow: 'hidden' }}>
                  {described[s.dir]}
                </Typography>
              ) : null}
            </Stack>
            {have ? (
              <Typography variant="caption" sx={{ color: have.status === 'ok' ? 'success.main' : 'warning.main', fontWeight: 600, flexShrink: 0 }}>
                {have.status === 'ok' ? 'Installed' : 'Edited since install'}
              </Typography>
            ) : null}
            <Button size="small" variant="outlined" onClick={() => void open(s)} sx={{ textTransform: 'none', flexShrink: 0 }}>
              {have ? 'Review' : 'Install'}
            </Button>
          </Stack>
        )
      })}
      {shown.length > limit && (
        <Button size="small" onClick={() => setLimit((n) => n + 40)} sx={{ alignSelf: 'flex-start', textTransform: 'none' }}>
          Show more ({shown.length - limit} more)
        </Button>
      )}
      {catalog && shown.length === 0 && (
        <Typography variant="caption" color="text.secondary">
          No skills match.
        </Typography>
      )}

      {installed.length > 0 && (
        <Stack sx={{ gap: 0.5, mt: 1 }}>
          <Typography variant="caption" sx={{ fontWeight: 700, textTransform: 'uppercase', letterSpacing: 0.4, color: 'text.secondary' }}>
            Installed from a catalog
          </Typography>
          {installed.map((i) => (
            <Stack key={i.name} direction="row" sx={{ alignItems: 'center', gap: 1, px: 1, py: 0.5 }}>
              {i.status === 'ok' ? <TickCircleLinear size={14} color="currentColor" /> : <DangerLinear size={14} color="currentColor" />}
              <Stack sx={{ flex: 1, minWidth: 0 }}>
                <Typography variant="body2" noWrap>
                  {i.name}
                </Typography>
                <Typography variant="caption" color="text.secondary" noWrap>
                  {i.repo} at {i.commit.slice(0, 7)}
                  {i.status !== 'ok' ? ` · changed: ${i.changed.slice(0, 3).join(', ')}${i.changed.length > 3 ? '…' : ''}` : ''}
                </Typography>
              </Stack>
              <ButtonBase onClick={() => void remove(i.name)} sx={{ px: 1, height: 24, borderRadius: '7px', fontSize: '0.75rem', color: 'text.secondary', '&:hover': { bgcolor: 'action.hover', color: 'error.main' } }}>
                Remove
              </ButtonBase>
            </Stack>
          ))}
        </Stack>
      )}

      <Dialog open={!!review} onClose={() => !installing && setReview(null)} maxWidth="sm" fullWidth>
        <DialogTitle>{review?.entry.name}</DialogTitle>
        <DialogContent>
          {!review?.preview && !review?.error && (
            <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
              <CircularProgress size={14} />
              <Typography variant="body2">Fetching the skill and scanning it…</Typography>
            </Stack>
          )}
          {review?.error && <Alert severity="error">{review.error}</Alert>}
          {review?.preview && verdict && (
            <Stack sx={{ gap: 1.5 }}>
              <Alert severity={verdict.severity}>
                <b>{verdict.label}.</b> {verdict.text}
              </Alert>

              <Typography variant="caption" color="text.secondary">
                From {catalog?.owner}/{catalog?.repo} at commit <code>{review.preview.commit.slice(0, 12)}</code>. Cataloged by{' '}
                {source === 'nvidia' ? 'NVIDIA' : 'Nebius'}; this app does not verify their signatures. {review.preview.files.length} file
                {review.preview.files.length === 1 ? '' : 's'} will be saved, each with a recorded checksum so later edits are noticed.
              </Typography>

              {review.preview.report.findings.length > 0 && (
                <Stack sx={{ gap: 0.5 }}>
                  <Typography variant="caption" sx={{ fontWeight: 700, textTransform: 'uppercase', letterSpacing: 0.4, color: 'text.secondary' }}>
                    What the scan found
                  </Typography>
                  {review.preview.report.findings.slice(0, 40).map((f, i) => (
                    <Stack key={i} direction="row" sx={{ gap: 1, alignItems: 'baseline' }}>
                      <Typography variant="caption" sx={{ width: 44, fontWeight: 700, color: sevColor(f.severity), textTransform: 'uppercase', fontSize: '0.625rem' }}>
                        {f.severity}
                      </Typography>
                      <Typography variant="caption" sx={{ flex: 1, minWidth: 0 }}>
                        {f.message}{' '}
                        <Box component="span" sx={{ color: 'text.disabled', fontFamily: '"Geist Mono", ui-monospace, monospace' }}>
                          {f.file}
                          {f.line ? `:${f.line}` : ''}
                        </Box>
                      </Typography>
                    </Stack>
                  ))}
                </Stack>
              )}

              <Stack sx={{ gap: 0.5 }}>
                <Typography variant="caption" sx={{ fontWeight: 700, textTransform: 'uppercase', letterSpacing: 0.4, color: 'text.secondary' }}>
                  {review.preview.card ? 'Skill card' : 'No skill card'}
                </Typography>
                <Box component="pre" sx={{ m: 0, p: 1.25, maxHeight: 200, overflow: 'auto', borderRadius: '8px', bgcolor: alpha(theme.palette.text.primary, 0.04), fontSize: '0.6875rem', fontFamily: '"Geist Mono", ui-monospace, monospace', whiteSpace: 'pre-wrap' }}>
                  {review.preview.card ?? 'This skill has no card, so there is no stated owner, dependency list or known-risk note to read. Read SKILL.md instead.'}
                </Box>
              </Stack>

              <Stack sx={{ gap: 0.5 }}>
                <Typography variant="caption" sx={{ fontWeight: 700, textTransform: 'uppercase', letterSpacing: 0.4, color: 'text.secondary' }}>
                  SKILL.md
                </Typography>
                <Box component="pre" sx={{ m: 0, p: 1.25, maxHeight: 220, overflow: 'auto', borderRadius: '8px', bgcolor: alpha(theme.palette.text.primary, 0.04), fontSize: '0.6875rem', fontFamily: '"Geist Mono", ui-monospace, monospace', whiteSpace: 'pre-wrap' }}>
                  {review.preview.skillMd?.slice(0, 6000)}
                </Box>
              </Stack>
            </Stack>
          )}
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setReview(null)} disabled={installing} sx={{ textTransform: 'none' }}>
            Cancel
          </Button>
          <Button
            variant="contained"
            disabled={!review?.preview || review.preview.report.verdict === 'block' || installing}
            onClick={() => void install()}
            sx={{ textTransform: 'none' }}
          >
            {installing ? 'Installing…' : review?.preview?.report.verdict === 'review' ? 'I read it: install' : 'Install'}
          </Button>
        </DialogActions>
      </Dialog>
    </Stack>
  )
}
