import { useEffect, useMemo, useRef, useState } from 'react'
import { Box, IconButton, Tooltip, ButtonBase, useTheme, alpha } from '../../../ui'
import { CopyLinear, CopySuccessLinear } from '../../../icons'
import { codeToHtml } from 'shiki'

/** Shiki theme per app mode.
 *
 * This used to be hardcoded to the dark theme with a hardcoded `#0d1117`
 * surface, so every code block in the app stayed GitHub-dark even in light
 * mode: a dark slab sitting in a light message list. Code is the single most
 * common non-prose element in this app, so getting it to follow the theme
 * matters more here than almost anywhere else. */
const THEMES = { dark: 'github-dark-default', light: 'github-light' } as const

/** Collapse anything longer than this, with an expand control. A 400-line
 * tool result should not push the rest of the conversation off screen. */
const COLLAPSE_LINES = 18
/** Dense blocks (tool output) are secondary content: they collapse sooner. */
const DENSE_COLLAPSE_LINES = 12

// Shiki resolves a grammar lazily per language via its own dynamic imports —
// an unknown/unsupported lang falls back to plain text rather than throwing.
async function highlight(code: string, lang: string, theme: string): Promise<string> {
  try {
    return await codeToHtml(code, { lang, theme })
  } catch {
    return await codeToHtml(code, { lang: 'text', theme })
  }
}

/**
 * @param bare Strip the card chrome — no header bar, border, radius, or
 *   background of its own. For places where the code IS the surface rather
 *   than a block embedded in prose (the file tree's inline preview), where a
 *   bordered card floating inside another panel reads as a nested box rather
 *   than as the file's contents.
 * @param filename Shown instead of the bare language tag when known — a path
 *   tells you far more than "TS" does.
 * @param collapsible Long blocks start collapsed with an expand control.
 *   On by default; pass false where the block is the whole point of the view.
 */
export function CodeBlock({
  code,
  lang,
  wrap = false,
  bare = false,
  filename,
  collapsible = true,
  showLineNumbers = false,
  dense = false,
}: {
  code: string
  lang: string
  wrap?: boolean
  bare?: boolean
  filename?: string
  collapsible?: boolean
  showLineNumbers?: boolean
  /** Smaller, quieter text for secondary content like tool output. */
  dense?: boolean
}) {
  const theme = useTheme()
  const [html, setHtml] = useState<string | null>(null)
  const [copied, setCopied] = useState(false)
  const [expanded, setExpanded] = useState(false)
  const copyTimer = useRef<ReturnType<typeof setTimeout> | null>(null)

  const lineCount = useMemo(() => code.split('\n').length, [code])
  const collapseAt = dense ? DENSE_COLLAPSE_LINES : COLLAPSE_LINES
  const isLong = collapsible && lineCount > collapseAt
  const shikiTheme = THEMES[theme.palette.mode === 'light' ? 'light' : 'dark']

  // Plain text has nothing to highlight, and shiki would only repaint it in
  // its theme's foreground instead of the surrounding text colour.
  const plain = lang === 'text' || lang === 'plaintext' || lang === 'txt'

  useEffect(() => {
    if (plain) return
    let cancelled = false
    // Debounced: while a code fence is still streaming in token-by-token,
    // `code` changes on every chunk — without this, shiki would re-tokenize
    // the whole (growing) block on every token instead of once it settles.
    const timer = setTimeout(() => {
      void highlight(code, lang, shikiTheme).then((h) => {
        if (!cancelled) setHtml(h)
      })
    }, 200)
    return () => {
      cancelled = true
      clearTimeout(timer)
    }
  }, [code, lang, shikiTheme, plain])

  // Without this the "Copied" state can outlive the component and warn.
  useEffect(() => () => { if (copyTimer.current) clearTimeout(copyTimer.current) }, [])

  const copy = () => {
    void navigator.clipboard.writeText(code)
    setCopied(true)
    if (copyTimer.current) clearTimeout(copyTimer.current)
    copyTimer.current = setTimeout(() => setCopied(false), 1200)
  }

  // A faint tint of whatever the block sits on, from the theme — not a
  // colour of its own. The GitHub-dark slab it used to be (#0d1117) was a
  // blue-black block that belonged to no surface in the app.
  const surface = alpha(theme.palette.text.primary, theme.palette.mode === 'light' ? 0.03 : 0.035)
  const fallbackText = dense ? theme.palette.text.secondary : theme.palette.text.primary

  const copyButton = (
    <Tooltip title={copied ? 'Copied' : 'Copy'}>
      <IconButton size="small" onClick={copy} aria-label="Copy code">
        {copied ? <CopySuccessLinear size={14} color="currentColor" /> : <CopyLinear size={14} color="currentColor" />}
      </IconButton>
    </Tooltip>
  )

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
          : {
              // Matches the app's surface radius rather than inventing a
              // third corner size (theme.shape is 10, surfaces are 12).
              borderRadius: '12px',
              overflow: 'hidden',
              border: 1,
              borderColor: 'divider',
              bgcolor: surface,
            }),
        '& pre': {
          m: 0,
          p: bare ? 0 : 1.5,
          // shiki writes its theme background as an inline style on the
          // <pre>, so only !important can hand the surface back to whatever
          // the block is sitting on.
          background: 'transparent !important',
          fontSize: dense ? '0.75rem' : '0.8125rem',
          lineHeight: dense ? 1.55 : 1.6,
          ...(wrap
            ? { whiteSpace: 'pre-wrap', wordBreak: 'break-word', overflowX: 'hidden' }
            : { overflowX: 'auto' }),
        },
        '& code': { fontFamily: '"Geist Mono", ui-monospace, monospace' },
        // Line numbers are drawn from the counter rather than injected into
        // the text, so copying still yields clean code.
        ...(showLineNumbers && {
          '& pre code': { counterReset: 'line' },
          '& pre code > .line': {
            counterIncrement: 'line',
            '&::before': {
              content: 'counter(line)',
              display: 'inline-block',
              width: '2.25em',
              mr: 1.5,
              textAlign: 'right',
              color: 'text.disabled',
              userSelect: 'none',
            },
          },
        }),
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
            aria-label="Copy code"
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
            gap: 1,
            px: 1.5,
            py: 0.5,
            minHeight: 32,
            borderBottom: 1,
            borderColor: 'divider',
          }}
        >
          <Box
            component="span"
            sx={{
              fontSize: '0.6875rem',
              color: 'text.secondary',
              fontFamily: filename ? '"Geist Mono", ui-monospace, monospace' : undefined,
              textTransform: filename ? 'none' : 'uppercase',
              letterSpacing: filename ? 0 : 0.4,
              minWidth: 0,
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
            }}
            title={filename}
          >
            {filename || lang}
          </Box>
          <Box sx={{ display: 'flex', alignItems: 'center', gap: 0.5, flexShrink: 0 }}>
            <Box component="span" sx={{ fontSize: '0.6875rem', color: 'text.disabled' }}>
              {lineCount} {lineCount === 1 ? 'line' : 'lines'}
            </Box>
            {copyButton}
          </Box>
        </Box>
      )}

      <Box
        sx={{
          position: 'relative',
          // The fade is a mask on the content, so it works on any surface —
          // a gradient to a fixed colour only looked right on one.
          ...(isLong && !expanded
            ? {
                maxHeight: `${collapseAt * (dense ? 1.55 : 1.6)}em`,
                overflow: 'hidden',
                maskImage: 'linear-gradient(to bottom, black calc(100% - 48px), transparent)',
              }
            : {}),
        }}
      >
        {html && !plain ? (
          <Box dangerouslySetInnerHTML={{ __html: html }} />
        ) : (
          // Same padding, font and line-height as the highlighted output, so
          // the block does not resize when shiki resolves — the plain text is
          // a first paint, not a placeholder of a different shape.
          <Box component="pre" sx={{ color: fallbackText }}>
            <code>{code}</code>
          </Box>
        )}

      </Box>

      {isLong && (
        <ButtonBase
          onClick={() => setExpanded((v) => !v)}
          sx={{
            width: '100%',
            py: 0.5,
            fontSize: '0.6875rem',
            color: 'text.secondary',
            borderTop: bare ? 0 : 1,
            borderColor: 'divider',
            '&:hover': { bgcolor: 'action.hover', color: 'text.primary' },
          }}
        >
          {expanded ? 'Show less' : `Show all ${lineCount} lines`}
        </ButtonBase>
      )}
    </Box>
  )
}
