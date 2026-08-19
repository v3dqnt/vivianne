import { useEffect, useState } from 'react'
import Box from '@mui/material/Box'
import Typography from '@mui/material/Typography'
import { call } from '../../../ipc'
import { CodeBlock } from '../../ai/components/CodeBlock'

function langFromPath(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase() ?? ''
  const map: Record<string, string> = {
    ts: 'typescript',
    tsx: 'tsx',
    js: 'javascript',
    jsx: 'jsx',
    rs: 'rust',
    py: 'python',
    json: 'json',
    md: 'markdown',
    css: 'css',
    html: 'html',
    toml: 'ini',
    yml: 'yaml',
    yaml: 'yaml',
    sh: 'bash',
    bash: 'bash',
  }
  return map[ext] ?? 'text'
}

/** Reads a file via `cmd_read_file_content` and renders it with shiki
 * highlighting (reusing the chat's CodeBlock). Used both inline under a row
 * in the file tree and, at full size, in the fullscreen viewer — the tree IS
 * the preview surface now, so this is a plain content renderer with no dock
 * state of its own.
 *
 * @param wrap reflow long lines instead of scrolling horizontally — right
 *   for the narrow inline preview, wrong for the fullscreen view where the
 *   original line structure is what you came to read.
 */
export function FileContent({ path, wrap = true }: { path: string; wrap?: boolean }) {
  const [content, setContent] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    setContent(null)
    setError(null)
    let stale = false
    void call<string>('cmd_read_file_content', { path })
      .then((c) => {
        if (stale) return
        if (typeof c === 'string') setContent(c)
        else setError('Could not read file')
      })
      .catch((e) => {
        if (!stale) setError(e instanceof Error ? e.message : String(e))
      })
    return () => {
      stale = true
    }
  }, [path])

  if (error) {
    return (
      <Typography variant="caption" color="error.main">
        {error}
      </Typography>
    )
  }
  if (content === null) {
    return (
      <Typography variant="caption" color="text.secondary">
        Loading…
      </Typography>
    )
  }
  // minWidth: 0 at every level is what lets the code area shrink below its
  // content's natural width inside a flex column — without it a long line
  // forces the whole dock wider instead of scrolling/wrapping internally.
  return (
    <Box sx={{ minWidth: 0, '& > *': { minWidth: 0 } }}>
      <CodeBlock code={content} lang={langFromPath(path)} wrap={wrap} />
    </Box>
  )
}
