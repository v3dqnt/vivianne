import Stack from '@mui/material/Stack'
import Box from '@mui/material/Box'
import Typography from '@mui/material/Typography'
import type { ChangelogEntry } from '../store/updateStore'

/** Muted per-section accent. The changelog's own vocabulary is Added /
 * Fixed / Changed / Removed, so colour carries the section's meaning and the
 * heading text doesn't have to be read to scan a release. */
function sectionColor(title: string): string {
  const t = title.toLowerCase()
  if (t.startsWith('add')) return 'success.main'
  if (t.startsWith('fix')) return 'warning.main'
  if (t.startsWith('remov') || t.startsWith('deprecat')) return 'error.main'
  return 'primary.main'
}

/** Renders parsed changelog entries. Shared by the Settings history list and
 * the "what's new" dialog so an entry can't render two different ways
 * depending on where it's shown.
 *
 * `**bold**` is the only inline markup the changelog actually uses (to lead a
 * bullet with its headline), so it's handled directly rather than pulling the
 * chat's full Markdown renderer into a settings panel. */
function renderInline(text: string) {
  const parts = text.split(/(\*\*[^*]+\*\*)/g)
  return parts.map((part, i) =>
    part.startsWith('**') && part.endsWith('**') ? (
      <Box key={i} component="span" sx={{ fontWeight: 600, color: 'text.primary' }}>
        {part.slice(2, -2)}
      </Box>
    ) : (
      part
    ),
  )
}

export function ChangelogBody({ entries, limit }: { entries: ChangelogEntry[]; limit?: number }) {
  const shown = limit ? entries.slice(0, limit) : entries

  return (
    <Stack sx={{ gap: 2.5 }}>
      {shown.map((entry) => (
        <Stack key={entry.version} sx={{ gap: 1 }}>
          <Stack direction="row" sx={{ alignItems: 'baseline', gap: 1 }}>
            <Typography variant="body2" sx={{ fontWeight: 700 }}>
              {entry.version}
            </Typography>
            {entry.date && (
              <Typography variant="caption" color="text.secondary">
                {entry.date}
              </Typography>
            )}
          </Stack>

          {entry.sections.map((section) => (
            <Stack key={section.title} sx={{ gap: 0.5 }}>
              <Typography
                variant="caption"
                sx={{
                  fontWeight: 700,
                  textTransform: 'uppercase',
                  letterSpacing: 0.5,
                  fontSize: '0.625rem',
                  color: sectionColor(section.title),
                }}
              >
                {section.title}
              </Typography>
              <Stack component="ul" sx={{ m: 0, pl: 2, gap: 0.5 }}>
                {section.items.map((item, i) => (
                  <Typography
                    key={i}
                    component="li"
                    variant="caption"
                    color="text.secondary"
                    sx={{ lineHeight: 1.6, display: 'list-item' }}
                  >
                    {renderInline(item)}
                  </Typography>
                ))}
              </Stack>
            </Stack>
          ))}
        </Stack>
      ))}
    </Stack>
  )
}
