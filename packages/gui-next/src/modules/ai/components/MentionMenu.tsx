import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import { MagicpenLinear, DocumentLinear, FolderLinear } from '../../../icons'

export interface MentionableSkill {
  name: string
  description: string
}

export interface MentionableFile {
  /** Workspace-relative display path, forward-slash separated. With more
   * than one workspace folder open this is prefixed by the folder's name so
   * two files with the same relative path stay tellable apart. */
  relPath: string
  /** File basename for the primary label. */
  name: string
  /** True for directories — inserting one leaves the mention open for deeper
   * navigation instead of closing. */
  isDir: boolean
  /** What actually gets inserted into the composer. Same as `relPath` for a
   * single-folder workspace (the agent resolves relative paths against the
   * primary folder); the absolute path once several folders are open, since
   * a relative path is ambiguous — or outright wrong — outside the primary. */
  insertPath: string
}

export type MentionItem =
  | { kind: 'skill'; skill: MentionableSkill }
  | { kind: 'file'; file: MentionableFile }

interface UnifiedMentionMenuProps {
  items: MentionItem[]
  activeIndex: number
  onSelect: (item: MentionItem) => void
  onHover: (index: number) => void
}

/** Unified `@` picker — skills first, then workspace files. Rendered inside
 * the composer's own relative wrapper, anchored just above it. */
export function UnifiedMentionMenu({ items, activeIndex, onSelect, onHover }: UnifiedMentionMenuProps) {
  if (items.length === 0) return null

  const skills = items.filter((it): it is Extract<MentionItem, { kind: 'skill' }> => it.kind === 'skill')
  const files = items.filter((it): it is Extract<MentionItem, { kind: 'file' }> => it.kind === 'file')

  let cursor = 0

  return (
    <Box sx={{ position: 'absolute', bottom: '100%', left: 0, mb: 1.5, width: '100%', maxWidth: 420, borderRadius: 2, border: 1, borderColor: 'divider', bgcolor: 'background.paper', boxShadow: 4, overflow: 'hidden', zIndex: 50 }}>
      {skills.length > 0 && (
        <>
          <Typography variant="caption" sx={{ display: 'block', px: 1.5, py: 0.75, fontSize: '0.5625rem', fontWeight: 600, textTransform: 'uppercase', letterSpacing: 0.4, color: 'text.secondary', borderBottom: 1, borderColor: 'divider' }}>
            Skills
          </Typography>
          <Box sx={{ p: 0.5 }}>
            {skills.map((it) => {
              const idx = cursor++
              const active = idx === activeIndex
              return (
                <Stack
                  key={`skill-${it.skill.name}`}
                  component="button"
                  direction="row"
                  onClick={() => onSelect(it)}
                  onMouseEnter={() => onHover(idx)}
                  sx={{
                    width: '100%',
                    alignItems: 'center',
                    gap: 1.5,
                    px: 1,
                    py: 0.75,
                    borderRadius: 1,
                    border: 'none',
                    textAlign: 'left',
                    cursor: 'pointer',
                    bgcolor: active ? 'action.selected' : 'transparent',
                    '&:hover': { bgcolor: active ? 'action.selected' : 'action.hover' },
                  }}
                >
                  <Box sx={{ width: 24, height: 24, borderRadius: 1, bgcolor: 'action.hover', border: 1, borderColor: 'divider', display: 'flex', alignItems: 'center', justifyContent: 'center', flexShrink: 0, color: 'text.primary' }}>
                    <MagicpenLinear size={13} color="currentColor" />
                  </Box>
                  <Box sx={{ minWidth: 0, flex: 1 }}>
                    <Typography variant="body2" sx={{ fontSize: '0.8125rem', fontWeight: 500, fontFamily: '"JetBrains Mono", ui-monospace, monospace' }}>
                      @{it.skill.name}
                    </Typography>
                    <Typography variant="caption" color="text.secondary" noWrap sx={{ display: 'block' }}>
                      {it.skill.description}
                    </Typography>
                  </Box>
                </Stack>
              )
            })}
          </Box>
        </>
      )}
      {files.length > 0 && (
        <>
          <Typography variant="caption" sx={{ display: 'block', px: 1.5, py: 0.75, fontSize: '0.5625rem', fontWeight: 600, textTransform: 'uppercase', letterSpacing: 0.4, color: 'text.secondary', borderBottom: 1, borderColor: 'divider', borderTop: skills.length > 0 ? 1 : 0 }}>
            Files
          </Typography>
          <Box sx={{ maxHeight: 280, overflowY: 'auto', p: 0.5 }}>
            {files.map((it) => {
              const idx = cursor++
              const active = idx === activeIndex
              return (
                <Stack
                  key={`file-${it.file.relPath}`}
                  component="button"
                  direction="row"
                  onClick={() => onSelect(it)}
                  onMouseEnter={() => onHover(idx)}
                  sx={{
                    width: '100%',
                    alignItems: 'center',
                    gap: 1.5,
                    px: 1,
                    py: 0.75,
                    borderRadius: 1,
                    border: 'none',
                    textAlign: 'left',
                    cursor: 'pointer',
                    bgcolor: active ? 'action.selected' : 'transparent',
                    '&:hover': { bgcolor: active ? 'action.selected' : 'action.hover' },
                  }}
                >
                  <Box sx={{ width: 24, height: 24, borderRadius: 1, bgcolor: 'action.hover', border: 1, borderColor: 'divider', display: 'flex', alignItems: 'center', justifyContent: 'center', flexShrink: 0, color: 'text.primary' }}>
                    {it.file.isDir ? <FolderLinear size={13} color="currentColor" /> : <DocumentLinear size={13} color="currentColor" />}
                  </Box>
                  <Box sx={{ minWidth: 0, flex: 1 }}>
                    <Typography variant="body2" sx={{ fontSize: '0.8125rem', fontWeight: 500, fontFamily: '"JetBrains Mono", ui-monospace, monospace' }} noWrap>
                      {it.file.name}
                      {it.file.isDir ? '/' : ''}
                    </Typography>
                    <Typography variant="caption" color="text.secondary" noWrap sx={{ display: 'block', fontFamily: '"JetBrains Mono", ui-monospace, monospace' }}>
                      {it.file.relPath}
                    </Typography>
                  </Box>
                </Stack>
              )
            })}
          </Box>
        </>
      )}
    </Box>
  )
}
