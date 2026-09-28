import { Box } from '../../../ui'
import type { MessageContent } from '../lib/types'

/** Renders one `image` content block. Click opens the full-res image in a new tab — no in-app lightbox, ponytail-simple. */
export function ImageBlock({ block }: { block: Extract<MessageContent, { type: 'image' }> }) {
  const src = block.base64 ? `data:${block.mediaType ?? 'image/png'};base64,${block.base64}` : block.url
  if (!src) return null

  return (
    <Box
      component="img"
      src={src}
      onClick={() => window.open(src, '_blank')}
      sx={{
        display: 'block',
        maxWidth: 320,
        maxHeight: 240,
        borderRadius: 1.5,
        border: 1,
        borderColor: 'divider',
        cursor: 'zoom-in',
        objectFit: 'contain',
      }}
    />
  )
}
