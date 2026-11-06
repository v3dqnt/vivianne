/**
 * Finding file names and paths in prose, so a reply can link them instead of
 * leaving a bare string. Deliberately conservative: only names ending in a
 * known file extension count, so "v1.8.2", "e.g." and "foo.bar()" stay text.
 */

const EXTENSIONS = [
  // documents
  'pdf', 'doc', 'docx', 'xls', 'xlsx', 'ppt', 'pptx', 'odt', 'ods', 'odp', 'rtf', 'txt', 'md', 'mdx', 'csv', 'tsv', 'epub',
  // code
  'ts', 'tsx', 'js', 'jsx', 'mjs', 'cjs', 'py', 'rs', 'go', 'java', 'kt', 'swift', 'c', 'h', 'cc', 'cpp', 'hpp', 'cs', 'rb', 'php', 'lua',
  'sh', 'bash', 'zsh', 'ps1', 'bat', 'cmd', 'sql', 'html', 'htm', 'css', 'scss', 'less', 'vue', 'svelte', 'dart', 'r', 'ipynb', 'tex',
  // data and config
  'json', 'jsonc', 'yaml', 'yml', 'toml', 'ini', 'cfg', 'conf', 'env', 'xml', 'lock', 'log', 'sqlite', 'db', 'parquet',
  // media
  'png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'bmp', 'ico', 'avif', 'mp4', 'webm', 'mov', 'm4v', 'mp3', 'wav', 'ogg', 'm4a', 'flac',
  // archives and binaries
  'zip', 'tar', 'gz', 'tgz', '7z', 'rar', 'exe', 'dll', 'msi', 'dmg', 'wasm', 'glb', 'gltf', 'obj', 'stl',
]
const EXT = EXTENSIONS.join('|')

/** A path or bare file name ending in a known extension. Not preceded by a
 * word character, slash, colon or dot (so URLs and `a.b.c` don't match), and
 * not followed by one. */
const PATH_RE = new RegExp(
  String.raw`(?<![\w/\\.:@#=-])((?:[A-Za-z]:[\\/]|~[\\/]|\.{1,2}[\\/])?(?:[\w.@~+-]+[\\/])*[\w@~+-][\w.@~+-]*\.(?:${EXT}))(?![\w-])(?![\\/]\w)`,
  'gi',
)

/** A first segment like `example.com` means a URL typed without its scheme. */
function looksLikeHost(path: string): boolean {
  if (/^(?:[A-Za-z]:|~|\.{1,2})[\\/]/.test(path)) return false
  const segments = path.split(/[\\/]/)
  return segments.length > 1 && /^[\w-]+(?:\.[\w-]+)+$/.test(segments[0])
}

export function isFilePath(text: string): boolean {
  const t = text.trim()
  if (!t || t.length > 260 || /[<>|*?"\n]/.test(t) || /:\/\//.test(t)) return false
  // Spaces only inside an absolute path (`C:\My Docs\a.txt`).
  if (/\s/.test(t) && !/^(?:[A-Za-z]:[\\/]|~[\\/]|[\\/])/.test(t)) return false
  if (looksLikeHost(t)) return false
  const m = /(?:^|[\\/])[^\\/]*\.([A-Za-z0-9]{1,8})$/.exec(t)
  return !!m && EXTENSIONS.includes(m[1].toLowerCase())
}

export type Piece = string | { path: string }

/** `text` cut into plain runs and file paths, in order. */
export function splitFilePaths(text: string): Piece[] {
  const out: Piece[] = []
  let last = 0
  for (const m of text.matchAll(PATH_RE)) {
    const path = m[1]
    if (looksLikeHost(path)) continue
    const at = m.index ?? 0
    if (at > last) out.push(text.slice(last, at))
    out.push({ path })
    last = at + path.length
  }
  if (last < text.length) out.push(text.slice(last))
  return out
}

export function dirName(path: string): string {
  const cut = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'))
  return cut > 0 ? path.slice(0, cut) : path
}
