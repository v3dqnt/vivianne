import { useEffect, useId, useState } from 'react'
import Box from '@mui/material/Box'
import Typography from '@mui/material/Typography'
import mermaid from 'mermaid'

let configured = false
function ensureConfigured() {
  if (configured) return
  mermaid.initialize({ startOnLoad: false, theme: 'dark', securityLevel: 'strict' })
  configured = true
}

export function MermaidBlock({ code }: { code: string }) {
  const id = useId().replace(/:/g, '')
  const [svg, setSvg] = useState<string | null>(null)
  const [error, setError] = useState(false)

  useEffect(() => {
    ensureConfigured()
    let cancelled = false
    // Debounced for the same reason as CodeBlock's shiki call: a diagram
    // fence still streaming in token-by-token is rarely valid mermaid syntax
    // yet, so re-parsing every chunk would just spam parse errors.
    const timer = setTimeout(() => {
      mermaid
        .parse(code, { suppressErrors: true })
        .then((ok) => {
          if (!ok || cancelled) return
          return mermaid.render(`mermaid-${id}`, code).then(({ svg: rendered }) => {
            if (!cancelled) {
              setSvg(rendered)
              setError(false)
            }
          })
        })
        .catch(() => {
          if (!cancelled) setError(true)
        })
    }, 250)
    return () => {
      cancelled = true
      clearTimeout(timer)
    }
  }, [code, id])

  if (error) {
    return (
      <Box
        component="pre"
        sx={{ my: 1, p: 1.5, borderRadius: 1.5, border: 1, borderColor: 'error.main', fontSize: '0.75rem', overflowX: 'auto' }}
      >
        {code}
      </Box>
    )
  }

  if (!svg) {
    return (
      <Typography variant="caption" color="text.secondary" sx={{ display: 'block', my: 1 }}>
        Rendering diagram…
      </Typography>
    )
  }

  return (
    <Box
      sx={{ my: 1, p: 1.5, borderRadius: 1.5, border: 1, borderColor: 'divider', bgcolor: 'background.paper', overflowX: 'auto', '& svg': { maxWidth: '100%' } }}
      dangerouslySetInnerHTML={{ __html: svg }}
    />
  )
}
