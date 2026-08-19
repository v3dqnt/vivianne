import { useRef } from 'react'
import IconButton from '@mui/material/IconButton'
import Tooltip from '@mui/material/Tooltip'
import { PaperclipLinear } from '../../../icons'
import type { ImageAttachment, FileAttachment } from '../lib/types'

// A truncated attachment still gives the model useful context; a multi-MB
// binary that silently doubles the prompt size does not.
const MAX_CHARS = 200_000

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

async function textAttachment(file: File): Promise<FileAttachment> {
  const raw = await file.text()
  const truncated = raw.length > MAX_CHARS
  return { name: file.name, content: truncated ? raw.slice(0, MAX_CHARS) : raw, truncated }
}

/** One button for both images and general files — branches per picked file
 * on MIME type rather than making the user pick which kind of attachment
 * button to click. Reads via the browser `File` API for both, so it works
 * identically whether the picked file lives on disk or, in a plain-browser
 * preview, nowhere real. */
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
          const images = files.filter((f) => f.type.startsWith('image/'))
          const rest = files.filter((f) => !f.type.startsWith('image/'))
          const [imageAttachments, fileAttachments] = await Promise.all([
            Promise.all(images.map(imageAttachment)),
            Promise.all(rest.map(textAttachment)),
          ])
          if (imageAttachments.length) onAddImages(imageAttachments)
          if (fileAttachments.length) onAddFiles(fileAttachments)
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
