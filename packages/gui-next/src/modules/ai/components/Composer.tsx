import { useCallback, useEffect, useMemo, useRef, useState, memo, type KeyboardEvent } from 'react'
import { Box, Paper, Stack, TextField, IconButton, Typography, alpha, useTheme } from '../../../ui'
import { SendLinear, CloseCircleLinear, DocumentTextLinear, StopLinear, SparkleLinear } from '../../../icons'
import { useAgentStore } from '../store/agentStore'
import { useWorkspaceStore } from '../store/workspaceStore'
import { useDaysStore } from '../../sessions/store/daysStore'
import { call } from '../../../ipc'
import type { ImageAttachment, FileAttachment } from '../lib/types'
import { ModelSelector as ModelSelectorImpl } from './ModelSelector'
import { ThinkingLevelControl as ThinkingLevelControlImpl } from './ThinkingLevelControl'
import { PermissionModePill as PermissionModePillImpl } from './PermissionModePill'
import { AttachFileButton as AttachFileButtonImpl } from './AttachFileButton'
import { ContextUsageBar as ContextUsageBarImpl } from './ContextUsageBar'
import { StreamStatusLine as StreamStatusLineImpl } from './StreamStatusLine'
import { RunStatusBar as RunStatusBarImpl } from './RunStatusBar'
import { LimitsPill as LimitsPillImpl } from './LimitsPill'
import { PlanPill as PlanPillImpl } from '../../plan/components/PlanPill'
import { usePlanStore } from '../../plan/store/planStore'
import { SubagentPanel as SubagentPanelImpl } from './SubagentPanel'
import { ComputerUseBanner as ComputerUseBannerImpl } from './ComputerUseBanner'
import { QueuePanel as QueuePanelImpl } from './QueuePanel'
import { UnifiedMentionMenu, type MentionItem, type MentionableFile, type MentionableSkill } from './MentionMenu'
import { buildAttachments, filesFromDataTransfer } from '../lib/attachments'
import { getSkillMentionQuery, applySkillMention, applyFileMention } from '../../../lib/mentions'

// Every one of these is a zero-prop sibling that subscribes to its own store
// slices — but Composer itself re-renders on every keystroke (text state) and
// every token (isBusy), which used to drag all of them along. Wrapping at the
// import site lets each bail out of those renders; context/store updates
// still get through.
const ModelSelector = memo(ModelSelectorImpl)
const ThinkingLevelControl = memo(ThinkingLevelControlImpl)
const PermissionModePill = memo(PermissionModePillImpl)
const AttachFileButton = memo(AttachFileButtonImpl)
const ContextUsageBar = memo(ContextUsageBarImpl)
const StreamStatusLine = memo(StreamStatusLineImpl)
const RunStatusBar = memo(RunStatusBarImpl)
const LimitsPill = memo(LimitsPillImpl)
const PlanPill = memo(PlanPillImpl)
const SubagentPanel = memo(SubagentPanelImpl)
const ComputerUseBanner = memo(ComputerUseBannerImpl)
const QueuePanel = memo(QueuePanelImpl)

/** Files ride along as plain text — no dedicated wire type, so they're woven
 * into the outgoing message the same way the image-count line primes the
 * model today, just with the actual content instead of a mention. */
function formatFilesForPrompt(files: FileAttachment[]): string {
  if (files.length === 0) return ''
  const blocks = files
    .map((f) => `<file path="${f.name}"${f.truncated ? ' truncated="true"' : ''}>\n${f.content}\n</file>`)
    .join('\n\n')
  return `\n\n${blocks}`
}

/** Flatten a `cmd_list_dir_tree` response into mentionable rows, capped so a
 * huge repo can't hang the picker. */
interface TreeFileNode {
  name: string
  path?: string
  type?: string
  children?: TreeFileNode[]
}

/** `displayPrefix` is the folder-qualified prefix shown in the menu (empty
 * for a single-folder workspace); `absolute` decides whether selecting a row
 * inserts the real path instead of the workspace-relative one. */
function flattenTree(
  nodes: TreeFileNode[],
  prefix: string,
  displayPrefix: string,
  absolute: boolean,
  out: MentionableFile[],
): void {
  for (const n of nodes) {
    const rel = prefix ? `${prefix}/${n.name}` : n.name
    const display = displayPrefix ? `${displayPrefix}/${rel}` : rel
    const isDir = n.type === 'directory'
    out.push({
      relPath: display,
      name: n.name,
      isDir,
      insertPath: absolute && n.path ? n.path.replace(/\\/g, '/') : rel,
    })
    if (isDir && Array.isArray(n.children) && n.children.length > 0) {
      flattenTree(n.children, rel, displayPrefix, absolute, out)
    }
    if (out.length > MAX_MENTIONABLE_FILES) return
  }
}

/** Deep/large enough to cover a real repo — `cmd_list_dir_tree` itself
 * clamps to depth 8 / 5000 entries, so these ask for everything it will
 * give. The old 4/800 quietly hid most of any non-trivial tree, which is
 * what made whole folders unmentionable. */
const TREE_MAX_DEPTH = 8
const TREE_MAX_ENTRIES = 5000
const MAX_MENTIONABLE_FILES = 8000

function basename(p: string): string {
  return p.split(/[\\/]/).filter(Boolean).pop() || p
}

/** The composer's narrow layout: controls collapse to their icons. */
const NARROW = {
    '& .cq-label, & .cq-wide': { display: 'none' },
    '& .cq-compact': { display: 'inline-flex' },
    '& .cq-context': { minWidth: 0 },
    // A lone icon inside a MUI Button still carries the margin meant to
    // separate it from the (now hidden) text.
    '& .cq-collapsible': { minWidth: 32, px: 0.75 },
    '& .cq-collapsible .ui-Button-startIcon, & .cq-collapsible .ui-Button-endIcon': { m: 0 },
  }

/**
 * Container queries need Safari 16; macOS 10.15's WebKit stops at 15.6 and
 * ignores them, leaving a narrow composer in its wide layout. There, measure
 * the element instead and mark it `data-narrow`, which the same styles key on.
 */
function useNarrowFallback(ref: React.RefObject<HTMLElement | null>, maxWidth: number) {
  useEffect(() => {
    const el = ref.current
    if (!el || typeof CSS === 'undefined' || CSS.supports('container-type: inline-size')) return
    const ro = new ResizeObserver(([entry]) => {
      el.dataset.narrow = String(entry.contentRect.width <= maxWidth)
    })
    ro.observe(el)
    return () => ro.disconnect()
  }, [ref, maxWidth])
}

export function Composer() {
  const theme = useTheme()
  const [text, setText] = useState('')
  const [pending, setPending] = useState<ImageAttachment[]>([])
  const [pendingFiles, setPendingFiles] = useState<FileAttachment[]>([])
  const [dragging, setDragging] = useState(false)
  const isBusy = useAgentStore((s) => s.isBusy)
  const stopping = useAgentStore((s) => s.stopping)
  const planMode = usePlanStore((s) => s.planMode)
  const sendMessage = useAgentStore((s) => s.sendMessage)
  const steer = useAgentStore((s) => s.steer)
  const abort = useAgentStore((s) => s.abort)
  const workspaceFolders = useWorkspaceStore((s) => s.folders)
  const draft = useAgentStore((s) => s.draft)
  const setDraft = useAgentStore((s) => s.setDraft)

  // One-shot: "edit this message" hands the original text over through the
  // store, and the composer takes ownership of it here and clears the
  // channel, so re-renders can't keep re-applying a stale draft over what
  // the user has since typed.
  useEffect(() => {
    if (!draft) return
    setText(draft)
    setDraft('')
    inputRef.current?.focus()
  }, [draft, setDraft])

  // Opening the project lead from the sidebar puts the cursor here.
  useEffect(() => {
    const focus = () => inputRef.current?.focus()
    window.addEventListener('viv:focus-composer', focus)
    return () => window.removeEventListener('viv:focus-composer', focus)
  }, [])

  // ── @ mentions (skills + workspace files) ──
  const [skills, setSkills] = useState<MentionableSkill[]>([])
  const [fileMatches, setFileMatches] = useState<MentionableFile[]>([])
  const [mentionIndex, setMentionIndex] = useState(0)
  const inputRef = useRef<HTMLInputElement>(null)

  // Fetch skills once on mount — the default `~/.viv/skills` dir is always
  // scanned by cmd_list_skills regardless of the extra-paths argument.
  useEffect(() => {
    void call<{ skills?: { name?: string; description?: string }[] }>('cmd_list_skills', { paths: [] }).then((res) => {
      if (res?.skills) setSkills(res.skills.map((s) => ({ name: s.name ?? '', description: s.description ?? '' })))
    })
  }, [])

  const mentionQuery = getSkillMentionQuery(text)
  const mentionActive = mentionQuery !== null
  const fileTreeCache = useRef<{ key: string; files: MentionableFile[] } | null>(null)

  // Fetches the (unfiltered) tree once per workspace root, not per
  // keystroke — `cmd_list_dir_tree` takes no query param, so the actual
  // per-character filtering in `mentionItems` below was being fed a fresh,
  // identical, unfiltered walk of up to 800 entries on every character typed
  // after `@`. Depending on `mentionActive` (only flips on entering/leaving
  // a mention) instead of `mentionQuery` (changes every keystroke) plus a
  // per-root cache means the IPC call fires at most once per workspace.
  useEffect(() => {
    if (!mentionActive || workspaceFolders.length === 0) return
    // Every workspace folder, not just the primary one — a folder added
    // alongside the first was previously unmentionable entirely.
    const key = workspaceFolders.join('\0')
    if (fileTreeCache.current?.key === key) {
      setFileMatches(fileTreeCache.current.files)
      return
    }
    let cancelled = false
    const multi = workspaceFolders.length > 1
    void Promise.all(
      workspaceFolders.map((root) =>
        call<TreeFileNode[]>('cmd_list_dir_tree', { root, maxDepth: TREE_MAX_DEPTH, maxEntries: TREE_MAX_ENTRIES })
          .then((tree) => ({ root, tree }))
          // One unreadable folder (permissions, unplugged drive) shouldn't
          // take the other folders' files down with it.
          .catch(() => ({ root, tree: null })),
      ),
    ).then((results) => {
      if (cancelled) return
      const flat: MentionableFile[] = []
      for (const { root, tree } of results) {
        if (!tree) continue
        flattenTree(tree, '', multi ? basename(root) : '', multi, flat)
      }
      fileTreeCache.current = { key, files: flat }
      setFileMatches(flat)
    })
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mentionActive, workspaceFolders])

  const mentionItems: MentionItem[] = useMemo(() => {
    if (mentionQuery === null) return []
    const q = mentionQuery.toLowerCase()
    const skillItems: MentionItem[] = skills
      .filter((s) => s.name.toLowerCase().includes(q))
      .slice(0, 5)
      .map((skill) => ({ kind: 'skill', skill }))
    const fileItems: MentionItem[] = fileMatches
      .filter((f) => f.relPath.toLowerCase().includes(q) || f.name.toLowerCase().includes(q))
      // The menu scrolls, so this only caps how much is rendered at once —
      // 8 was low enough that a common query buried the file you wanted.
      .slice(0, 50)
      .map((file) => ({ kind: 'file', file }))
    return [...skillItems, ...fileItems]
  }, [mentionQuery, skills, fileMatches])

  const mentionOpen = mentionItems.length > 0

  useEffect(() => {
    setMentionIndex(0)
  }, [mentionQuery])

  const applyMention = (item: MentionItem) => {
    setText((v) => (item.kind === 'skill' ? applySkillMention(v, item.skill.name) : applyFileMention(v, item.file.insertPath)))
    inputRef.current?.focus()
  }

  const submit = () => {
    const hasAnything = text.trim() || pending.length > 0 || pendingFiles.length > 0
    if (!hasAnything) return
    const toSend = text + formatFilesForPrompt(pendingFiles)
    const images = pending
    setText('')
    setPending([])
    setPendingFiles([])
    // `sendMessage` routes to the steering queue when busy — one code path
    // for both idle-send and send-while-busy, so the send button always
    // matches the Enter behavior (queue) instead of silently doing nothing.
    //
    // `ensureToday` first: a conversation left open past midnight rolls over
    // to the new day on its next message, rather than appending to
    // yesterday's. It is a no-op the rest of the time.
    void useDaysStore
      .getState()
      .ensureToday()
      .finally(() => void sendMessage(toSend, images))
  }

  const doSteer = () => {
    if (!text.trim()) return
    const toSend = text + formatFilesForPrompt(pendingFiles)
    setText('')
    setPending([])
    setPendingFiles([])
    void steer(toSend)
  }

  // Stable identities so the memoized AttachFileButton actually bails on
  // keystroke re-renders instead of seeing new props every time.
  const handleAddImages = useCallback((imgs: ImageAttachment[]) => setPending((p) => [...p, ...imgs]), [])
  const handleAddFiles = useCallback((files: FileAttachment[]) => setPendingFiles((p) => [...p, ...files]), [])

  /** Shared by paste and drop so every entry point accepts the same types.
   * Images become vision attachments; anything else becomes a file
   * attachment, with binaries described rather than decoded into mojibake
   * (see `buildAttachments`). */
  const ingestFiles = useCallback(async (files: File[]) => {
    if (files.length === 0) return
    const { images, files: docs } = await buildAttachments(files)
    if (images.length) setPending((p) => [...p, ...images])
    if (docs.length) setPendingFiles((p) => [...p, ...docs])
  }, [])

  const onPaste = useCallback(
    (e: React.ClipboardEvent) => {
      const files = filesFromDataTransfer(e.clipboardData)
      if (files.length === 0) return
      // Only when there ARE files — otherwise this would swallow ordinary
      // text pastes into the composer.
      e.preventDefault()
      void ingestFiles(files)
    },
    [ingestFiles],
  )

  const onDrop = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault()
      setDragging(false)
      void ingestFiles(filesFromDataTransfer(e.dataTransfer))
    },
    [ingestFiles],
  )

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (mentionOpen) {
      if (e.key === 'ArrowDown') {
        e.preventDefault()
        setMentionIndex((i) => (i + 1) % mentionItems.length)
        return
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault()
        setMentionIndex((i) => (i - 1 + mentionItems.length) % mentionItems.length)
        return
      }
      if (e.key === 'Enter' || e.key === 'Tab') {
        e.preventDefault()
        applyMention(mentionItems[mentionIndex])
        return
      }
      if (e.key === 'Escape') {
        e.preventDefault()
        setText((v) => v.replace(/@[A-Za-z0-9_./\\-]*$/, ''))
        return
      }
    }
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      submit()
    }
  }

  const rootRef = useRef<HTMLDivElement | null>(null)
  useNarrowFallback(rootRef, 560)

  return (
    // The gap on all sides (not just top) plus each panel's own radius/shadow
    // is what reads as "floating" rather than a bar glued to the window edge.
    // maxWidth/mx match MessageList's transcript cap so the composer's edges
    // line up with the message column above it instead of stretching wider.
    <Box
      ref={rootRef}
      onPaste={onPaste}
      onDrop={onDrop}
      onDragOver={(e) => {
        // Both preventDefaults are required for a drop target: without them
        // the browser navigates to the dropped file instead of firing onDrop.
        e.preventDefault()
        if (!dragging) setDragging(true)
      }}
      onDragLeave={(e) => {
        // Fires for every child too, so ignore the ones where the pointer is
        // still inside the composer.
        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setDragging(false)
      }}
      sx={{
        flexShrink: 0,
        pb: 2,
        pt: 0.5,
        position: 'relative',
        maxWidth: 880,
        mx: 'auto',
        width: '100%',
        // The composer measures ITSELF, not the window. The chat is a panel now
        // and can be docked anywhere from a full column to a narrow sidebar,
        // so the window's width says nothing about how much room it has.
        containerType: 'inline-size',
        containerName: 'composer',
        px: 2,
        // Compact-only pieces are hidden until the container query below
        // swaps them in.
        '& .cq-compact': { display: 'none' },
        // Narrow: controls collapse to their icons. Nothing is removed — the
        // full value is in each control's tooltip and accessible label, and a
        // click opens the same menu as before.
        '@container composer (max-width: 560px)': NARROW,
        // Older WebKit (macOS 10.15's) has no container queries; see `useNarrowFallback`.
        '&[data-narrow="true"]': NARROW,
      }}
    >
      {dragging && (
        <Box
          sx={{
            position: 'absolute',
            inset: 8,
            zIndex: 10,
            borderRadius: '12px',
            border: '2px dashed',
            borderColor: 'primary.main',
            bgcolor: 'action.hover',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            pointerEvents: 'none',
            fontSize: '0.8125rem',
            color: 'primary.main',
            fontWeight: 600,
          }}
        >
          Drop files to attach
        </Box>
      )}
      <Stack sx={{ gap: 0.5 }}>
        {/* The workspace and branch chips moved to the sidebar, beside the
            project they belong to. Only the computer-use notice stays here —
            it is about the run in progress, which is this box's business. */}
        <ComputerUseBanner />

        <SubagentPanel />
        <StreamStatusLine />

        {/* Inline, in normal flow — no longer a floating popover. */}
        <QueuePanel />

        {/* Mention menu still needs its own anchor to float over the input. */}
        <Box sx={{ position: 'relative' }}>
          {/* Docked on the composer's top edge — part of the box, not a card above it. */}
          <RunStatusBar />
          {mentionOpen && <UnifiedMentionMenu items={mentionItems} activeIndex={mentionIndex} onSelect={applyMention} onHover={setMentionIndex} />}

          <Paper
            elevation={0}
            sx={{
              // Terax's composer: a card on the pane — card fill, a hairline,
              // 18px corners. Focus shows the ring colour and its soft halo.
              borderRadius: '18px',
              overflow: 'hidden',
              bgcolor: 'background.card',
              border: 1,
              borderColor: 'divider',
              boxShadow: 'none',
              transition: 'border-color 0.16s ease, box-shadow 0.16s ease',
              '&:focus-within': { borderColor: theme.palette.tokens.ring, boxShadow: `0 0 0 3px ${alpha(theme.palette.tokens.ring, 0.2)}` },
            }}
          >
            {(pending.length > 0 || pendingFiles.length > 0) && (
              <Stack direction="row" spacing={1} sx={{ px: 2, pt: 1.5, flexWrap: 'wrap' }}>
                {pending.map((img, i) => (
                  <Box key={i} sx={{ position: 'relative' }}>
                    <Box
                      component="img"
                      src={`data:${img.media_type};base64,${img.base64}`}
                      sx={{ width: 56, height: 56, objectFit: 'cover', borderRadius: 2, border: 1, borderColor: 'divider' }}
                    />
                    <IconButton
                      size="small"
                      onClick={() => setPending((p) => p.filter((_, j) => j !== i))}
                      sx={{ position: 'absolute', top: -8, right: -8, bgcolor: 'background.paper', p: 0.25, '&:hover': { bgcolor: 'background.paper' } }}
                    >
                      <CloseCircleLinear size={16} color="currentColor" />
                    </IconButton>
                  </Box>
                ))}
                {pendingFiles.map((f, i) => (
                  <Stack
                    key={i}
                    direction="row"
                    sx={{
                      alignItems: 'center',
                      gap: 0.5,
                      pl: 1,
                      pr: 0.5,
                      py: 0.5,
                      borderRadius: 2,
                      border: 1,
                      borderColor: 'divider',
                      bgcolor: 'action.hover',
                      maxWidth: 200,
                    }}
                  >
                    <DocumentTextLinear size={14} color="currentColor" />
                    <Typography variant="caption" noWrap title={f.name} sx={{ minWidth: 0 }}>
                      {f.name}
                    </Typography>
                    <IconButton size="small" onClick={() => setPendingFiles((p) => p.filter((_, j) => j !== i))} sx={{ p: 0.25 }}>
                      <CloseCircleLinear size={14} color="currentColor" />
                    </IconButton>
                  </Stack>
                ))}
              </Stack>
            )}

            <TextField
              inputRef={inputRef}
              fullWidth
              multiline
              minRows={1}
              maxRows={8}
              placeholder={isBusy ? 'Agent is working — send to queue, or steer to redirect now...' : planMode ? 'Describe what you want built — the agent will research it and propose a plan' : 'Do anything'}
              value={text}
              onChange={(e) => setText(e.target.value)}
              onKeyDown={onKeyDown}
              variant="standard"
              slotProps={{ input: { disableUnderline: true } }}
              sx={{ px: 1.5, pt: 1.25, pb: 0.75, '& .ui-InputBase-root': { fontSize: '0.8125rem', lineHeight: 1.65 } }}
            />

            <Stack direction="row" sx={{ alignItems: 'center', gap: 0.5, px: 1, pb: 0.75 }}>
              <AttachFileButton
                onAddImages={handleAddImages}
                onAddFiles={handleAddFiles}
              />
              <PermissionModePill />
              <PlanPill />
              <LimitsPill />

              <Box sx={{ flex: 1 }} />

              <ContextUsageBar />
              <ThinkingLevelControl />
              <ModelSelector />

              {/* One button covers send/steer/abort — idle sends (or queues,
                  via the same submit()), busy-with-empty-input stops the
                  run, busy-with-text-typed redirects it. Typing while busy
                  is already the signal that means "steer," so that's the
                  default action for that state rather than a separate
                  button alongside abort. */}
              <IconButton
                onClick={() => {
                  if (!isBusy) submit()
                  else if (text.trim()) doSteer()
                  else void abort()
                }}
                disabled={(!isBusy && !text.trim() && pending.length === 0 && pendingFiles.length === 0) || (stopping && !text.trim())}
                title={!isBusy ? 'Send' : text.trim() ? 'Steer — interrupt the current task and redirect it with this message' : stopping ? 'Stopping…' : 'Stop — end the current task'}
                sx={{
                  bgcolor: !isBusy ? 'action.selected' : text.trim() ? 'primary.main' : 'error.main',
                  color: !isBusy ? 'text.primary' : text.trim() ? 'primary.contrastText' : 'error.contrastText',
                  width: 28,
                  height: 28,
                  borderRadius: '8px',
                  flexShrink: 0,
                  transition: 'background-color 0.1s ease, transform 0.1s ease',
                  '&:hover': { bgcolor: !isBusy ? 'primary.main' : text.trim() ? 'primary.dark' : 'error.dark', color: !isBusy ? 'primary.contrastText' : undefined },
                  '&:active': { transform: 'scale(0.92)' },
                  '&:disabled, &[aria-disabled="true"]': { bgcolor: 'action.disabledBackground', color: 'text.disabled' },
                }}
              >
                {isBusy && !text.trim() ? (
                  <StopLinear size={15} color="currentColor" />
                ) : isBusy ? (
                  <SparkleLinear size={15} color="currentColor" />
                ) : (
                  <SendLinear size={15} color="currentColor" />
                )}
              </IconButton>
            </Stack>
          </Paper>
        </Box>
      </Stack>
    </Box>
  )
}
