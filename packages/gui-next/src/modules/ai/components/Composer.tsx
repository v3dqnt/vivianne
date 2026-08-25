import { useCallback, useEffect, useMemo, useRef, useState, memo, type KeyboardEvent } from 'react'
import Box from '@mui/material/Box'
import Paper from '@mui/material/Paper'
import Stack from '@mui/material/Stack'
import TextField from '@mui/material/TextField'
import IconButton from '@mui/material/IconButton'
import Typography from '@mui/material/Typography'
import { SendLinear, CloseCircleLinear, DocumentTextLinear, StopLinear, SparkleLinear } from '../../../icons'
import { useAgentStore } from '../store/agentStore'
import { useWorkspaceStore } from '../store/workspaceStore'
import { call } from '../../../ipc'
import type { ImageAttachment, FileAttachment } from '../lib/types'
import { ModelSelector as ModelSelectorImpl } from './ModelSelector'
import { ThinkingLevelControl as ThinkingLevelControlImpl } from './ThinkingLevelControl'
import { WorkspaceSelector as WorkspaceSelectorImpl } from './WorkspaceSelector'
import { BranchIndicator as BranchIndicatorImpl } from './BranchIndicator'
import { PermissionModePill as PermissionModePillImpl } from './PermissionModePill'
import { AttachFileButton as AttachFileButtonImpl } from './AttachFileButton'
import { ContextUsageBar as ContextUsageBarImpl } from './ContextUsageBar'
import { StreamStatusLine as StreamStatusLineImpl } from './StreamStatusLine'
import { PlanModeBanner as PlanModeBannerImpl } from './PlanModeBanner'
import { SubagentPanel as SubagentPanelImpl } from './SubagentPanel'
import { ComputerUseBanner as ComputerUseBannerImpl } from './ComputerUseBanner'
import { QueuePanel as QueuePanelImpl } from './QueuePanel'
import { UnifiedMentionMenu, type MentionItem, type MentionableFile, type MentionableSkill } from './MentionMenu'
import { getSkillMentionQuery, applySkillMention, applyFileMention } from '../../../lib/mentions'

// Every one of these is a zero-prop sibling that subscribes to its own store
// slices — but Composer itself re-renders on every keystroke (text state) and
// every token (isBusy), which used to drag all of them along. Wrapping at the
// import site lets each bail out of those renders; context/store updates
// still get through.
const ModelSelector = memo(ModelSelectorImpl)
const ThinkingLevelControl = memo(ThinkingLevelControlImpl)
const WorkspaceSelector = memo(WorkspaceSelectorImpl)
const BranchIndicator = memo(BranchIndicatorImpl)
const PermissionModePill = memo(PermissionModePillImpl)
const AttachFileButton = memo(AttachFileButtonImpl)
const ContextUsageBar = memo(ContextUsageBarImpl)
const StreamStatusLine = memo(StreamStatusLineImpl)
const PlanModeBanner = memo(PlanModeBannerImpl)
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
  type?: string
  children?: TreeFileNode[]
}

function flattenTree(nodes: TreeFileNode[], prefix: string, out: MentionableFile[]): void {
  for (const n of nodes) {
    const rel = prefix ? `${prefix}/${n.name}` : n.name
    const isDir = n.type === 'directory'
    out.push({ relPath: rel, name: n.name, isDir })
    if (isDir && Array.isArray(n.children) && n.children.length > 0) {
      flattenTree(n.children, rel, out)
    }
    if (out.length > 1500) return
  }
}

export function Composer() {
  const [text, setText] = useState('')
  const [pending, setPending] = useState<ImageAttachment[]>([])
  const [pendingFiles, setPendingFiles] = useState<FileAttachment[]>([])
  const isBusy = useAgentStore((s) => s.isBusy)
  const sendMessage = useAgentStore((s) => s.sendMessage)
  const steer = useAgentStore((s) => s.steer)
  const abort = useAgentStore((s) => s.abort)
  const workspaceFolders = useWorkspaceStore((s) => s.folders)

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
  const fileTreeCache = useRef<{ root: string; files: MentionableFile[] } | null>(null)

  // Fetches the (unfiltered) tree once per workspace root, not per
  // keystroke — `cmd_list_dir_tree` takes no query param, so the actual
  // per-character filtering in `mentionItems` below was being fed a fresh,
  // identical, unfiltered walk of up to 800 entries on every character typed
  // after `@`. Depending on `mentionActive` (only flips on entering/leaving
  // a mention) instead of `mentionQuery` (changes every keystroke) plus a
  // per-root cache means the IPC call fires at most once per workspace.
  useEffect(() => {
    if (!mentionActive || workspaceFolders.length === 0) return
    const root = workspaceFolders[0]
    if (fileTreeCache.current?.root === root) {
      setFileMatches(fileTreeCache.current.files)
      return
    }
    let cancelled = false
    void call<TreeFileNode[]>('cmd_list_dir_tree', {
      root,
      maxDepth: 4,
      maxEntries: 800,
    }).then((tree) => {
      if (cancelled || !tree) return
      const flat: MentionableFile[] = []
      flattenTree(tree, '', flat)
      fileTreeCache.current = { root, files: flat }
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
      .slice(0, 8)
      .map((file) => ({ kind: 'file', file }))
    return [...skillItems, ...fileItems]
  }, [mentionQuery, skills, fileMatches])

  const mentionOpen = mentionItems.length > 0

  useEffect(() => {
    setMentionIndex(0)
  }, [mentionQuery])

  const applyMention = (item: MentionItem) => {
    setText((v) => (item.kind === 'skill' ? applySkillMention(v, item.skill.name) : applyFileMention(v, item.file.relPath)))
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
    void sendMessage(toSend, images)
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

  return (
    // The gap on all sides (not just top) plus each panel's own radius/shadow
    // is what reads as "floating" rather than a bar glued to the window edge.
    <Box sx={{ flexShrink: 0, px: 2, pb: 2, pt: 0.5, position: 'relative' }}>
      <Stack sx={{ gap: 0.5 }}>
        {/* Context strip — informational, sits above the input as its own
            small surface rather than crowding the composer's own toolbar. */}
        <Stack
          direction="row"
          sx={{
            alignSelf: 'flex-start',
            alignItems: 'center',
            gap: 0.5,
            px: 0.5,
            py: 0.25,
            borderRadius: 999,
            bgcolor: 'rgba(255,255,255,0.04)',
          }}
        >
          <WorkspaceSelector />
          <BranchIndicator />
          <ComputerUseBanner />
        </Stack>

        <PlanModeBanner />
        <SubagentPanel />
        <StreamStatusLine />

        {/* Inline, in normal flow — no longer a floating popover. */}
        <QueuePanel />

        {/* Mention menu still needs its own anchor to float over the input. */}
        <Box sx={{ position: 'relative' }}>
          {mentionOpen && <UnifiedMentionMenu items={mentionItems} activeIndex={mentionIndex} onSelect={applyMention} onHover={setMentionIndex} />}

          <Paper
            elevation={0}
            sx={{
              borderRadius: 3,
              border: 1,
              borderColor: 'rgba(255,255,255,0.08)',
              bgcolor: 'rgba(255,255,255,0.045)',
              backdropFilter: 'blur(20px) saturate(150%)',
              WebkitBackdropFilter: 'blur(20px) saturate(150%)',
              boxShadow: '0 12px 32px rgba(0,0,0,0.4), inset 0 1px 0 rgba(255,255,255,0.06)',
              overflow: 'hidden',
            }}
          >
            {(pending.length > 0 || pendingFiles.length > 0) && (
              <Stack direction="row" spacing={1} sx={{ px: 2, pt: 1.5, flexWrap: 'wrap' }}>
                {pending.map((img, i) => (
                  <Box key={i} sx={{ position: 'relative' }}>
                    <Box
                      component="img"
                      src={`data:${img.media_type};base64,${img.base64}`}
                      sx={{ width: 56, height: 56, objectFit: 'cover', borderRadius: 2, border: 1, borderColor: 'rgba(255,255,255,0.1)' }}
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
                      borderColor: 'rgba(255,255,255,0.1)',
                      bgcolor: 'rgba(255,255,255,0.03)',
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
              minRows={2}
              maxRows={8}
              placeholder={isBusy ? 'Agent is working — send to queue, or steer to redirect now...' : 'Do anything'}
              value={text}
              onChange={(e) => setText(e.target.value)}
              onKeyDown={onKeyDown}
              variant="standard"
              slotProps={{ input: { disableUnderline: true } }}
              sx={{ px: 2, pt: 1.25, pb: 0.5, '& .MuiInputBase-root': { fontSize: '0.9375rem' } }}
            />

            <Stack direction="row" sx={{ alignItems: 'center', gap: 0.75, px: 1.5, pb: 1, pt: 0.5 }}>
              <AttachFileButton
                onAddImages={handleAddImages}
                onAddFiles={handleAddFiles}
              />
              <PermissionModePill />

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
                disabled={!isBusy && !text.trim() && pending.length === 0 && pendingFiles.length === 0}
                title={!isBusy ? 'Send' : text.trim() ? 'Steer — interrupt the current task and redirect it with this message' : 'Stop — end the current task'}
                sx={{
                  bgcolor: !isBusy ? 'action.selected' : text.trim() ? 'primary.main' : 'error.main',
                  color: !isBusy ? 'text.primary' : text.trim() ? 'primary.contrastText' : 'error.contrastText',
                  width: 32,
                  height: 32,
                  flexShrink: 0,
                  transition: 'background-color 0.1s ease, transform 0.1s ease',
                  '&:hover': { bgcolor: !isBusy ? 'primary.main' : text.trim() ? 'primary.dark' : 'error.dark', color: !isBusy ? 'primary.contrastText' : undefined },
                  '&:active': { transform: 'scale(0.92)' },
                  '&.Mui-disabled': { bgcolor: 'action.disabledBackground', color: 'text.disabled' },
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
