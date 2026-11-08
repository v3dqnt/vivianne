/**
 * Which file-type icon a file or folder gets, the way VS Code's Material Icon Theme picks one: an exact file name first
 * (`package.json`, `Dockerfile`), then the longest matching extension (`d.ts` before `ts`), then the generic file. Folders go by name,
 * with an open variant. Light themes use the theme's light variants where it has them.
 *
 * Pure: the manifest is passed in, so this can be tested without loading the 380 KB table.
 */

export interface IconTable {
  file: string
  ext: Record<string, string>
  names: Record<string, string>
  folders: Record<string, string>
  foldersOpen: Record<string, string>
}

export interface IconManifest extends IconTable {
  folder: string
  folderOpen: string
  light: Partial<IconTable>
}

export function fileIconName(manifest: IconManifest, name: string, opts: { dir?: boolean; open?: boolean; light?: boolean } = {}): string {
  const lower = name.toLowerCase()
  const light = opts.light ? manifest.light : undefined
  if (opts.dir) {
    if (opts.open) return light?.foldersOpen?.[lower] ?? manifest.foldersOpen[lower] ?? manifest.folderOpen
    return light?.folders?.[lower] ?? manifest.folders[lower] ?? manifest.folder
  }
  const byName = light?.names?.[lower] ?? manifest.names[lower]
  if (byName) return byName
  // Every dotted suffix, longest first: `a.test.tsx` tries `test.tsx`, then `tsx`.
  const parts = lower.split('.')
  for (let i = 1; i < parts.length; i++) {
    const suffix = parts.slice(i).join('.')
    const hit = light?.ext?.[suffix] ?? manifest.ext[suffix]
    if (hit) return hit
  }
  return light?.file ?? manifest.file
}
