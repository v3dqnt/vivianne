import { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from 'react'
import { Alert, Box, ButtonBase, IconButton, InputBase, Stack, Tooltip, Typography, alpha, keyframes, useTheme } from '../../../ui'
import { SendLinear, StopLinear, SparkleLinear, CloseLinear, DocumentTextLinear, MicrophoneLinear } from '../../../icons'
import { call, on } from '../../../ipc'
import { AttachFileButton } from './AttachFileButton'
import { buildAttachments, filesFromDataTransfer } from '../lib/attachments'
import type { FileAttachment, ImageAttachment } from '../lib/types'
import { useInstantMic } from '../../instant/lib/useInstantMic'
import { markDictated, noteSent } from '../lib/readAloud'
import { highlights, suggest, type KnownFile } from '../lib/fileSuggest'

export interface ComposerAttachments {
  images: ImageAttachment[]
  files: FileAttachment[]
}

export interface ChatComposerHandle {
  focus: () => void
  /** Attachments waiting to be sent, e.g. for a caller that steers instead. */
  takeAttachments: () => ComposerAttachments
}

/**
 * The one message box: the main chat, a thread, a DM, the team room and an
 * agent's steer box all use it, so they look and behave the same.
 *
 * The caller owns the text (it completes mentions and restores drafts); the
 * box owns attachments, dictation, Enter-to-send and the send/stop button.
 */
export const ChatComposer = forwardRef<ChatComposerHandle, {
  value: string
  onChange: (text: string) => void
  /** Send. Throwing restores what was typed and shows the error. */
  onSubmit: (text: string, attachments: ComposerAttachments) => void | Promise<void>
  placeholder?: string
  /** Accept files and images: attach button, paste, drop. */
  attachments?: boolean
  /** Something is running: the button stops it (or steers, when typed into). */
  busy?: boolean
  stopping?: boolean
  onStop?: () => void
  /** Busy and typed into: redirect the run with this text. Without it, the
   * text is sent normally (queued by the caller). */
  onSteer?: (text: string, attachments: ComposerAttachments) => void
  /** Handles keys first (a mention menu's arrows); true = handled. */
  onKeyDownFirst?: (e: KeyboardEvent) => boolean
  /** Floats above the box (a mention menu). */
  overlay?: ReactNode
  /** Docked on the box's top edge (a run's status). */
  top?: ReactNode
  /** Controls on the row under the box, at its left. Outside the box: the
   * box itself is only for attachments and the prompt. */
  left?: ReactNode
  /** Controls on the row under the box, at its right. */
  right?: ReactNode
  /** The files a prompt can name: their names are highlighted, and a
   * half-typed or mistyped one is completed or corrected with Tab. */
  knownFiles?: KnownFile[]
  error?: string | null
  dense?: boolean
}>(function ChatComposer(
  { value, onChange, onSubmit, placeholder = 'Message', attachments = true, busy = false, stopping = false, onStop, onSteer, onKeyDownFirst, overlay, top, left, right, knownFiles, error: outerError, dense },
  ref,
) {
  const theme = useTheme()
  const inputRef = useRef<HTMLTextAreaElement | null>(null)
  const [images, setImages] = useState<ImageAttachment[]>([])
  const [files, setFiles] = useState<FileAttachment[]>([])
  const [dragging, setDragging] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [interim, setInterim] = useState('')
  // File names: where the caret is, and the suggestion the user waved off.
  const [caret, setCaret] = useState(0)
  const [waved, setWaved] = useState<string | null>(null)
  const known = useMemo(() => knownFiles ?? [], [knownFiles])
  const marks = useMemo(() => (value && known.length ? highlights(value, known) : value.includes('@') ? highlights(value, []) : []), [value, known])
  const suggestion = useMemo(() => {
    if (!known.length) return null
    const s = suggest(value, caret, known)
    return s && s.replacement !== waved ? s : null
  }, [value, caret, known, waved])
  const accept = () => {
    if (!suggestion) return
    const next = value.slice(0, suggestion.start) + suggestion.replacement + value.slice(suggestion.end)
    onChange(next)
    const pos = suggestion.start + suggestion.replacement.length
    setCaret(pos)
    requestAnimationFrame(() => inputRef.current?.setSelectionRange(pos, pos))
  }

  const take = useCallback((): ComposerAttachments => {
    const a = { images, files }
    setImages([])
    setFiles([])
    return a
  }, [images, files])

  useImperativeHandle(ref, () => ({ focus: () => inputRef.current?.focus(), takeAttachments: take }), [take])

  const ingest = useCallback(
    async (list: File[]) => {
      if (!attachments || list.length === 0) return
      const built = await buildAttachments(list)
      if (built.images.length) setImages((p) => [...p, ...built.images])
      if (built.files.length) setFiles((p) => [...p, ...built.files])
    },
    [attachments],
  )

  const typed = value.trim().length > 0
  const hasAttachments = images.length + files.length > 0

  const send = async () => {
    if (!typed && !hasAttachments) return
    const text = value
    const sent = { images, files }
    setError(null)
    noteSent()
    onChange('')
    setImages([])
    setFiles([])
    try {
      await onSubmit(text, sent)
    } catch (e) {
      onChange(text)
      setImages(sent.images)
      setFiles(sent.files)
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  const primary = () => {
    if (!busy) return void send()
    if (typed && onSteer) {
      const text = value
      onChange('')
      onSteer(text, take())
      return
    }
    if (typed || hasAttachments) return void send()
    onStop?.()
  }

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (onKeyDownFirst?.(e)) return
    if (suggestion && e.key === 'Tab' && !e.shiftKey) {
      e.preventDefault()
      accept()
      return
    }
    if (suggestion && e.key === 'Escape') {
      setWaved(suggestion.replacement)
      return
    }
    if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault()
      void send()
    }
  }

  // Dictation appends what was said to whatever is typed.
  const append = useCallback(
    (spoken: string) => {
      setInterim('')
      if (!spoken) return
      markDictated()
      const cur = inputRef.current?.value ?? value
      onChange(cur && !/\s$/.test(cur) ? `${cur} ${spoken}` : cur + spoken)
      inputRef.current?.focus()
    },
    [onChange, value],
  )

  const shownError = outerError ?? error
  const stopMode = busy && !typed && !hasAttachments
  const steerMode = busy && typed && !!onSteer

  return (
    <Box
      onPaste={(e) => {
        const pasted = filesFromDataTransfer(e.clipboardData)
        if (!attachments || pasted.length === 0) return
        // Only when there are files: plain text pastes go to the input.
        e.preventDefault()
        void ingest(pasted)
      }}
      onDragOver={(e) => {
        if (!attachments) return
        // Required for a drop target, or the webview navigates to the file.
        e.preventDefault()
        if (!dragging) setDragging(true)
      }}
      onDragLeave={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setDragging(false)
      }}
      onDrop={(e) => {
        if (!attachments) return
        e.preventDefault()
        setDragging(false)
        void ingest(filesFromDataTransfer(e.dataTransfer))
      }}
      sx={{ position: 'relative' }}
    >
      {overlay}
      {top}
      <Box
        sx={{
          position: 'relative',
          borderRadius: '18px',
          bgcolor: 'background.card',
          border: 1,
          borderColor: dragging ? theme.palette.tokens.ring : 'divider',
          transition: 'border-color 0.16s ease, box-shadow 0.16s ease',
          '&:focus-within': { borderColor: theme.palette.tokens.ring, boxShadow: `0 0 0 3px ${alpha(theme.palette.tokens.ring, 0.2)}` },
        }}
      >
        {dragging && (
          <Box
            sx={{
              position: 'absolute',
              inset: 4,
              zIndex: 2,
              borderRadius: '14px',
              border: '2px dashed',
              borderColor: 'primary.main',
              bgcolor: alpha(theme.palette.background.card, 0.85),
              display: 'grid',
              placeItems: 'center',
              pointerEvents: 'none',
              fontSize: '0.8125rem',
              fontWeight: 600,
              color: 'primary.main',
            }}
          >
            Drop to attach
          </Box>
        )}
        {hasAttachments && (
          <Stack direction="row" sx={{ gap: 0.75, flexWrap: 'wrap', px: 1.5, pt: 1.25 }}>
            {images.map((img, i) => (
              <AttachmentChip
                key={`i${i}`}
                kind="image"
                label={`Image ${i + 1}`}
                image={dataUrl(img)}
                onRemove={() => setImages((p) => p.filter((_, j) => j !== i))}
              />
            ))}
            {files.map((f, i) => (
              <AttachmentChip key={`f${i}`} kind="file" label={f.name} onRemove={() => setFiles((p) => p.filter((_, j) => j !== i))} />
            ))}
          </Stack>
        )}
        <Stack direction="row" sx={{ alignItems: 'flex-end', gap: 0.5, px: 0.75, py: 0.625 }}>
          {attachments && (
            <AttachFileButton onAddImages={(i) => setImages((p) => [...p, ...i])} onAddFiles={(f) => setFiles((p) => [...p, ...f])} />
          )}
          <Box sx={{ position: 'relative', flex: 1, minWidth: 0 }}>
          {/* The text again, behind the real field, with file names marked.
              The field's own text is transparent so only this shows. */}
          {marks.length > 0 && <HighlightedText value={value} marks={marks} scrollOf={inputRef} />}
          <InputBase
            inputRef={inputRef}
            multiline
            fullWidth
            minRows={1}
            maxRows={dense ? 6 : 10}
            value={value}
            placeholder={placeholder}
            onChange={(e) => {
              onChange(e.target.value)
              setCaret(e.target.selectionStart ?? e.target.value.length)
              setWaved(null)
            }}
            onKeyDown={onKeyDown}
            onKeyUp={(e) => setCaret((e.target as HTMLTextAreaElement).selectionStart ?? 0)}
            onClick={(e) => setCaret((e.target as HTMLTextAreaElement).selectionStart ?? 0)}
            sx={{
              position: 'relative',
              px: 0.5,
              py: 0.5,
              fontSize: '0.875rem',
              lineHeight: 1.6,
              ...(marks.length > 0 ? { '& textarea': { color: 'transparent', caretColor: theme.palette.text.primary } } : {}),
            }}
          />
          </Box>
          <DictateButton onInterim={setInterim} onText={append} onError={setError} />
        <Tooltip title={stopMode ? (stopping ? 'Stopping…' : 'Stop') : steerMode ? 'Steer: redirect the run with this' : 'Send'}>
          <span>
            <IconButton
              aria-label={stopMode ? 'Stop' : steerMode ? 'Steer' : 'Send'}
              onClick={primary}
              disabled={(!busy && !typed && !hasAttachments) || (stopMode && (stopping || !onStop))}
              sx={{
                width: 30,
                height: 30,
                borderRadius: '50%',
                flexShrink: 0,
                bgcolor: stopMode ? 'error.main' : 'text.primary',
                color: stopMode ? 'error.contrastText' : 'background.paper',
                transition: 'transform 0.1s ease, background-color 0.12s',
                '&:hover': { bgcolor: stopMode ? 'error.dark' : alpha(theme.palette.text.primary, 0.85) },
                '&:active': { transform: 'scale(0.92)' },
                '&:disabled': { bgcolor: 'action.disabledBackground', color: 'text.disabled' },
              }}
            >
              {stopMode ? <StopLinear size={14} color="currentColor" /> : steerMode ? <SparkleLinear size={15} color="currentColor" /> : <SendLinear size={15} color="currentColor" />}
            </IconButton>
          </span>
        </Tooltip>
        </Stack>
        {suggestion && (
          <Stack direction="row" sx={{ alignItems: 'center', gap: 0.75, px: 1.75, pb: 0.5 }}>
            <Typography variant="caption" color="text.secondary">
              {suggestion.kind === 'fix' ? 'Did you mean' : 'Complete to'}
            </Typography>
            <ButtonBase
              onClick={accept}
              sx={{ px: 0.75, borderRadius: '6px', fontFamily: '"Geist Mono", ui-monospace, monospace', fontSize: '0.75rem', color: 'primary.main', bgcolor: alpha(theme.palette.primary.main, 0.12) }}
            >
              {suggestion.replacement}
            </ButtonBase>
            <Typography variant="caption" color="text.disabled">
              Tab
            </Typography>
          </Stack>
        )}
        {interim && (
          <Typography sx={{ px: 1.75, pb: 0.5, fontSize: '0.875rem', color: 'text.disabled', fontStyle: 'italic' }}>{interim}…</Typography>
        )}
      </Box>
      {(left || right) && (
        <Stack direction="row" sx={{ alignItems: 'center', justifyContent: 'space-between', gap: 1, px: 0.5, pt: 0.5, minWidth: 0 }}>
          {/* Each side is one group with one gap, so the spacing between
              controls is the same whatever they are. */}
          <Stack direction="row" sx={{ alignItems: 'center', gap: 0.5, minWidth: 0, flexWrap: 'wrap' }}>
            {left}
          </Stack>
          <Stack direction="row" sx={{ alignItems: 'center', gap: 0.5, minWidth: 0, flexShrink: 0 }}>
            {right}
          </Stack>
        </Stack>
      )}
      {shownError && (
        <Alert severity="error" sx={{ mt: 0.75, py: 0.25, fontSize: '0.75rem' }}>
          {shownError}
        </Alert>
      )}
    </Box>
  )
})

/** The prompt's text drawn behind the field, with `marks` highlighted. Sized
 * and wrapped exactly like the field so the two line up, and scrolled with it. */
function HighlightedText({ value, marks, scrollOf }: { value: string; marks: { start: number; end: number }[]; scrollOf: { current: HTMLTextAreaElement | null } }) {
  const theme = useTheme()
  const ref = useRef<HTMLDivElement | null>(null)
  useEffect(() => {
    const ta = scrollOf.current
    if (!ta) return
    const sync = () => {
      if (ref.current) ref.current.scrollTop = ta.scrollTop
    }
    sync()
    ta.addEventListener('scroll', sync)
    return () => ta.removeEventListener('scroll', sync)
  }, [scrollOf, value])
  const parts: ReactNode[] = []
  let at = 0
  marks.forEach((m, i) => {
    if (m.start > at) parts.push(value.slice(at, m.start))
    parts.push(
      <Box
        component="span"
        key={i}
        sx={{ color: 'primary.main', bgcolor: alpha(theme.palette.primary.main, 0.16), borderRadius: '4px', boxShadow: `0 0 0 1px ${alpha(theme.palette.primary.main, 0.16)}` }}
      >
        {value.slice(m.start, m.end)}
      </Box>,
    )
    at = m.end
  })
  if (at < value.length) parts.push(value.slice(at))
  return (
    <Box
      ref={ref}
      aria-hidden
      sx={{
        position: 'absolute',
        inset: 0,
        px: 0.5,
        py: 0.5,
        fontSize: '0.875rem',
        lineHeight: 1.6,
        fontFamily: 'inherit',
        whiteSpace: 'pre-wrap',
        wordBreak: 'break-word',
        overflow: 'hidden',
        pointerEvents: 'none',
        color: 'text.primary',
      }}
    >
      {parts}
      {/* A trailing newline would otherwise collapse. */}
      {value.endsWith('\n') ? '\u200b' : null}
    </Box>
  )
}

const dataUrls = new WeakMap<ImageAttachment, string>()
/** `data:` URL for an attached image, built once per attachment. */
function dataUrl(img: ImageAttachment): string {
  let url = dataUrls.get(img)
  if (!url) dataUrls.set(img, (url = `data:${img.media_type};base64,${img.base64}`))
  return url
}

/** One attachment: a thumbnail or file icon, its name, and remove. */
export function AttachmentChip({ label, kind, image, onRemove }: { label: string; kind: 'image' | 'file'; image?: string; onRemove?: () => void }) {
  return (
    <Stack
      direction="row"
      aria-label={`${kind === 'image' ? 'Image' : 'File'}: ${label}`}
      sx={{ alignItems: 'center', gap: 0.75, pl: image ? 0.375 : 1, pr: onRemove ? 0.375 : 1, py: 0.375, borderRadius: '10px', border: 1, borderColor: 'divider', bgcolor: 'background.paper', maxWidth: 240 }}
    >
      {image ? (
        <Box component="img" src={image} alt="" sx={{ width: 28, height: 28, borderRadius: '7px', objectFit: 'cover', flexShrink: 0 }} />
      ) : (
        <Box sx={{ display: 'inline-flex', color: 'text.secondary', flexShrink: 0 }}>
          <DocumentTextLinear size={14} color="currentColor" />
        </Box>
      )}
      <Typography noWrap sx={{ fontSize: '0.75rem', minWidth: 0 }}>
        {label}
      </Typography>
      {onRemove && (
        <ButtonBase aria-label={`Remove ${label}`} onClick={onRemove} sx={{ borderRadius: '6px', p: 0.25, color: 'text.secondary', '&:hover': { color: 'text.primary' } }}>
          <CloseLinear size={12} color="currentColor" />
        </ButtonBase>
      )}
    </Stack>
  )
}

const pulse = keyframes`
  0%, 100% { box-shadow: 0 0 0 0 rgba(239, 68, 68, 0.45); }
  50%      { box-shadow: 0 0 0 5px rgba(239, 68, 68, 0); }
`

/**
 * Speak instead of typing: click to start, click (or Esc) to stop. The
 * words show as they're heard and land in the box when you stop. Uses the
 * same on-device speech engine as Viv Instant, downloading it the first time.
 */
function DictateButton({ onText, onInterim, onError }: { onText: (t: string) => void; onInterim: (t: string) => void; onError: (e: string) => void }) {
  const [state, setState] = useState<'off' | 'preparing' | 'listening' | 'finishing'>('off')
  const [percent, setPercent] = useState<number | null>(null)
  const mic = useInstantMic({
    onInterim,
    onUtterance: (t) => {
      setState('off')
      onText(t)
    },
    onError: (e) => {
      setState('off')
      onInterim('')
      onError(e)
    },
  })

  useEffect(() => {
    if (state !== 'preparing') return
    const off = on<{ received: number; total: number }>('instant:stt_download', ({ received, total }) => setPercent(total > 0 ? Math.round((received / total) * 100) : null))
    return () => void off.then((f) => f())
  }, [state])

  const start = async () => {
    setState('preparing')
    try {
      // Downloads the speech model the first time; instant after that.
      await call('cmd_instant_stt_preload', {})
      setPercent(null)
      setState('listening')
      await mic.start({ hold: true })
    } catch (e) {
      setState('off')
      onError(`Dictation isn't available: ${e instanceof Error ? e.message : String(e)}`)
    }
  }
  const stop = () => {
    setState('finishing')
    mic.release()
  }

  useEffect(() => {
    if (state !== 'listening') return
    const esc = (e: globalThis.KeyboardEvent) => {
      if (e.key === 'Escape') stop()
    }
    window.addEventListener('keydown', esc)
    return () => window.removeEventListener('keydown', esc)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state])

  const label =
    state === 'preparing' ? (percent !== null ? `Getting the speech model… ${percent}%` : 'Starting the microphone…') : state === 'listening' ? 'Listening: click to finish' : state === 'finishing' ? 'Writing it down…' : 'Dictate'
  return (
    <Tooltip title={label}>
      <span>
        <IconButton
          aria-label={label}
          aria-pressed={state === 'listening'}
          disabled={state === 'preparing' || state === 'finishing'}
          onClick={() => (state === 'listening' ? stop() : void start())}
          sx={{
            width: 30,
            height: 30,
            borderRadius: '50%',
            flexShrink: 0,
            color: state === 'listening' ? 'common.white' : 'text.secondary',
            bgcolor: state === 'listening' ? 'error.main' : 'transparent',
            animation: state === 'listening' ? `${pulse} 1.4s ease-in-out infinite` : undefined,
            '&:hover': { bgcolor: state === 'listening' ? 'error.dark' : 'action.hover', color: state === 'listening' ? 'common.white' : 'text.primary' },
          }}
        >
          <MicrophoneLinear size={15} color="currentColor" />
        </IconButton>
      </span>
    </Tooltip>
  )
}
