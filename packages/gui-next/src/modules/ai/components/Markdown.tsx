import { memo, type ReactNode } from 'react'
import { ChatImage } from './ImageBlock'
import { InteractiveBlock } from './InteractiveBlock'
import { WidgetBlock } from './WidgetBlock'
import { OpenUIBlock } from './OpenUIBlock'
import { Box, Link, alpha } from '../../../ui'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import remarkMath from 'remark-math'
import 'katex/dist/katex.min.css'
import { MathSpan } from './sci/MathSpan'
import remend from 'remend'
import { CodeBlock } from './CodeBlock'
import { MermaidBlock } from './MermaidBlock'
import { ChartBlock } from './ChartBlock'
import { Model3DBlock } from './Model3DBlock'
import { ChemBlock } from './sci/ChemBlock'
import { BioBlock } from './sci/BioBlock'
import { PlotBlock } from './sci/PlotBlock'
import { PhysicsBlock } from './sci/PhysicsBlock'
import { Mol3DBlock } from './sci/Mol3DBlock'
import { useThrottledValue } from '../../../lib/useThrottledValue'
import { normalizeMath } from '../lib/mathText'
import { useModsStore, chatRenderer } from '../../mods/store/modsStore'
import { ModChatBlock } from '../../mods/components/ModChatBlock'
import { remarkFiles } from '../lib/remarkFiles'
import { FileLink } from './FileLink'

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
  // Re-render when mods load or change: a fence may have gained a renderer.
  const mods = useModsStore((s) => s.mods)
  const healed = normalizeMath(remend(throttledText))

  return (
    <Box
      className="select-text"
      sx={{
        // The chat's type: a 14px body on a 24px line (14 × φ ≈ 22.6, held
        // to the 4px grid), headings a step of √φ apart (14 → 18 → 22 → 28),
        // and every gap a multiple of 4.
        fontSize: '14px',
        lineHeight: '24px',
        '& > *:first-of-type': { mt: 0 },
        '& > *:last-of-type': { mb: 0 },
        '& p': { m: 0, mb: '12px', fontSize: '14px', lineHeight: '24px' },
        '& ul, & ol': { mt: 0, mb: '12px', pl: '20px', fontSize: '14px', lineHeight: '24px' },
        '& li': { mb: '4px', pl: '4px' },
        '& li::marker': { color: 'text.secondary' },
        '& li > p': { mb: 0 },
        '& li > ul, & li > ol': { mt: '4px', mb: 0 },
        '& h1, & h2, & h3, & h4': { mt: '24px', mb: '8px', fontWeight: 600, letterSpacing: '-0.01em', color: 'text.primary' },
        '& h1': { fontSize: '28px', lineHeight: '36px' },
        '& h2': { fontSize: '22px', lineHeight: '28px' },
        '& h3': { fontSize: '18px', lineHeight: '24px' },
        '& h4': { fontSize: '14px', lineHeight: '20px', color: 'text.secondary', textTransform: 'uppercase', letterSpacing: '0.04em' },
        '& strong': { fontWeight: 600, color: 'text.primary' },
        '& em': { fontStyle: 'italic' },
        '& blockquote': {
          m: 0,
          mb: '12px',
          py: '4px',
          pl: '12px',
          borderLeft: '2px solid',
          borderColor: 'primary.main',
          color: 'text.secondary',
          '& p': { mb: '4px' },
        },
        '& hr': { border: 0, borderTop: 1, borderColor: 'divider', my: '24px' },
        '& table': { borderCollapse: 'collapse', mb: '12px', fontSize: '13px', lineHeight: '20px', display: 'block', overflowX: 'auto' },
        '& th, & td': { border: 1, borderColor: 'divider', px: '8px', py: '4px', textAlign: 'left' },
        '& th': { bgcolor: 'action.hover', fontWeight: 600 },
        // Inline code reads as code at a glance: the accent colour on a
        // tint of it, not prose grey.
        '& code:not(pre code)': {
          fontFamily: '"Geist Mono", ui-monospace, monospace',
          fontSize: '0.857em',
          color: 'primary.main',
          bgcolor: (t: { palette: { primary: { main: string } } }) => alpha(t.palette.primary.main, 0.12),
          border: '1px solid',
          borderColor: (t: { palette: { primary: { main: string } } }) => alpha(t.palette.primary.main, 0.2),
          borderRadius: '4px',
          px: '4px',
          py: '1px',
        },
        '& a': { color: 'primary.main', textDecoration: 'none', '&:hover': { textDecoration: 'underline' } },
        // Display maths scrolls sideways instead of overflowing the bubble;
        // a long derivation is still readable, and the page never widens.
        '& .katex-display': { overflowX: 'auto', overflowY: 'hidden', py: '4px', my: '8px' },
        '& .katex': { fontSize: '1.05em' },
      }}
    >
      <ReactMarkdown
        remarkPlugins={[remarkGfm, remarkMath, remarkFiles]}
        components={{
          // File names and paths (see `remarkFiles`) open in the Preview.
          span: ({ children, ...rest }) => {
            const file = (rest as Record<string, unknown>)['data-file']
            if (typeof file === 'string') return <FileLink path={file} label={String(children)} prose />
            return <span>{children}</span>
          },
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
            return url ? <ChatImage src={url} alt={alt} inline /> : null
          },
          code(props) {
            const { children, className } = props
            const match = /language-([\w-]+)/.exec(className ?? '')
            // Inline code has no `language-*` className and no surrounding
            // `<pre>` — react-markdown gives both the same `code` renderer,
            // so the className is the only reliable inline/block signal.
            if (!match) return <code className={className}>{children}</code>
            // Formulas: remark-math emits them as `language-math` code nodes.
            if (match[1] === 'math') return <MathSpan tex={String(children).replace(/\n$/, '')} display={/math-display/.test(className ?? '')} />
            const code = String(children).replace(/\n$/, '')
            // Rich fence languages render as real figures rather than as
            // source. Same mechanism mermaid already used — the model emits a
            // fenced block inside its own message, so the figure appears where
            // the explanation is, not buried in a collapsed tool card.
            if (match[1] === 'mermaid') return <MermaidBlock code={code} />
            if (match[1] === 'chart') return <ChartBlock code={code} />
            if (match[1] === 'ui') return <InteractiveBlock code={code} />
            if (match[1] === 'widget') return <WidgetBlock code={code} />
            if (match[1] === 'openui-lang' || match[1] === 'openui') return <OpenUIBlock code={code} />
            if (match[1] === 'model3d') return <Model3DBlock code={code} />
            if (match[1] === 'chem') return <ChemBlock code={code} />
            if (match[1] === 'bio') return <BioBlock code={code} />
            if (match[1] === 'plot') return <PlotBlock code={code} />
            if (match[1] === 'physics') return <PhysicsBlock code={code} />
            if (match[1] === 'mol3d') return <Mol3DBlock code={code} />
            // A fence a running mod renders (`contributes.chat`).
            if (chatRenderer(mods, match[1])) return <ModChatBlock fence={match[1]} code={code} />
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
