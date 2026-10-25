import { useEffect, useMemo, useRef, useState, memo, type KeyboardEvent } from 'react'
import { Box, Stack } from '../../../ui'
import { useAgentStore } from '../store/agentStore'
import { useWorkspaceStore } from '../store/workspaceStore'
import { useDaysStore } from '../../sessions/store/daysStore'
import { call, on } from '../../../ipc'
import type { FileAttachment } from '../lib/types'
import { ModelPicker } from './ModelPicker'
import { ChatComposer, type ChatComposerHandle, type ComposerAttachments } from './ChatComposer'
import { ThinkingLevelControl as ThinkingLevelControlImpl } from './ThinkingLevelControl'
import { ModePill as ModePillImpl } from './ModePill'
import { ContextUsageBar as ContextUsageBarImpl } from './ContextUsageBar'
import { StreamStatusLine as StreamStatusLineImpl } from './StreamStatusLine'
import { RunStatusBar as RunStatusBarImpl } from './RunStatusBar'
import { LimitsPill as LimitsPillImpl } from './LimitsPill'
import { usePlanStore } from '../../plan/store/planStore'
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
const ChatModelPicker = memo(function ChatModelPicker() {
  return <ModelPicker mode="chat" />
})
const ThinkingLevelControl = memo(ThinkingLevelControlImpl)
const ModePill = memo(ModePillImpl)
const ContextUsageBar = memo(ContextUsageBarImpl)
const StreamStatusLine = memo(StreamStatusLineImpl)
const RunStatusBar = memo(RunStatusBarImpl)
const LimitsPill = memo(LimitsPillImpl)
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
    // The model's name shortens rather than going: it is the one control
    // whose text says what is selected.
    '& .cq-model': { maxWidth: 110 },
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

/** The workspace's files for @-mentions, kept across chats and remounts.
 * ponytail: a time window; a watcher-driven index if trees get huge. */
let fileTreeCache: { key: string; files: MentionableFile[]; at: number } | null = null
const FILE_TREE_TTL_MS = 30_000

export function Composer() {
  const [text, setText] = useState('')
  const isBusy = useAgentStore((s) => s.isBusy)
  const stopping = useAgentStore((s) => s.stopping)
  const planMode = usePlanStore((s) => s.planMode)
  const sendMessage = useAgentStore((s) => s.sendMessage)
  const steer = useAgentStore((s) => s.steer)
  const abort = useAgentStore((s) => s.abort)
  const workspaceFolders = useWorkspaceStore((s) => s.folders)
  const draft = useAgentStore((s) => s.draft)
  const setDraft = useAgentStore((s) => s.setDraft)
  const box = useRef<ChatComposerHandle | null>(null)

  // One-shot: "edit this message" hands the original text over through the
  // store, and the composer takes ownership of it here and clears the
  // channel, so re-renders can't keep re-applying a stale draft over what
  // the user has since typed.
  useEffect(() => {
    if (!draft) return
    setText(draft)
    setDraft('')
    box.current?.focus()
  }, [draft, setDraft])

  // Opening the project lead from the sidebar puts the cursor here.
  useEffect(() => {
    const focus = () => box.current?.focus()
    window.addEventListener('viv:focus-composer', focus)
    return () => window.removeEventListener('viv:focus-composer', focus)
  }, [])

  // Files dropped on the notch land here as paths.
  useEffect(() => {
    const off = on<string[]>('viv:attach-paths', (paths) => {
      if (!paths?.length) return
      const names = paths.map((p) => `"${p}"`).join(', ')
      setText((v) => `${v ? v + '\n' : ''}About ${names}: `)
      box.current?.focus()
    })
    return () => void off.then((f) => f())
  }, [])

  // ── @ mentions (skills + workspace files) ──
  const [skills, setSkills] = useState<MentionableSkill[]>([])
  const [fileMatches, setFileMatches] = useState<MentionableFile[]>([])
  const [mentionIndex, setMentionIndex] = useState(0)

  // Fetch skills once on mount — the default `~/.viv/skills` dir is always
  // scanned by cmd_list_skills regardless of the extra-paths argument.
  useEffect(() => {
    void call<{ skills?: { name?: string; description?: string }[] }>('cmd_list_skills', { paths: [] }).then((res) => {
      if (res?.skills) setSkills(res.skills.map((s) => ({ name: s.name ?? '', description: s.description ?? '' })))
    })
  }, [])

  const mentionQuery = getSkillMentionQuery(text)
  const mentionActive = mentionQuery !== null
  // File names are needed whenever the prompt might name one (to mark and to
  // complete them), not only inside an @ mention: loaded once the box has
  // something typed, then reused.
  const wantsFiles = mentionActive || text.trim().length > 0

  // The (unfiltered) tree, fetched once per set of workspace folders when a
  // mention starts — not per keystroke.
  useEffect(() => {
    if (!wantsFiles || workspaceFolders.length === 0) return
    const key = workspaceFolders.join('\0')
    if (fileTreeCache?.key === key && Date.now() - fileTreeCache.at < FILE_TREE_TTL_MS) {
      setFileMatches(fileTreeCache.files)
      return
    }
    let cancelled = false
    const multi = workspaceFolders.length > 1
    void Promise.all(
      workspaceFolders.map((root) =>
        call<TreeFileNode[]>('cmd_list_dir_tree', { root, maxDepth: TREE_MAX_DEPTH, maxEntries: TREE_MAX_ENTRIES })
          .then((tree) => ({ root, tree }))
          // One unreadable folder shouldn't take the others' files down.
          .catch(() => ({ root, tree: null })),
      ),
    ).then((results) => {
      if (cancelled) return
      const flat: MentionableFile[] = []
      for (const { root, tree } of results) {
        if (!tree) continue
        flattenTree(tree, '', multi ? basename(root) : '', multi, flat)
      }
      fileTreeCache = { key, files: flat, at: Date.now() }
      setFileMatches(flat)
    })
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [wantsFiles, workspaceFolders])

  // What the prompt can name: files, by name and by path.
  const knownFiles = useMemo(
    () => fileMatches.filter((f) => !f.isDir).map((f) => ({ name: f.name, path: f.insertPath })),
    [fileMatches],
  )

  const mentionItems: MentionItem[] = useMemo(() => {
    if (mentionQuery === null) return []
    const q = mentionQuery.toLowerCase()
    const skillItems: MentionItem[] = skills
      .filter((s) => s.name.toLowerCase().includes(q))
      .slice(0, 5)
      .map((skill) => ({ kind: 'skill', skill }))
    const fileItems: MentionItem[] = fileMatches
      .filter((f) => f.relPath.toLowerCase().includes(q) || f.name.toLowerCase().includes(q))
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
    box.current?.focus()
  }

  // `sendMessage` queues when busy: one path for idle-send and send-while-
  // busy. `ensureToday` first, so a chat left open past midnight rolls over.
  const submit = (typed: string, a: ComposerAttachments) => {
    const toSend = typed + formatFilesForPrompt(a.files)
    void useDaysStore
      .getState()
      .ensureToday()
      .finally(() => void sendMessage(toSend, a.images))
  }

  const onMentionKey = (e: KeyboardEvent) => {
    if (!mentionOpen) return false
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault()
      const n = mentionItems.length
      setMentionIndex((i) => (e.key === 'ArrowDown' ? (i + 1) % n : (i - 1 + n) % n))
      return true
    }
    if (e.key === 'Enter' || e.key === 'Tab') {
      e.preventDefault()
      applyMention(mentionItems[mentionIndex])
      return true
    }
    if (e.key === 'Escape') {
      e.preventDefault()
      setText((v) => v.replace(/@[A-Za-z0-9_./\\-]*$/, ''))
      return true
    }
    return false
  }

  const rootRef = useRef<HTMLDivElement | null>(null)
  useNarrowFallback(rootRef, 560)

  return (
    // Floats with a gap on every side, capped to the transcript's width so
    // its edges line up with the messages above.
    <Box
      ref={rootRef}
      sx={{
        flexShrink: 0,
        pb: 2,
        pt: 0.5,
        position: 'relative',
        maxWidth: 880,
        mx: 'auto',
        width: '100%',
        // The composer measures itself, not the window: the chat is a panel
        // that can be docked anywhere from a full column to a sidebar.
        containerType: 'inline-size',
        containerName: 'composer',
        px: 2,
        '& .cq-compact': { display: 'none' },
        '@container composer (max-width: 560px)': NARROW,
        '&[data-narrow="true"]': NARROW,
      }}
    >
      <Stack sx={{ gap: 0.5 }}>
        <ComputerUseBanner />
        <SubagentPanel />
        <StreamStatusLine />
        <QueuePanel />
        <ChatComposer
          ref={box}
          value={text}
          onChange={setText}
          onSubmit={submit}
          onSteer={(typed, a) => void steer(typed + formatFilesForPrompt(a.files))}
          busy={isBusy}
          stopping={stopping}
          onStop={() => void abort()}
          onKeyDownFirst={onMentionKey}
          knownFiles={knownFiles}
          placeholder={isBusy ? 'Agent is working — send to queue, or steer to redirect now...' : planMode ? 'Describe what you want built — the agent will research it and propose a plan' : 'Do anything'}
          overlay={mentionOpen && <UnifiedMentionMenu items={mentionItems} activeIndex={mentionIndex} onSelect={applyMention} onHover={setMentionIndex} />}
          top={<RunStatusBar />}
          left={
            <>
              <ModePill />
              <LimitsPill />
            </>
          }
          right={
            <>
              <ContextUsageBar />
              <ThinkingLevelControl />
              <ChatModelPicker />
            </>
          }
        />
      </Stack>
    </Box>
  )
}
