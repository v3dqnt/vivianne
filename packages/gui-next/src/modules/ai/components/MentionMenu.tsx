import { useEffect, useRef, type ReactNode } from 'react'
import { Box, Stack, Typography, alpha, useTheme } from '../../../ui'
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

const MONO = '"Geist Mono", ui-monospace, monospace'

/** A colour per broad file type, for the extension badge. */
function extHue(ext: string): string {
  if (['ts', 'tsx', 'js', 'jsx', 'mjs', 'cjs'].includes(ext)) return '#3b82f6'
  if (['rs', 'go', 'c', 'cpp', 'h', 'swift', 'kt', 'java', 'cs'].includes(ext)) return '#f97316'
  if (['py', 'rb', 'php', 'lua', 'sh', 'ps1'].includes(ext)) return '#22c55e'
  if (['md', 'mdx', 'txt', 'rst'].includes(ext)) return '#a855f7'
  if (['json', 'yaml', 'yml', 'toml', 'ini', 'env', 'lock'].includes(ext)) return '#eab308'
  if (['css', 'scss', 'html', 'svg'].includes(ext)) return '#ec4899'
  if (['png', 'jpg', 'jpeg', 'gif', 'webp', 'pdf', 'docx', 'xlsx', 'pptx'].includes(ext)) return '#14b8a6'
  return '#8a8a8a'
}

/**
 * The floating card every `@` picker sits in: the popover surface, a hairline,
 * the pane radius and a soft shadow, with a header naming what's listed and the
 * keys that work.
 */
export function MentionShell({ title, children, width = 440 }: { title: string; children: ReactNode; width?: number }) {
  const theme = useTheme()
  const dark = theme.palette.mode === 'dark'
  return (
    <Box
      role="listbox"
      sx={{
        position: 'absolute',
        bottom: '100%',
        left: 0,
        mb: 1,
        width: '100%',
        maxWidth: width,
        borderRadius: '14px',
        border: 1,
        borderColor: 'divider',
        bgcolor: 'background.paper',
        boxShadow: dark ? '0 12px 32px rgba(0,0,0,0.45), 0 2px 6px rgba(0,0,0,0.3)' : '0 12px 32px rgba(0,0,0,0.12), 0 2px 6px rgba(0,0,0,0.06)',
        overflow: 'hidden',
        zIndex: 50,
        animation: 'viv-mention-in 120ms ease-out',
        '@keyframes viv-mention-in': { from: { opacity: 0, transform: 'translateY(4px)' }, to: { opacity: 1, transform: 'none' } },
      }}
    >
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1, px: 1.5, pt: 1, pb: 0.75 }}>
        <Typography sx={{ fontSize: '0.75rem', fontWeight: 600, flex: 1 }}>{title}</Typography>
        <Stack direction="row" sx={{ gap: 0.5, alignItems: 'center' }}>
          {[
            ['↑↓', 'move'],
            ['↵', 'insert'],
            ['esc', 'close'],
          ].map(([k, label]) => (
            <Stack key={k} direction="row" sx={{ alignItems: 'center', gap: 0.375 }}>
              <Box
                component="kbd"
                sx={{
                  fontFamily: MONO,
                  fontSize: '0.625rem',
                  lineHeight: 1,
                  px: 0.5,
                  py: 0.25,
                  borderRadius: '4px',
                  border: 1,
                  borderColor: 'divider',
                  color: 'text.secondary',
                  bgcolor: 'background.card',
                }}
              >
                {k}
              </Box>
              <Typography sx={{ fontSize: '0.625rem', color: 'text.disabled' }}>{label}</Typography>
            </Stack>
          ))}
        </Stack>
      </Stack>
      {children}
    </Box>
  )
}

/** A section label with a count. */
export function MentionSection({ label, count }: { label: string; count: number }) {
  return (
    <Stack direction="row" sx={{ alignItems: 'baseline', gap: 0.75, px: 1.5, pt: 0.75, pb: 0.25 }}>
      <Typography sx={{ fontSize: '0.625rem', fontWeight: 600, textTransform: 'uppercase', letterSpacing: 0.6, color: 'text.secondary' }}>{label}</Typography>
      <Typography sx={{ fontSize: '0.625rem', color: 'text.disabled', fontFamily: MONO }}>{count}</Typography>
    </Stack>
  )
}

/** One row: a tinted icon tile, a title and a quieter line, and a tag on the
 * right. The active row gets the selection fill and an accent edge. */
export function MentionRow({
  active,
  icon,
  tint,
  title,
  subtitle,
  tag,
  onSelect,
  onHover,
}: {
  active: boolean
  icon: ReactNode
  tint: string
  title: ReactNode
  subtitle?: ReactNode
  tag?: ReactNode
  onSelect: () => void
  onHover: () => void
}) {
  const theme = useTheme()
  const ref = useRef<HTMLButtonElement | null>(null)
  // Keep the keyboard's choice in view while arrowing through a long list.
  useEffect(() => {
    if (active) ref.current?.scrollIntoView({ block: 'nearest' })
  }, [active])
  return (
    <Stack
      ref={ref}
      component="button"
      role="option"
      aria-selected={active}
      direction="row"
      onMouseDown={(e: React.MouseEvent) => e.preventDefault()}
      onClick={onSelect}
      onMouseEnter={onHover}
      sx={{
        position: 'relative',
        width: '100%',
        alignItems: 'center',
        gap: 1.25,
        pl: 1.25,
        pr: 1,
        py: 0.625,
        border: 'none',
        borderRadius: '9px',
        textAlign: 'left',
        cursor: 'pointer',
        color: 'text.primary',
        bgcolor: active ? alpha(theme.palette.text.primary, 0.07) : 'transparent',
        transition: 'background-color 80ms',
        '&::before': {
          content: '""',
          position: 'absolute',
          left: 3,
          top: 8,
          bottom: 8,
          width: 2,
          borderRadius: 2,
          bgcolor: active ? 'primary.main' : 'transparent',
        },
      }}
    >
      <Box
        sx={{
          width: 26,
          height: 26,
          borderRadius: '8px',
          flexShrink: 0,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          color: tint,
          bgcolor: alpha(tint, 0.12),
          border: 1,
          borderColor: alpha(tint, 0.22),
        }}
      >
        {icon}
      </Box>
      <Box sx={{ minWidth: 0, flex: 1 }}>
        <Typography noWrap sx={{ fontSize: '0.8125rem', fontWeight: 500, lineHeight: 1.35 }}>
          {title}
        </Typography>
        {subtitle && (
          <Typography noWrap sx={{ fontSize: '0.6875rem', color: 'text.secondary', lineHeight: 1.35 }}>
            {subtitle}
          </Typography>
        )}
      </Box>
      {tag && <Box sx={{ flexShrink: 0 }}>{tag}</Box>}
    </Stack>
  )
}

function Tag({ children, color }: { children: ReactNode; color?: string }) {
  return (
    <Typography
      component="span"
      sx={{
        fontSize: '0.625rem',
        fontFamily: MONO,
        px: 0.625,
        py: 0.125,
        borderRadius: '5px',
        color: color ?? 'text.secondary',
        bgcolor: color ? alpha(color, 0.12) : 'action.hover',
      }}
    >
      {children}
    </Typography>
  )
}

interface UnifiedMentionMenuProps {
  items: MentionItem[]
  activeIndex: number
  onSelect: (item: MentionItem) => void
  onHover: (index: number) => void
}

/** Unified `@` picker — skills first, then workspace files. Rendered inside
 * the composer's own relative wrapper, anchored just above it. */
export function UnifiedMentionMenu({ items, activeIndex, onSelect, onHover }: UnifiedMentionMenuProps) {
  const theme = useTheme()
  if (items.length === 0) return null

  const skills = items.filter((it): it is Extract<MentionItem, { kind: 'skill' }> => it.kind === 'skill')
  const files = items.filter((it): it is Extract<MentionItem, { kind: 'file' }> => it.kind === 'file')
  const primary = theme.palette.primary.main
  let cursor = 0

  return (
    <MentionShell title="Mention a skill or file">
      <Box sx={{ maxHeight: 340, overflowY: 'auto', px: 0.5, pb: 0.5 }}>
        {skills.length > 0 && (
          <>
            <MentionSection label="Skills" count={skills.length} />
            {skills.map((it) => {
              const idx = cursor++
              return (
                <MentionRow
                  key={`skill-${it.skill.name}`}
                  active={idx === activeIndex}
                  onSelect={() => onSelect(it)}
                  onHover={() => onHover(idx)}
                  tint={primary}
                  icon={<MagicpenLinear size={13} color="currentColor" />}
                  title={<Box component="span" sx={{ fontFamily: MONO }}>@{it.skill.name}</Box>}
                  subtitle={it.skill.description}
                  tag={<Tag>Skill</Tag>}
                />
              )
            })}
          </>
        )}
        {files.length > 0 && (
          <>
            <MentionSection label="Files" count={files.length} />
            {files.map((it) => {
              const idx = cursor++
              const ext = it.file.isDir ? '' : (it.file.name.split('.').pop() ?? '').toLowerCase()
              const hue = it.file.isDir ? theme.palette.text.secondary : extHue(ext)
              const dir = it.file.relPath.slice(0, Math.max(0, it.file.relPath.length - it.file.name.length)).replace(/\/$/, '')
              return (
                <MentionRow
                  key={`file-${it.file.relPath}`}
                  active={idx === activeIndex}
                  onSelect={() => onSelect(it)}
                  onHover={() => onHover(idx)}
                  tint={hue}
                  icon={it.file.isDir ? <FolderLinear size={13} color="currentColor" /> : <DocumentLinear size={13} color="currentColor" />}
                  title={
                    <>
                      {it.file.name}
                      {it.file.isDir ? '/' : ''}
                    </>
                  }
                  subtitle={dir ? <Box component="span" sx={{ fontFamily: MONO }}>{dir}</Box> : undefined}
                  tag={it.file.isDir ? <Tag>Folder</Tag> : ext && ext !== it.file.name.toLowerCase() ? <Tag color={hue}>.{ext}</Tag> : undefined}
                />
              )
            })}
          </>
        )}
      </Box>
    </MentionShell>
  )
}
