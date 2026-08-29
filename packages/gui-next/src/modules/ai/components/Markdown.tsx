import { memo, type ReactNode } from 'react'
import Box from '@mui/material/Box'
import Link from '@mui/material/Link'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import remend from 'remend'
import { CodeBlock } from './CodeBlock'
import { MermaidBlock } from './MermaidBlock'
import { useThrottledValue } from '../../../lib/useThrottledValue'

const VIDEO_EXT = /\.(mp4|webm|mov|m4v)$/i
const AUDIO_EXT = /\.(mp3|wav|ogg|m4a|flac)$/i

/**
 * Chat text is rendered as markdown while it's still streaming in
 * token-by-token, so it routinely arrives mid-`**bold`/mid-`` `code ``/etc.
 * `remend` closes those dangling markers before `react-markdown` parses the
 * string, so a partial token never flashes as broken/literal syntax.
 */
export const Markdown = memo(function Markdown({ text }: { text: string }) {
  // `remend` + `ReactMarkdown` re-parse the whole accumulated string from
  // scratch on every call — cheap for a settled message, but this component
  // gets a new `text` prop on every streamed token (often 10-60/sec), and
  // parse cost grows with message length. Throttling the actual parse to
  // ~100ms keeps the visible text live-feeling while cutting redundant
  // full-message reparses during long replies.
  const throttledText = useThrottledValue(text, 100)
  const healed = remend(throttledText)

  return (
    <Box
      sx={{
        '& > *:first-of-type': { mt: 0 },
        '& > *:last-of-type': { mb: 0 },
        '& p': { m: 0, mb: 1, lineHeight: 1.6, fontSize: '0.875rem' },
        '& ul, & ol': { mt: 0, mb: 1, pl: 3, fontSize: '0.875rem' },
        '& li': { mb: 0.25 },
        '& li > p': { mb: 0 },
        '& h1, & h2, & h3, & h4': { mt: 1.5, mb: 0.75, fontWeight: 600, lineHeight: 1.35 },
        '& h1': { fontSize: '1.25rem' },
        '& h2': { fontSize: '1.125rem' },
        '& h3': { fontSize: '1rem' },
        '& h4': { fontSize: '0.9375rem' },
        '& blockquote': {
          m: 0,
          mb: 1,
          pl: 1.5,
          borderLeft: 3,
          borderColor: 'divider',
          color: 'text.secondary',
        },
        '& hr': { border: 0, borderTop: 1, borderColor: 'divider', my: 1.5 },
        '& table': { borderCollapse: 'collapse', mb: 1, fontSize: '0.8125rem', display: 'block', overflowX: 'auto' },
        '& th, & td': { border: 1, borderColor: 'divider', px: 1, py: 0.5, textAlign: 'left' },
        '& th': { bgcolor: 'action.hover', fontWeight: 600 },
        '& code:not(pre code)': {
          fontFamily: '"JetBrains Mono", ui-monospace, monospace',
          fontSize: '0.8125em',
          bgcolor: 'action.hover',
          borderRadius: 1,
          px: 0.5,
          py: 0.15,
        },
        '& a': { color: 'primary.main' },
      }}
    >
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          a: ({ href, children }) => {
            // The model has no real way to emit a native <video>/<audio> tag
            // through markdown — a plain link to a media file is the only
            // signal available, so linked media renders as a player instead
            // of a bare "click here" link.
            if (href && VIDEO_EXT.test(href)) return <Box component="video" src={href} controls sx={{ maxWidth: '100%', borderRadius: 1.5, my: 1 }} />
            if (href && AUDIO_EXT.test(href)) return <Box component="audio" src={href} controls sx={{ width: '100%', my: 1 }} />
            return (
              <Link href={href} target="_blank" rel="noopener noreferrer">
                {children}
              </Link>
            )
          },
          img: ({ src, alt }) => {
            const url = typeof src === 'string' ? src : ''
            if (VIDEO_EXT.test(url)) return <Box component="video" src={url} controls sx={{ maxWidth: '100%', borderRadius: 1.5, my: 1 }} />
            if (AUDIO_EXT.test(url)) return <Box component="audio" src={url} controls sx={{ width: '100%', my: 1 }} />
            return <Box component="img" src={url} alt={alt} sx={{ maxWidth: '100%', borderRadius: 1.5, my: 1 }} />
          },
          code(props) {
            const { children, className } = props
            const match = /language-(\w+)/.exec(className ?? '')
            // Inline code has no `language-*` className and no surrounding
            // `<pre>` — react-markdown gives both the same `code` renderer,
            // so the className is the only reliable inline/block signal.
            if (!match) return <code className={className}>{children}</code>
            const code = String(children).replace(/\n$/, '')
            if (match[1] === 'mermaid') return <MermaidBlock code={code} />
            return <CodeBlock code={code} lang={match[1]} />
          },
          // The `code` renderer above already produces CodeBlock's own
          // container for fenced blocks — an unwrapped <pre> would double up.
          pre: ({ children }: { children?: ReactNode }) => <>{children}</>,
        }}
      >
        {healed}
      </ReactMarkdown>
    </Box>
  )
})
