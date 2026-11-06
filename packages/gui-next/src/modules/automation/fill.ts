/** `{{file}}` becomes the path that triggered the run. */
export function fill(text: string, file: string | null | undefined): string {
  return text.replace(/\{\{\s*file\s*\}\}/gi, file ?? '(no file)')
}
