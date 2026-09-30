import { Box, alpha } from '../../../ui'
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
export function FileLink({ path }: { path: string }) {
  const root = useWorkspaceStore((s) => s.folders[0] ?? null)
  const full = resolvePath(path, root)
  return (
    <Box
      component="span"
      role="link"
      tabIndex={0}
      title={`${full}\nOpen in Preview`}
      onClick={(e) => {
        // Inside a card header that toggles on click: open, don't toggle.
        e.stopPropagation()
        usePanelStore.getState().openDocument(full)
      }}
      onKeyDown={(e) => {
        if (e.key === 'Enter') {
          e.stopPropagation()
          usePanelStore.getState().openDocument(full)
        }
      }}
      sx={{
        fontFamily: '"Geist Mono", ui-monospace, monospace',
        fontSize: '0.75rem',
        color: 'info.main',
        px: 0.5,
        py: '1px',
        borderRadius: '4px',
        bgcolor: (t) => alpha(t.palette.info.main, 0.1),
        cursor: 'pointer',
        whiteSpace: 'nowrap',
        overflow: 'hidden',
        textOverflow: 'ellipsis',
        minWidth: 0,
        '&:hover': { textDecoration: 'underline' },
        '&:focus-visible': { outline: 1, outlineColor: 'info.main' },
      }}
    >
      {baseName(path)}
    </Box>
  )
}
