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

export function CodeBlock({ code, lang, wrap = false }: { code: string; lang: string; wrap?: boolean }) {
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
        borderRadius: 1.5,
        overflow: 'hidden',
        border: 1,
        borderColor: 'divider',
        '& pre': {
          m: 0,
          p: 1.5,
          fontSize: '0.8125rem',
          lineHeight: 1.55,
          ...(wrap
            ? { whiteSpace: 'pre-wrap', wordBreak: 'break-word', overflowX: 'hidden' }
            : { overflowX: 'auto' }),
        },
        '& code': { fontFamily: '"JetBrains Mono", ui-monospace, monospace' },
      }}
    >
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
      {html ? (
        <Box sx={{ bgcolor: '#0d1117' }} dangerouslySetInnerHTML={{ __html: html }} />
      ) : (
        <Box component="pre" sx={{ bgcolor: '#0d1117', color: '#c9d1d9' }}>
          <code>{code}</code>
        </Box>
      )}
    </Box>
  )
}
