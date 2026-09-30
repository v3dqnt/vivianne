import { call } from '../../../ipc'

/** Mirrors `coding_agent::document_edit::DocPart`. */
export interface DocPart {
  id: string
  kind: string
  text: string
  label: string
  editable: boolean
}

export type DocKind = 'pdf' | 'word' | 'sheet' | 'slides'

const KIND_BY_EXT: Record<string, DocKind> = {
  pdf: 'pdf',
  docx: 'word',
  xlsx: 'sheet',
  xlsm: 'sheet',
  xls: 'sheet',
  ods: 'sheet',
  csv: 'sheet',
  pptx: 'slides',
}

/** Formats the backend can change in place (`agent::office::EDITABLE`). */
const EDITABLE_EXT = new Set(['docx', 'xlsx', 'pptx'])

export function extOf(path: string): string {
  const i = path.lastIndexOf('.')
  return i === -1 ? '' : path.slice(i + 1).toLowerCase()
}

export function kindOf(path: string): DocKind | null {
  return KIND_BY_EXT[extOf(path)] ?? null
}

export function isDocument(path: string): boolean {
  return kindOf(path) !== null
}

export function canEdit(path: string): boolean {
  return EDITABLE_EXT.has(extOf(path))
}

export function fileName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() || path
}

export async function readBytes(path: string): Promise<{ base64: string; bytes: Uint8Array }> {
  const base64 = await call<string>('cmd_read_document_bytes', { path })
  if (base64 == null) throw new Error('The desktop backend is not connected.')
  const bin = atob(base64)
  const bytes = new Uint8Array(bin.length)
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i)
  return { base64, bytes }
}

export async function inspect(path: string): Promise<DocPart[]> {
  return (await call<DocPart[]>('cmd_inspect_document', { path })) ?? []
}

export async function editPart(path: string, part: string, text: string): Promise<void> {
  await call('cmd_edit_document_part', { path, part, text })
}

export async function restore(path: string, base64: string): Promise<void> {
  await call('cmd_restore_document_bytes', { path, base64 })
}

export const MIME: Record<string, string> = {
  pdf: 'application/pdf',
  docx: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
  pptx: 'application/vnd.openxmlformats-officedocument.presentationml.presentation',
}
