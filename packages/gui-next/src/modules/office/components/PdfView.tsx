import { useEffect, useMemo } from 'react'
import { Box } from '../../../ui'
import { MIME } from '../lib/docs'

/** A PDF in the webview's own viewer — search, zoom and page thumbnails come
 * for free, and it is the same renderer the user's browser uses. */
export function PdfView({ bytes }: { bytes: Uint8Array }) {
  const url = useMemo(() => URL.createObjectURL(new Blob([bytes as BlobPart], { type: MIME.pdf })), [bytes])
  useEffect(() => () => URL.revokeObjectURL(url), [url])
  return <Box component="iframe" title="PDF preview" src={url} sx={{ flex: 1, width: '100%', border: 0, bgcolor: '#525659' }} />
}
