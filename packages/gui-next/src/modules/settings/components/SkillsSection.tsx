import { useEffect, useMemo, useRef, useState } from 'react'
import { Stack, Typography, Divider, Button, InputBase } from '../../../ui'
import { MagicpenLinear, DocumentUploadLinear, SearchNormalLinear } from '../../../icons'
import { call } from '../../../ipc'

interface SkillInfo {
  name: string
  description?: string
  path?: string
}

/** Skills panel — lists every installed skill (paths shown) and lets you
 * upload a `.md` skill file straight into the default skills dir via the
 * browser File API (no fs plugin needed). */
export function SkillsSection() {
  const [skills, setSkills] = useState<SkillInfo[]>([])
  const [busy, setBusy] = useState(false)
  const [message, setMessage] = useState<string | null>(null)
  const [query, setQuery] = useState('')
  const inputRef = useRef<HTMLInputElement>(null)

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase()
    if (!q) return skills
    return skills.filter((s) => s.name.toLowerCase().includes(q) || s.description?.toLowerCase().includes(q))
  }, [skills, query])

  const refresh = async () => {
    const res = await call<{ skills?: SkillInfo[] }>('cmd_list_skills', { paths: [] })
    setSkills(res?.skills ?? [])
  }

  useEffect(() => {
    void refresh()
  }, [])

  const upload = async (file: File) => {
    if (!file.name.toLowerCase().endsWith('.md')) {
      setMessage('Only .md skill files are supported.')
      return
    }
    setBusy(true)
    setMessage(null)
    try {
      const content = await file.text()
      await call<string>('cmd_save_skill_file', { name: file.name, content })
      await refresh()
      setMessage(`Uploaded ${file.name}`)
    } catch (err) {
      setMessage(`Upload failed: ${err instanceof Error ? err.message : String(err)}`)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Stack sx={{ gap: 0.5 }}>
      <Stack direction="row" sx={{ alignItems: 'center', justifyContent: 'space-between', mb: 0.5 }}>
        <Typography variant="subtitle2" sx={{ fontWeight: 600 }}>
          Skills
        </Typography>
        <Button
          size="small"
          onClick={() => inputRef.current?.click()}
          disabled={busy}
          startIcon={<DocumentUploadLinear size={14} color="currentColor" />}
          sx={{ textTransform: 'none' }}
        >
          {busy ? 'Uploading…' : 'Upload .md'}
        </Button>
        <input
          ref={inputRef}
          type="file"
          accept=".md"
          hidden
          onChange={(e) => {
            const f = e.target.files?.[0]
            e.target.value = ''
            if (f) void upload(f)
          }}
        />
      </Stack>
      <Typography variant="caption" color="text.secondary" sx={{ mb: 1 }}>
        Skills are markdown files that teach the agent reusable behaviors. They live in your skills directory; upload one
        here to make it available immediately.
      </Typography>
      {message && (
        <Typography variant="caption" color="text.secondary" sx={{ mb: 1 }}>
          {message}
        </Typography>
      )}
      {skills.length > 0 && (
        <Stack
          direction="row"
          sx={{ alignItems: 'center', gap: 1, px: 1.5, py: 0.75, mb: 1, borderRadius: 2, border: 1, borderColor: 'divider' }}
        >
          <SearchNormalLinear size={14} color="currentColor" />
          <InputBase
            fullWidth
            placeholder="Search skills…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            sx={{ fontSize: '0.8125rem' }}
          />
        </Stack>
      )}
      <Divider sx={{ mb: 0.5 }} />
      {skills.length === 0 ? (
        <Typography variant="caption" color="text.secondary" sx={{ py: 1 }}>
          No skills installed yet.
        </Typography>
      ) : filtered.length === 0 ? (
        <Typography variant="caption" color="text.secondary" sx={{ py: 1 }}>
          No skills match "{query}".
        </Typography>
      ) : (
        filtered.map((s) => (
          <Stack key={s.name} direction="row" sx={{ alignItems: 'center', gap: 1.5, py: 1, px: 1, borderRadius: 2, '&:hover': { bgcolor: 'action.hover' } }}>
            <MagicpenLinear size={15} color="currentColor" />
            <Stack sx={{ flex: 1, minWidth: 0 }}>
              <Typography variant="body2" sx={{ fontWeight: 500 }} noWrap>
                {s.name}
              </Typography>
              {s.description && (
                <Typography variant="caption" color="text.secondary" noWrap>
                  {s.description}
                </Typography>
              )}
              {s.path && (
                <Typography variant="caption" noWrap sx={{ display: 'block', fontFamily: '"Geist Mono", ui-monospace, monospace', fontSize: '0.625rem', color: 'text.secondary' }}>
                  {s.path}
                </Typography>
              )}
            </Stack>
          </Stack>
        ))
      )}
    </Stack>
  )
}
