import { useState } from 'react'
import type React from 'react'
import { Box, ButtonBase, Stack, Tooltip } from '../../../ui'
import { GlobalLinear } from '../../../icons'
import type { Source } from '../lib/sources'
import { onWebLinkClick } from '../lib/openLink'

function host(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, '')
  } catch {
    return url
  }
}

/** Chips shown before "+N more". */
const SHOWN = 6

/** The site's icon, or a globe when it has none (or is offline). */
function Favicon({ url }: { url: string }) {
  const [failed, setFailed] = useState(false)
  const h = host(url)
  if (failed || !/^https?:/.test(url)) return <GlobalLinear size={12} color="currentColor" style={{ flexShrink: 0, opacity: 0.6 }} />
  return (
    <Box
      component="img"
      src={`https://icons.duckduckgo.com/ip3/${h}.ico`}
      alt=""
      onError={() => setFailed(true)}
      sx={{ width: 12, height: 12, borderRadius: '2px', flexShrink: 0 }}
    />
  )
}

/** What a reply's web searches drew on, under it: one chip per source (its
 * site, the title on hover), a few at first and the rest a click away. */
export function Sources({ sources }: { sources: Source[] }) {
  const [all, setAll] = useState(false)
  if (sources.length === 0) return null
  const shown = all ? sources : sources.slice(0, SHOWN)
  const chip = {
    display: 'inline-flex',
    alignItems: 'center',
    gap: '4px',
    height: 24,
    maxWidth: 220,
    px: '8px',
    borderRadius: '6px',
    border: 1,
    borderColor: 'divider',
    fontSize: '0.75rem',
    lineHeight: '16px',
    color: 'text.secondary',
    textDecoration: 'none',
    '&:hover': { color: 'text.primary', bgcolor: 'action.hover', borderColor: 'text.disabled' },
  } as const
  return (
    <Stack direction="row" sx={{ flexWrap: 'wrap', gap: '4px', mt: '8px', alignItems: 'center' }} aria-label={`${sources.length} sources`}>
      {shown.map((s) => (
        <Tooltip key={s.url} title={`${s.title || s.url} — click to open in Preview, Ctrl-click for your browser`}>
          <ButtonBase component="a" href={s.url} target="_blank" rel="noopener noreferrer" onClick={(e: React.MouseEvent) => onWebLinkClick(e, s.url)} sx={chip}>
            <Favicon url={s.url} />
            <Box component="span" sx={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
              {host(s.url)}
            </Box>
          </ButtonBase>
        </Tooltip>
      ))}
      {sources.length > SHOWN && (
        <ButtonBase onClick={() => setAll((v) => !v)} sx={{ ...chip, borderStyle: 'dashed' }}>
          {all ? 'Fewer' : `+${sources.length - SHOWN} more`}
        </ButtonBase>
      )}
    </Stack>
  )
}
