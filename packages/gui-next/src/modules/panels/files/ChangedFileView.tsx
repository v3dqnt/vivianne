import { useEffect, useState } from 'react'
import { Box, Stack, Typography, ButtonBase, alpha, useTheme } from '../../../ui'
import { call } from '../../../ipc'
import { FileContent } from '../../dock/components/FilePreview'
import { parseDiff } from './diff'

/** A changed file, opened on its diff with the contents one click away. */
export function ChangedFileView({ path, root }: { path: string; root: string }) {
  const theme = useTheme()
  const [mode, setMode] = useState<'diff' | 'file'>('diff')
  const [diff, setDiff] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    setDiff(null)
    void (async () => {
      // The working-tree diff is what the badges count; a fully staged change
      // has an empty one, so fall back to the index.
      let text = await call<string>('cmd_git_diff_file', { path: root, file: path, staged: false }).catch((e) => {
        if (!cancelled) setError(String(e))
        return ''
      })
      if (!text) text = await call<string>('cmd_git_diff_file', { path: root, file: path, staged: true }).catch(() => '')
      if (!cancelled) setDiff(text ?? '')
    })()
    return () => {
      cancelled = true
    }
  }, [path, root])

  const hunks = diff ? parseDiff(diff) : []

  const tab = (id: 'diff' | 'file', label: string) => (
    <ButtonBase
      key={id}
      onClick={() => setMode(id)}
      sx={{
        px: 1, height: 22, borderRadius: '6px', fontSize: '0.6875rem', fontWeight: 600,
        color: mode === id ? 'text.primary' : 'text.secondary',
        bgcolor: mode === id ? 'action.selected' : 'transparent',
        '&:hover': { bgcolor: mode === id ? 'action.selected' : 'action.hover' },
      }}
    >
      {label}
    </ButtonBase>
  )

  return (
    <Stack sx={{ border: 1, borderColor: 'divider', borderRadius: '8px', overflow: 'hidden' }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: 0.5, px: 0.75, py: 0.5, borderBottom: 1, borderColor: 'divider', bgcolor: 'background.chrome', flexShrink: 0 }}>
        {tab('diff', 'Diff')}
        {tab('file', 'File')}
        <Typography variant="caption" color="text.secondary" sx={{ ml: 'auto', fontSize: '0.625rem' }}>
          {hunks.length} {hunks.length === 1 ? 'hunk' : 'hunks'}
        </Typography>
      </Stack>

      {mode === 'file' ? (
        <Box sx={{ maxHeight: 420, overflow: 'auto' }}>
          <FileContent path={path} full fresh />
        </Box>
      ) : error ? (
        <Typography variant="caption" color="error.main" sx={{ p: 1.5 }}>{error}</Typography>
      ) : diff === null ? (
        <Typography variant="caption" color="text.secondary" sx={{ p: 1.5 }}>Loading diff…</Typography>
      ) : hunks.length === 0 ? (
        <Typography variant="caption" color="text.secondary" sx={{ p: 1.5 }}>No textual changes.</Typography>
      ) : (
        <Box sx={{ maxHeight: 420, overflow: 'auto', fontFamily: '"Geist Mono", ui-monospace, monospace', fontSize: '0.6875rem', lineHeight: 1.55 }}>
          {hunks.map((hunk, i) => (
            <Box key={`${hunk.header}-${i}`}>
              <Box
                sx={{
                  position: 'sticky', top: 0, zIndex: 1, px: 1, py: 0.25,
                  bgcolor: alpha(theme.palette.info.main, 0.12), color: 'text.secondary',
                  borderTop: i > 0 ? 1 : 0, borderColor: 'divider', whiteSpace: 'pre',
                }}
              >
                {hunk.header}
              </Box>
              {hunk.lines.map((line, j) => (
                <Stack
                  key={j}
                  direction="row"
                  sx={{
                    bgcolor: line.kind === 'add' ? alpha(theme.palette.success.main, 0.12) : line.kind === 'del' ? alpha(theme.palette.error.main, 0.12) : 'transparent',
                  }}
                >
                  <Box component="span" sx={{ width: 34, flexShrink: 0, textAlign: 'right', pr: 0.75, color: 'text.disabled', userSelect: 'none' }}>
                    {line.oldNo ?? ''}
                  </Box>
                  <Box component="span" sx={{ width: 34, flexShrink: 0, textAlign: 'right', pr: 0.75, color: 'text.disabled', userSelect: 'none' }}>
                    {line.newNo ?? ''}
                  </Box>
                  <Box component="span" sx={{ width: 12, flexShrink: 0, textAlign: 'center', color: line.kind === 'add' ? 'success.main' : line.kind === 'del' ? 'error.main' : 'text.disabled', userSelect: 'none' }}>
                    {line.kind === 'add' ? '+' : line.kind === 'del' ? '−' : ' '}
                  </Box>
                  <Box component="span" sx={{ whiteSpace: 'pre', pr: 1, minWidth: 0 }}>{line.text}</Box>
                </Stack>
              ))}
            </Box>
          ))}
        </Box>
      )}
    </Stack>
  )
}
