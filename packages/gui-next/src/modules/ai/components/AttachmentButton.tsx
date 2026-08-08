import { useRef } from 'react'
import IconButton from '@mui/material/IconButton'
import Tooltip from '@mui/material/Tooltip'
import { GalleryAddLinear } from 'mx-icons'
import type { ImageAttachment } from '../lib/types'

function fileToAttachment(file: File): Promise<ImageAttachment> {
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

export function AttachmentButton({ onAdd }: { onAdd: (images: ImageAttachment[]) => void }) {
  const inputRef = useRef<HTMLInputElement>(null)

  return (
    <>
      <input
        ref={inputRef}
        type="file"
        accept="image/*"
        multiple
        hidden
        onChange={async (e) => {
          const files = Array.from(e.target.files ?? [])
          e.target.value = ''
          if (!files.length) return
          const images = await Promise.all(files.map(fileToAttachment))
          onAdd(images)
        }}
      />
      <Tooltip title="Attach image">
        <IconButton size="small" onClick={() => inputRef.current?.click()}>
          <GalleryAddLinear size={17} color="currentColor" />
        </IconButton>
      </Tooltip>
    </>
  )
}
