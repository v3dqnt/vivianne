import { useEffect, useState } from 'react'
import Box from '@mui/material/Box'
import IconButton from '@mui/material/IconButton'
import Tooltip from '@mui/material/Tooltip'
import { CopyLinear, CopySuccessLinear } from '../../../icons'
import { codeToHtml } from 'shiki'

const THEME = 'github-dark-default'

// Shiki resolves a grammar lazily per language via its own dynamic imports —
// an unknown/unsupported lang falls back to plain text rather than throwing.
async function highlight(code: string, lang: string): Promise<string> {
  try {
    return await codeToHtml(code, { lang, theme: THEME })
  } catch {
    return await codeToHtml(code, { lang: 'text', theme: THEME })
  }
}

/**
 * @param bare Strip the card chrome — no header bar, border, radius, or
 *   background of its own. For places where the code IS the surface rather
 *   than a block embedded in prose (the file tree's inline preview), where a
 *   bordered card floating inside another panel reads as a nested box rather
 *   than as the file's contents.
 */
export function CodeBlock({
  code,
  lang,
  wrap = false,
  bare = false,
}: {
  code: string
  lang: string
  wrap?: boolean
  bare?: boolean
}) {
  const [html, setHtml] = useState<string | null>(null)
  const [copied, setCopied] = useState(false)

  useEffect(() => {
    let cancelled = false
    // Debounced: while a code fence is still streaming in token-by-token,
    // `code` changes on every chunk — without this, shiki would re-tokenize
    // the whole (growing) block on every token instead of once it settles.
    const timer = setTimeout(() => {
      void highlight(code, lang).then((h) => {
        if (!cancelled) setHtml(h)
      })
    }, 200)
    return () => {
      cancelled = true
      clearTimeout(timer)
    }
  }, [code, lang])

  const copy = () => {
    void navigator.clipboard.writeText(code)
    setCopied(true)
    setTimeout(() => setCopied(false), 1200)
  }

  return (
    <Box
      sx={{
        position: 'relative',
        my: 1,
        // `minWidth: 0` + `maxWidth: '100%'` are what actually let this shrink
        // inside a flex parent — without them the <pre>'s content width wins
        // and the whole containing panel grows instead of scrolling.
        minWidth: 0,
        maxWidth: '100%',
        ...(bare
          ? { my: 0, '&:hover .code-copy': { opacity: 1 } }
          : { borderRadius: 1.5, overflow: 'hidden', border: 1, borderColor: 'divider' }),
        '& pre': {
          m: 0,
          p: bare ? 0 : 1.5,
          // shiki writes its theme background as an inline style on the
          // <pre>, so only !important can hand the surface back to whatever
          // the preview is sitting on.
          ...(bare ? { background: 'transparent !important' } : {}),
          fontSize: '0.8125rem',
          lineHeight: 1.55,
          ...(wrap
            ? { whiteSpace: 'pre-wrap', wordBreak: 'break-word', overflowX: 'hidden' }
            : { overflowX: 'auto' }),
        },
        '& code': { fontFamily: '"JetBrains Mono", ui-monospace, monospace' },
      }}
    >
      {bare ? (
        // Copy stays available, but as a hover-only affordance floating over
        // the code instead of a header bar that would reinstate the card.
        <Tooltip title={copied ? 'Copied' : 'Copy'}>
          <IconButton
            size="small"
            className="code-copy"
            onClick={copy}
            sx={{ position: 'absolute', top: 2, right: 2, zIndex: 1, opacity: 0, transition: 'opacity 0.12s' }}
          >
            {copied ? <CopySuccessLinear size={13} color="currentColor" /> : <CopyLinear size={13} color="currentColor" />}
          </IconButton>
        </Tooltip>
      ) : (
        <Box
          sx={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            px: 1.5,
            py: 0.5,
            bgcolor: 'action.hover',
            borderBottom: 1,
            borderColor: 'divider',
          }}
        >
          <Box component="span" sx={{ fontSize: '0.6875rem', color: 'text.secondary', textTransform: 'uppercase', letterSpacing: 0.4 }}>
            {lang}
          </Box>
          <Tooltip title={copied ? 'Copied' : 'Copy'}>
            <IconButton size="small" onClick={copy}>
              {copied ? <CopySuccessLinear size={14} color="currentColor" /> : <CopyLinear size={14} color="currentColor" />}
            </IconButton>
          </Tooltip>
        </Box>
      )}
      {html ? (
        <Box sx={bare ? undefined : { bgcolor: '#0d1117' }} dangerouslySetInnerHTML={{ __html: html }} />
      ) : (
        <Box component="pre" sx={bare ? { color: 'text.secondary' } : { bgcolor: '#0d1117', color: '#c9d1d9' }}>
          <code>{code}</code>
        </Box>
      )}
    </Box>
  )
}
