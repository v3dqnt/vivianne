import type { ImageAttachment, FileAttachment } from './types'

// A truncated attachment still gives the model useful context; a multi-MB
// blob that silently doubles the prompt size does not.
const MAX_CHARS = 200_000

/** Extensions that are text even though their MIME type is often blank or
 * wrong (browsers report `application/octet-stream` for plenty of these). */
const TEXT_EXTS = new Set([
  'ts', 'tsx', 'js', 'jsx', 'mjs', 'cjs', 'rs', 'py', 'go', 'java', 'rb', 'c', 'h', 'cpp', 'hpp', 'cs',
  'swift', 'kt', 'php', 'lua', 'sh', 'bash', 'zsh', 'ps1', 'sql', 'r', 'jl', 'ex', 'exs', 'vue', 'svelte',
  'md', 'mdx', 'txt', 'rst', 'adoc', 'log', 'csv', 'tsv',
  'json', 'jsonc', 'yaml', 'yml', 'toml', 'ini', 'cfg', 'conf', 'env', 'properties',
  'html', 'htm', 'css', 'scss', 'sass', 'less', 'xml', 'svg', 'gitignore', 'dockerfile', 'lock',
])

function extOf(name: string): string {
  const i = name.lastIndexOf('.')
  return i === -1 ? '' : name.slice(i + 1).toLowerCase()
}

function formatBytes(n: number): string {
  if (n >= 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`
  if (n >= 1024) return `${(n / 1024).toFixed(1)} KB`
  return `${n} B`
}

function imageAttachment(file: File): Promise<ImageAttachment> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => {
      const result = reader.result as string
      resolve({ base64: result.split(',')[1] ?? '', media_type: file.type || 'image/png' })
    }
    reader.onerror = () => reject(reader.error)
    reader.readAsDataURL(file)
  })
}

/** True when the decoded text is plausibly real text rather than a binary
 * reinterpreted as UTF-8.
 *
 * `File.text()` never fails — it substitutes U+FFFD for undecodable bytes —
 * so a PDF or .docx came through as pages of mojibake that looked like
 * content to the model. NUL bytes and a high replacement-character ratio are
 * what actually distinguish the two. */
function looksLikeText(s: string): boolean {
  if (s.includes('\u0000')) return false
  const sample = s.slice(0, 4096)
  if (sample.length === 0) return true
  let replacements = 0
  for (const ch of sample) if (ch === '�') replacements++
  return replacements / sample.length < 0.05
}

/** A stand-in body for a file we can't read as text. The model is told what
 * it is rather than handed garbage, and — when the file came from disk —
 * where to find it, so it can use its own `read` tool (which handles PDFs)
 * instead of relying on the attachment. */
function binaryPlaceholder(file: File): string {
  const path = (file as File & { path?: string }).path
  return [
    `[Binary file — contents not inlined]`,
    `name: ${file.name}`,
    `type: ${file.type || `.${extOf(file.name)}` || 'unknown'}`,
    `size: ${formatBytes(file.size)}`,
    path ? `path: ${path}` : null,
    path ? `Use the read tool on that path if you need its contents.` : null,
  ]
    .filter(Boolean)
    .join('\n')
}

async function fileAttachment(file: File): Promise<FileAttachment> {
  const ext = extOf(file.name)
  const isTextByExt = TEXT_EXTS.has(ext) || TEXT_EXTS.has(file.name.toLowerCase())
  const isTextByMime = file.type.startsWith('text/') || /json|xml|javascript|typescript|yaml|toml/.test(file.type)

  // Decode first, then decide: extension and MIME are hints, the bytes are
  // the evidence. An unknown extension holding real text still reads fine,
  // and a `.txt` that's actually binary doesn't produce mojibake.
  const raw = await file.text()
  if (!isTextByExt && !isTextByMime && !looksLikeText(raw)) {
    return { name: file.name, content: binaryPlaceholder(file), truncated: false }
  }
  if (!looksLikeText(raw)) {
    return { name: file.name, content: binaryPlaceholder(file), truncated: false }
  }

  const truncated = raw.length > MAX_CHARS
  return { name: file.name, content: truncated ? raw.slice(0, MAX_CHARS) : raw, truncated }
}

export interface SplitAttachments {
  images: ImageAttachment[]
  files: FileAttachment[]
}

/** Turn a set of dropped/pasted/picked files into attachments.
 *
 * One shared path so the attach button, paste, and drag-drop can't diverge
 * on which types they accept — previously only the button existed, and it
 * ran every non-image through `file.text()` regardless of whether that made
 * any sense. */
export async function buildAttachments(files: File[]): Promise<SplitAttachments> {
  const images = files.filter((f) => f.type.startsWith('image/'))
  const rest = files.filter((f) => !f.type.startsWith('image/'))
  const [imageAttachments, fileAttachments] = await Promise.all([
    Promise.all(images.map(imageAttachment)),
    Promise.all(rest.map(fileAttachment)),
  ])
  return { images: imageAttachments, files: fileAttachments }
}

/** Files carried by a paste or drop event, from either `files` or the
 * `items` list — a screenshot pasted from the clipboard arrives only as an
 * item, with no entry in `files`. */
export function filesFromDataTransfer(dt: DataTransfer | null): File[] {
  if (!dt) return []
  const out: File[] = []
  if (dt.files && dt.files.length > 0) out.push(...Array.from(dt.files))
  if (out.length === 0 && dt.items) {
    for (const item of Array.from(dt.items)) {
      if (item.kind !== 'file') continue
      const f = item.getAsFile()
      if (f) out.push(f)
    }
  }
  return out
}
