import { Box } from '../../../ui'
import { call } from '../../../ipc'
import { dirName } from '../lib/filePaths'
import { usePanelStore } from '../../dock/store/panelStore'
import { useWorkspaceStore } from '../store/workspaceStore'

const ABSOLUTE = /^(?:[a-zA-Z]:[\\/]|[\\/]{1,2})/

/** The last segment of a path, either separator. */
export function baseName(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, '')
  return trimmed.split(/[\\/]/).pop() || trimmed
}

/** A tool's path argument made absolute against the workspace it ran in. */
export function resolvePath(path: string, root: string | null): string {
  if (ABSOLUTE.test(path) || !root) return path
  const sep = root.includes('\\') ? '\\' : '/'
  return `${root.replace(/[\\/]+$/, '')}${sep}${path.replace(/^\.[\\/]/, '').replace(/[\\/]/g, sep)}`
}

/** The file a tool call is about, when it is about one: `path`/`file_path`
 * that names a file (has an extension), not a folder to search in. */
export function filePathArg(args: Record<string, unknown> | undefined): string | null {
  if (!args) return null
  const raw = typeof args.path === 'string' ? args.path : typeof args.file_path === 'string' ? args.file_path : null
  if (!raw || /[*?]/.test(raw)) return null
  return /\.[A-Za-z0-9]{1,8}$/.test(baseName(raw)) ? raw : null
}

/**
 * A file named by what a reader recognises (its name, not its path),
 * highlighted, and opening in the Preview panel on click. The full path is
 * on hover.
 */
export function FileLink({ path, label, prose = false }: { path: string; label?: string; prose?: boolean }) {
  const root = useWorkspaceStore((s) => s.folders[0] ?? null)
  const full = resolvePath(path, root)
  const open = () => usePanelStore.getState().openDocument(full)
  const reveal = () => void call('cmd_open_external', { path: dirName(full), target: 'explorer' })
  return (
    <Box
      component="span"
      role="link"
      tabIndex={0}
      title={`${full}\nClick to open in Preview. Ctrl+click or right-click to show in the folder.`}
      onClick={(e) => {
        // Inside a card header that toggles on click: open, don't toggle.
        e.stopPropagation()
        if (e.ctrlKey || e.metaKey) reveal()
        else open()
      }}
      onContextMenu={(e) => {
        e.preventDefault()
        e.stopPropagation()
        reveal()
      }}
      onKeyDown={(e) => {
        if (e.key === 'Enter') {
          e.stopPropagation()
          open()
        }
      }}
      sx={
        prose
          ? {
              // Inside a sentence: wraps with it, reads as a link, not a chip.
              color: 'primary.main',
              fontFamily: '"Geist Mono", ui-monospace, monospace',
              fontSize: '0.8125em',
              fontWeight: 500,
              cursor: 'pointer',
              wordBreak: 'break-word',
              '&:hover': { textDecoration: 'underline' },
              '&:focus-visible': { outline: 1, outlineColor: 'primary.main' },
            }
          : {
              fontWeight: 600,
              color: 'primary.main',
              borderRadius: '4px',
              '&:hover': { textDecoration: 'underline' },
              cursor: 'pointer',
              whiteSpace: 'nowrap',
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              minWidth: 0,
              '&:focus-visible': { outline: 1, outlineColor: 'primary.main' },
            }
      }
    >
      {label ?? baseName(path)}
    </Box>
  )
}
