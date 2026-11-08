import { useEffect, useState } from 'react'
import { Box, useTheme } from '../ui'
import { fileIconName, type IconManifest } from '../lib/fileIcons'

// Every icon from material-icon-theme, as URLs: nothing is fetched until an <img> asks for it.
const ICONS = import.meta.glob('../../../../node_modules/material-icon-theme/icons/*.svg', { query: '?url', import: 'default', eager: true }) as Record<string, string>
const URLS: Record<string, string> = Object.fromEntries(Object.entries(ICONS).map(([path, url]) => [path.slice(path.lastIndexOf('/') + 1, -4), url]))

// The name → icon table is large, so it loads once, on first use.
let manifest: IconManifest | null = null
let loading: Promise<IconManifest> | null = null
const load = () => (loading ??= import('../lib/fileIconManifest.json').then((m) => (manifest = m.default as unknown as IconManifest)))

/** A file's or folder's type icon (Rust crab, TypeScript TS, a src folder…), as in VS Code's Material Icon Theme. */
export function FileTypeIcon({ name, dir = false, open = false, size = 16 }: { name: string; dir?: boolean; open?: boolean; size?: number }) {
  const theme = useTheme()
  const [, setReady] = useState(manifest !== null)
  useEffect(() => {
    if (!manifest) void load().then(() => setReady(true))
  }, [])
  const icon = manifest ? fileIconName(manifest, name, { dir, open, light: theme.palette.mode === 'light' }) : null
  const url = icon ? (URLS[icon] ?? URLS[dir ? 'folder' : 'file']) : null
  return <Box component={url ? 'img' : 'span'} src={url ?? undefined} alt="" aria-hidden sx={{ width: size, height: size, flexShrink: 0, display: 'block' }} />
}
