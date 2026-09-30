import { useRef } from 'react'
import { IconButton, Tooltip } from '../../../ui'
import { PaperclipLinear } from '../../../icons'
import type { ImageAttachment, FileAttachment } from '../lib/types'
import { buildAttachments } from '../lib/attachments'

/** One button for every file type. The actual classification lives in
 * `buildAttachments`, shared with the composer's paste and drag-drop
 * handlers so all three accept exactly the same things. */
export function AttachFileButton({
  onAddImages,
  onAddFiles,
}: {
  onAddImages: (images: ImageAttachment[]) => void
  onAddFiles: (files: FileAttachment[]) => void
}) {
  const inputRef = useRef<HTMLInputElement>(null)

  return (
    <>
      <input
        ref={inputRef}
        type="file"
        multiple
        hidden
        onChange={async (e) => {
          const files = Array.from(e.target.files ?? [])
          e.target.value = ''
          if (!files.length) return
          const { images, files: docs } = await buildAttachments(files)
          if (images.length) onAddImages(images)
          if (docs.length) onAddFiles(docs)
        }}
      />
      <Tooltip title="Attach file or image">
        <IconButton size="small" onClick={() => inputRef.current?.click()}>
          <PaperclipLinear size={17} color="currentColor" />
        </IconButton>
      </Tooltip>
    </>
  )
}
