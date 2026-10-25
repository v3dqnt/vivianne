import { useState } from 'react'
import { Box, Link, Stack, Typography } from '../../../ui'
import { ChevronRightLinear, GlobalLinear } from '../../../icons'
import type { Source } from '../lib/sources'

function host(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, '')
  } catch {
    return url
  }
}

/** What a reply's web searches drew on, under it, folded away until asked
 * for: "Used 4 sources", then each as a link. */
export function Sources({ sources }: { sources: Source[] }) {
  const [open, setOpen] = useState(false)
  if (sources.length === 0) return null
  return (
    <Stack sx={{ gap: 0.5, px: 0.25, mt: 0.5, alignItems: 'flex-start' }}>
      <Box
        role="button"
        tabIndex={0}
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        onKeyDown={(e) => (e.key === 'Enter' || e.key === ' ') && (e.preventDefault(), setOpen((o) => !o))}
        sx={{ display: 'inline-flex', alignItems: 'center', gap: 0.5, cursor: 'pointer', color: 'text.secondary', '&:hover': { color: 'text.primary' } }}
      >
        <ChevronRightLinear
          size={13}
          color="currentColor"
          style={{ transition: 'transform 120ms', transform: open ? 'rotate(90deg)' : 'none' }}
        />
        <Typography variant="caption" sx={{ fontWeight: 500 }}>
          Used {sources.length} source{sources.length === 1 ? '' : 's'}
        </Typography>
      </Box>
      {open && (
        <Stack sx={{ gap: 0.5, pl: 2.25, minWidth: 0, maxWidth: '100%' }}>
          {sources.map((s) => (
            <Stack key={s.url} direction="row" sx={{ gap: 0.75, alignItems: 'baseline', minWidth: 0 }}>
              <GlobalLinear size={12} color="currentColor" style={{ flexShrink: 0, alignSelf: 'center', opacity: 0.6 }} />
              <Link href={s.url} target="_blank" rel="noopener noreferrer" underline="hover" title={s.url} sx={{ fontSize: '0.8125rem', minWidth: 0 }}>
                {s.title}
              </Link>
              <Typography variant="caption" color="text.secondary" noWrap sx={{ flexShrink: 0 }}>
                {host(s.url)}
              </Typography>
            </Stack>
          ))}
        </Stack>
      )}
    </Stack>
  )
}
