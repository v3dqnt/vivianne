import { useEffect, useMemo, useState, type ReactNode } from 'react'
import { Dialog, Stack, Box, TextField, Button, Typography, Alert, LinearProgress, ButtonBase, ToggleButton, ToggleButtonGroup, alpha, useTheme } from '../../../ui'
import { FolderLinear, TickCircleLinear, CloseLinear } from '../../../icons'
import { call } from '../../../ipc'
import { isWindows } from '../../../lib/platform'
import { useProjectsStore, type Project } from '../store/projectsStore'
import { useMasterStore } from '../store/masterStore'
import { useDaysStore } from '../store/daysStore'
import { useAgentStore } from '../../ai/store/agentStore'
import { Choices, ListInput, Question } from '../onboarding/fields'
import {
  AUDIENCES,
  AUTONOMY,
  CHECK_INS,
  DEADLINES,
  DESIGN,
  EXPLANATIONS,
  GIT,
  KINDS,
  MAX_PRIORITIES,
  PRIORITIES,
  SCALES,
  STAGES,
  STYLES,
  TESTING,
  emptyAnswers,
  featuresFor,
  platformsFor,
  techGroupsFor,
  toBrief,
  toCharter,
  toInstructions,
  type Answers,
  type FollowUp,
} from '../onboarding/model'

/** Mirrors `agent::master::CharterDraft`. */
interface CharterDraft {
  charter: string
  draftedByModel: boolean
  error: string | null
}

/** Mirrors `agent::master::FollowUpQuestion`. */
interface FollowUpQuestion {
  question: string
  options: string[]
  multi: boolean
}

type StepId = 'basics' | 'kind' | 'people' | 'features' | 'tech' | 'quality' | 'style' | 'questions' | 'review'

const STEPS: Array<{ id: StepId; title: string; sub: string }> = [
  { id: 'basics', title: 'Basics', sub: 'Name, folder and the one-line idea' },
  { id: 'kind', title: 'The project', sub: 'What kind of thing you’re building' },
  { id: 'people', title: 'Users & problem', sub: 'Who it’s for and why it should exist' },
  { id: 'features', title: 'Features', sub: 'What the first version must do' },
  { id: 'tech', title: 'Technology', sub: 'Your stack, or leave it to the master' },
  { id: 'quality', title: 'Quality & constraints', sub: 'Priorities, testing, timeline, design' },
  { id: 'style', title: 'Working style', sub: 'How the master should work with you' },
  { id: 'questions', title: 'Follow-up questions', sub: 'What the master still wants to know' },
  { id: 'review', title: 'Review', sub: 'The charter your master will work from' },
]

function lastSegment(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? ''
}

/**
 * Starting a project: an interview, not a form.
 *
 * Nine short steps, each offering options to pick from — because a picked
 * option is faster than typing, and because a list of options surfaces things
 * the user wouldn't think to mention (offline support, who can see what, how
 * it gets deployed). Only the first two steps are required; everything else
 * can be skipped, and a skipped question becomes an open question in the
 * charter for the master to raise, rather than a guess baked in silently.
 *
 * The follow-up step is where the interview earns its keep: the model reads
 * everything answered so far and asks the questions specific to *this*
 * project, which a fixed list never can.
 *
 * With `project` set, briefs a master for a project that already exists.
 */
export function OnboardingDialog({
  open,
  project,
  onClose,
  onMultiplayer,
}: {
  open: boolean
  project: Project | null
  onClose: () => void
  onMultiplayer?: () => void
}) {
  const theme = useTheme()
  const createProject = useProjectsStore((s) => s.create)
  const loadMaster = useMasterStore((s) => s.load)

  const [a, setA] = useState<Answers>(() => emptyAnswers())
  const [step, setStep] = useState<StepId>('basics')
  const [visited, setVisited] = useState<Set<StepId>>(new Set(['basics']))

  const [questionsState, setQuestionsState] = useState<'idle' | 'loading' | 'done' | 'failed'>('idle')
  const [questionsError, setQuestionsError] = useState<string | null>(null)

  const [charter, setCharter] = useState('')
  const [instructions, setInstructions] = useState('')
  const [masterName, setMasterName] = useState('')
  const [drafting, setDrafting] = useState(false)
  const [draftNote, setDraftNote] = useState<string | null>(null)
  const [draftedFrom, setDraftedFrom] = useState<string | null>(null)

  const [creating, setCreating] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!open) return
    const fresh = emptyAnswers(project?.name ?? '')
    if (project?.path) {
      fresh.folderMode = 'existing'
      fresh.existing = project.path
    }
    setA(fresh)
    setStep('basics')
    setVisited(new Set(['basics']))
    setQuestionsState('idle')
    setQuestionsError(null)
    setCharter('')
    setInstructions('')
    setMasterName('')
    setDraftNote(null)
    setDraftedFrom(null)
    setCreating(false)
    setError(null)
  }, [open, project])

  const set = <K extends keyof Answers>(key: K, value: Answers[K]) => setA((prev) => ({ ...prev, [key]: value }))

  const index = STEPS.findIndex((s) => s.id === step)
  const brief = useMemo(() => toBrief(a), [a])

  const folderReady = project ? !!project.path : a.folderMode === 'new' ? !!a.parent.trim() : !!a.existing.trim()
  const stepReady: Record<StepId, boolean> = {
    basics: !!a.name.trim() && folderReady && a.pitch.trim().length >= 10,
    kind: !!a.kind,
    people: true,
    features: true,
    tech: true,
    quality: true,
    style: true,
    questions: questionsState !== 'loading',
    review: !!charter.trim() && !drafting,
  }

  const go = (id: StepId) => {
    setStep(id)
    setVisited((v) => new Set(v).add(id))
    setError(null)
  }

  const loadQuestions = async () => {
    setQuestionsState('loading')
    setQuestionsError(null)
    try {
      const qs = await call<FollowUpQuestion[]>('cmd_onboarding_questions', {
        brief: toBrief(a, { includeFollowUps: false }),
      })
      if (!qs) throw new Error('Vivianne’s backend didn’t respond.')
      set(
        'followUps',
        qs.map((q): FollowUp => ({ question: q.question, options: q.options, multi: q.multi, answer: [], note: '' })),
      )
      setQuestionsState('done')
    } catch (e) {
      setQuestionsError(e instanceof Error ? e.message : String(e))
      setQuestionsState('failed')
    }
  }

  const draft = async () => {
    setDrafting(true)
    setDraftNote(null)
    if (!masterName.trim()) setMasterName(`${a.name.trim()} Lead`)
    setInstructions(toInstructions(a))
    try {
      const result = await call<CharterDraft>('cmd_draft_charter', { name: a.name.trim(), description: brief })
      if (result?.draftedByModel) {
        setCharter(result.charter)
      } else {
        // Built from the structured answers instead — a complete charter, not
        // a blank template, because the interview captured everything a
        // model draft would have worked from.
        setCharter(toCharter(a))
        setDraftNote(
          result?.error
            ? `The model couldn’t draft this (${result.error.replace(/\.$/, '')}), so it was built straight from your answers.`
            : 'Built straight from your answers — the backend didn’t return a draft.',
        )
      }
      setDraftedFrom(brief)
    } catch (e) {
      setCharter(toCharter(a))
      setDraftNote(`Drafting failed (${(e instanceof Error ? e.message : String(e)).replace(/\.$/, '')}), so it was built straight from your answers.`)
      setDraftedFrom(brief)
    } finally {
      setDrafting(false)
    }
  }

  const next = () => {
    const target = STEPS[index + 1]?.id
    if (!target) return
    go(target)
    if (target === 'questions' && questionsState === 'idle') void loadQuestions()
    if (target === 'review' && !charter.trim()) void draft()
  }

  const finish = async (start: boolean) => {
    setCreating(true)
    setError(null)
    try {
      let root: string
      if (project) {
        root = project.path as string
      } else {
        root =
          a.folderMode === 'new'
            ? ((await call<string>('cmd_create_workspace_folder', { parent: a.parent.trim(), name: a.name.trim() })) ?? '')
            : a.existing.trim()
      }
      if (!root) throw new Error('No project folder.')
      // Charter before project: if the folder already holds a charter this
      // fails, and nothing half-made is left in the project list.
      await call('cmd_create_master', { root, name: masterName.trim(), charter, instructions })
      if (!project) {
        await createProject(a.name.trim(), root, null)
      } else if (useProjectsStore.getState().activeProjectId !== project.id) {
        useProjectsStore.getState().setActiveProject(project.id)
      }
      await loadMaster(root)
      onClose()
      if (start) {
        await useDaysStore.getState().ensureToday()
        const kickoff = await call<string>('cmd_master_kickoff_prompt')
        if (kickoff) void useAgentStore.getState().sendMessage(kickoff)
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setCreating(false)
    }
  }

  const answeredCount = (id: StepId): boolean => {
    switch (id) {
      case 'basics':
        return stepReady.basics
      case 'kind':
        return !!a.kind
      case 'people':
        return a.audience.length > 0 || !!a.problem.trim()
      case 'features':
        return a.features.length + a.mustHave.length > 0
      case 'tech':
        return a.stackChoice === 'agent' || Object.values(a.tech).some((v) => v.length)
      case 'quality':
        return a.priorities.length > 0 || !!a.testing || !!a.deadline
      case 'style':
        return !!a.autonomy
      case 'questions':
        return a.followUps.some((f) => f.answer.length || f.note.trim())
      case 'review':
        return !!charter.trim()
    }
  }

  const content: Record<StepId, ReactNode> = {
    basics: (
      <>
        {!project && (
          <Question label="Project name">
            <TextField id="ob-name" size="small" value={a.name} onChange={(e) => set('name', e.target.value)} autoFocus fullWidth />
          </Question>
        )}
        {!project ? (
          <Question label="Where should it live?">
            <ToggleButtonGroup
              size="small"
              exclusive
              value={a.folderMode}
              onChange={(_, v: Answers['folderMode'] | null) => v && set('folderMode', v)}
              sx={{ '& .ui-ToggleButton': { textTransform: 'none', px: 1.5, py: 0.375 } }}
            >
              <ToggleButton value="new">Create a new folder</ToggleButton>
              <ToggleButton value="existing">Use an existing folder</ToggleButton>
            </ToggleButtonGroup>
            <Stack direction="row" sx={{ gap: 1, alignItems: 'center' }}>
              <TextField
                id="ob-folder"
                size="small"
                fullWidth
                placeholder={isWindows ? String.raw`C:\Users\you\Work` : '/Users/you/Work'}
                label={a.folderMode === 'new' ? 'Create it inside' : 'Folder'}
                value={a.folderMode === 'new' ? a.parent : a.existing}
                onChange={(e) => set(a.folderMode === 'new' ? 'parent' : 'existing', e.target.value)}
              />
              <Button
                size="small"
                startIcon={<FolderLinear size={13} color="currentColor" />}
                sx={{ flexShrink: 0 }}
                onClick={async () => {
                  const { open: openDialog } = await import('@tauri-apps/plugin-dialog')
                  const picked = await openDialog({ directory: true, multiple: false })
                  if (typeof picked !== 'string') return
                  if (a.folderMode === 'new') set('parent', picked)
                  else {
                    set('existing', picked)
                    if (!a.name.trim()) set('name', lastSegment(picked))
                  }
                }}
              >
                Browse
              </Button>
            </Stack>
            {a.folderMode === 'new' && a.parent.trim() && a.name.trim() && (
              <Typography variant="caption" color="text.secondary" sx={{ fontFamily: '"Geist Mono", ui-monospace, monospace', fontSize: '0.6875rem' }}>
                {`${a.parent.replace(/[\\/]+$/, '')}${isWindows ? '\\' : '/'}${a.name.trim()}`}
              </Typography>
            )}
          </Question>
        ) : !project.path ? (
          <Alert severity="warning">This project has no folder. Set one with Edit project first — the charter is saved inside it.</Alert>
        ) : null}
        <Question label="In one sentence, what is it?" hint="The elevator pitch. The rest of the interview fills in the detail.">
          <TextField
            id="ob-pitch"
            size="small"
            fullWidth
            multiline
            minRows={2}
            value={a.pitch}
            autoFocus={!!project}
            onChange={(e) => set('pitch', e.target.value)}
            placeholder="A recipe app that imports recipes from any site and turns a week of meals into one shopping list."
          />
        </Question>
      </>
    ),

    kind: (
      <>
        <Question label="What kind of project is it?">
          <Choices options={KINDS} value={a.kind} onChange={(v) => setA((p) => ({ ...p, kind: v, platforms: [], tech: {} }))} />
        </Question>
        {platformsFor(a.kind).length > 0 && (
          <Question label="Which platforms?">
            <Choices multi options={platformsFor(a.kind)} value={a.platforms} onChange={(v) => set('platforms', v)} />
          </Question>
        )}
        <Question label="Where does it stand today?">
          <Choices options={STAGES} value={a.stage} onChange={(v) => set('stage', v)} />
        </Question>
      </>
    ),

    people: (
      <>
        <Question label="Who is it for?" hint="Pick every group that applies.">
          <Choices multi options={AUDIENCES} value={a.audience} onChange={(v) => set('audience', v)} />
        </Question>
        <Question label="What problem does it solve for them?" hint="What do they do today without it, and what’s painful about that?">
          <TextField id="ob-problem" size="small" fullWidth multiline minRows={3} value={a.problem} onChange={(e) => set('problem', e.target.value)} />
        </Question>
        <Question label="How many people will use it?">
          <Choices options={SCALES} value={a.scale} onChange={(v) => set('scale', v)} />
        </Question>
      </>
    ),

    features: (
      <>
        <Question label="Which of these does the first version need?" hint={`Suggested for a ${a.kind?.toLowerCase() ?? 'project like this'}.`}>
          <Choices multi options={featuresFor(a.kind)} value={a.features} onChange={(v) => set('features', v)} />
        </Question>
        <Question label="Anything else it must do?" hint="The features specific to your idea. One per line — press Enter to add.">
          <ListInput id="ob-musthave" value={a.mustHave} onChange={(v) => set('mustHave', v)} placeholder="e.g. Scale a recipe up or down by servings" />
        </Question>
        <Question label="Later, not now">
          <ListInput id="ob-later" value={a.later} onChange={(v) => set('later', v)} placeholder="e.g. Share meal plans with others" />
        </Question>
        <Question label="Deliberately not doing" hint="Saying this up front stops the master building it anyway.">
          <ListInput id="ob-out" value={a.outOfScope} onChange={(v) => set('outOfScope', v)} placeholder="e.g. No social feed" />
        </Question>
      </>
    ),

    tech: (
      <>
        <Question label="Do you have a stack in mind?">
          <ToggleButtonGroup
            size="small"
            exclusive
            value={a.stackChoice}
            onChange={(_, v: Answers['stackChoice'] | null) => v && set('stackChoice', v)}
            sx={{ '& .ui-ToggleButton': { textTransform: 'none', px: 1.5, py: 0.375 } }}
          >
            <ToggleButton value="agent">Let the master choose</ToggleButton>
            <ToggleButton value="specify">I have preferences</ToggleButton>
          </ToggleButtonGroup>
        </Question>
        {a.stackChoice === 'specify' &&
          techGroupsFor(a.kind).map((g) =>
            g.multi ? (
              <Question key={g.id} label={g.label}>
                <Choices multi options={g.options} value={a.tech[g.id] ?? []} onChange={(v) => set('tech', { ...a.tech, [g.id]: v })} />
              </Question>
            ) : (
              <Question key={g.id} label={g.label}>
                <Choices
                  options={g.options}
                  value={a.tech[g.id]?.[0] ?? null}
                  onChange={(v) => set('tech', { ...a.tech, [g.id]: v ? [v] : [] })}
                />
              </Question>
            ),
          )}
        {a.stackChoice === 'agent' && (
          <Typography variant="body2" color="text.secondary">
            The master picks a stack that fits everything else you’ve said, and records each choice — with the reason — in the
            charter, where you can overrule it.
          </Typography>
        )}
        <Question label="Services, APIs or accounts it should use?" hint="Anything already decided: an existing API, a database you run, a payments account, a design system.">
          <TextField id="ob-technotes" size="small" fullWidth multiline minRows={2} value={a.techNotes} onChange={(e) => set('techNotes', e.target.value)} />
        </Question>
      </>
    ),

    quality: (
      <>
        <Question label="What matters most?" hint={`Pick up to ${MAX_PRIORITIES}, most important first. The master uses these to settle trade-offs.`}>
          <Choices multi max={MAX_PRIORITIES} options={PRIORITIES} value={a.priorities} onChange={(v) => set('priorities', v)} />
        </Question>
        <Question label="How thoroughly should it be tested?">
          <Choices options={TESTING} value={a.testing} onChange={(v) => set('testing', v)} />
        </Question>
        <Question label="Timeline">
          <Choices options={DEADLINES} value={a.deadline} onChange={(v) => set('deadline', v)} />
        </Question>
        <Question label="Design">
          <Choices options={DESIGN} value={a.design} onChange={(v) => set('design', v)} />
        </Question>
        <Question label="Visual style">
          <Choices multi options={STYLES} value={a.styles} onChange={(v) => set('styles', v)} />
        </Question>
        <Question label="Any other constraints?" hint="Budget, licensing, compliance, must work offline, must run on old hardware…">
          <TextField id="ob-constraints" size="small" fullWidth multiline minRows={2} value={a.constraints} onChange={(e) => set('constraints', e.target.value)} />
        </Question>
      </>
    ),

    style: (
      <>
        <Question label="How much should it decide on its own?">
          <Choices options={AUTONOMY} value={a.autonomy} onChange={(v) => set('autonomy', v)} allowOther={false} />
        </Question>
        <Question label="How often should it check in?">
          <Choices options={CHECK_INS} value={a.checkIns} onChange={(v) => set('checkIns', v)} allowOther={false} />
        </Question>
        <Question label="Git">
          <Choices options={GIT} value={a.git} onChange={(v) => set('git', v)} allowOther={false} />
        </Question>
        <Question label="Explanations">
          <Choices options={EXPLANATIONS} value={a.explanations} onChange={(v) => set('explanations', v)} allowOther={false} />
        </Question>
        <Typography variant="caption" color="text.secondary">
          These become the master’s standing instructions. They’re saved in <code>.viv/agents/master.md</code>, so you can change
          them later.
        </Typography>
      </>
    ),

    questions: (
      <>
        {questionsState === 'loading' && (
          <Stack sx={{ gap: 1, py: 3 }}>
            <Typography variant="body2" color="text.secondary">
              Reading your answers and working out what’s still unclear…
            </Typography>
            <LinearProgress />
          </Stack>
        )}
        {questionsState === 'failed' && (
          <Alert
            severity="info"
            action={
              <Button color="inherit" size="small" onClick={() => void loadQuestions()}>
                Try again
              </Button>
            }
          >
            Couldn’t get follow-up questions ({questionsError?.replace(/\.$/, '')}). You can go on without them — anything unclear becomes an open
            question in the charter.
          </Alert>
        )}
        {questionsState === 'done' && a.followUps.length === 0 && (
          <Typography variant="body2" color="text.secondary">
            No follow-up questions — your answers covered it.
          </Typography>
        )}
        {questionsState === 'done' &&
          a.followUps.map((f, i) => {
            const update = (patch: Partial<FollowUp>) =>
              set(
                'followUps',
                a.followUps.map((x, j) => (j === i ? { ...x, ...patch } : x)),
              )
            return (
              <Question key={i} label={f.question}>
                {f.options.length > 0 &&
                  (f.multi ? (
                    <Choices multi options={f.options} value={f.answer} onChange={(v) => update({ answer: v })} />
                  ) : (
                    <Choices options={f.options} value={f.answer[0] ?? null} onChange={(v) => update({ answer: v ? [v] : [] })} />
                  ))}
                <TextField
                  id={`ob-fu-${i}`}
                  size="small"
                  fullWidth
                  placeholder={f.options.length ? 'Add detail (optional)' : 'Your answer'}
                  value={f.note}
                  onChange={(e) => update({ note: e.target.value })}
                />
              </Question>
            )
          })}
        {questionsState === 'done' && (
          <Stack direction="row" sx={{ gap: 1, alignItems: 'center' }}>
            <Button size="small" onClick={() => void loadQuestions()} sx={{ textTransform: 'none' }}>
              Ask different questions
            </Button>
            <Typography variant="caption" color="text.disabled">
              Unanswered questions become open questions in the charter.
            </Typography>
          </Stack>
        )}
        <Question label="Anything else the master should know?">
          <TextField id="ob-notes" size="small" fullWidth multiline minRows={2} value={a.notes} onChange={(e) => set('notes', e.target.value)} />
        </Question>
      </>
    ),

    review: drafting ? (
      <Stack sx={{ gap: 1, py: 4 }}>
        <Typography variant="body2" color="text.secondary">
          Writing the charter from your answers…
        </Typography>
        <LinearProgress />
      </Stack>
    ) : (
      <>
        {draftNote && <Alert severity="info">{draftNote}</Alert>}
        {draftedFrom !== null && draftedFrom !== brief && (
          <Alert
            severity="warning"
            action={
              <Button color="inherit" size="small" onClick={() => void draft()}>
                Redraft
              </Button>
            }
          >
            You’ve changed answers since this was drafted. Redrafting replaces any edits you made below.
          </Alert>
        )}
        <Question label="Master agent’s name" hint="How it introduces itself. You’re talking to it whenever this project is open.">
          <TextField id="ob-master-name" size="small" fullWidth value={masterName} onChange={(e) => setMasterName(e.target.value)} />
        </Question>
        <Question label="Charter" hint={<>Saved as <code>.viv/charter.md</code>. The master keeps it current as decisions are made.</>}>
          <TextField
            id="ob-charter"
            fullWidth
            multiline
            minRows={14}
            value={charter}
            onChange={(e) => setCharter(e.target.value)}
            slotProps={{ input: { sx: { fontFamily: '"Geist Mono", ui-monospace, monospace', fontSize: '0.78rem', lineHeight: 1.55 } } }}
          />
        </Question>
        <Question label="Standing instructions" hint={<>Saved as <code>.viv/agents/master.md</code> — from the working-style step.</>}>
          <TextField
            id="ob-instructions"
            fullWidth
            multiline
            minRows={4}
            value={instructions}
            onChange={(e) => setInstructions(e.target.value)}
            slotProps={{ input: { sx: { fontSize: '0.8125rem' } } }}
          />
        </Question>
      </>
    ),
  }

  const current = STEPS[index]
  const isLast = step === 'review'

  return (
    <Dialog
      open={open}
      onClose={creating ? undefined : onClose}
      maxWidth={false}
      slotProps={{ paper: { sx: { width: 'min(980px, 94vw)', height: 'min(760px, 90vh)', maxWidth: 'none' } } }}
    >
      <Stack direction={{ xs: 'column', md: 'row' }} sx={{ height: '100%', minHeight: 0 }}>
        <Stack
          component="nav"
          aria-label="Onboarding steps"
          sx={{
            width: { md: 240 },
            flexShrink: 0,
            borderRight: { md: 1 },
            borderBottom: { xs: 1, md: 0 },
            borderColor: 'divider',
            bgcolor: alpha(theme.palette.text.primary, 0.025),
            p: 1.5,
            gap: 0.25,
            overflowY: 'auto',
            maxHeight: { xs: 140, md: 'none' },
          }}
        >
          <Typography sx={{ fontWeight: 700, fontSize: '0.9375rem', px: 1, pt: 0.5, pb: 1.25 }}>
            {project ? `Brief a master for ${project.name}` : 'New project'}
          </Typography>
          {STEPS.map((s, i) => {
            const active = s.id === step
            // Steps can be revisited freely; jumping ahead past a required
            // step can't, since later steps build on the answers.
            const reachable = visited.has(s.id) || i <= index
            // Visited AND answered: some steps start with sensible defaults
            // selected, and a tick on a step never opened would claim the
            // user had made choices they never saw.
            const done = s.id !== step && visited.has(s.id) && answeredCount(s.id)
            return (
              <ButtonBase
                key={s.id}
                disabled={!reachable || creating}
                onClick={() => go(s.id)}
                aria-current={active ? 'step' : undefined}
                sx={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: 1.25,
                  justifyContent: 'flex-start',
                  textAlign: 'left',
                  px: 1,
                  py: 0.75,
                  borderRadius: '8px',
                  bgcolor: active ? alpha(theme.palette.primary.main, 0.13) : 'transparent',
                  opacity: reachable ? 1 : 0.45,
                  '&:hover': { bgcolor: active ? alpha(theme.palette.primary.main, 0.16) : 'action.hover' },
                }}
              >
                <Box
                  sx={{
                    width: 22,
                    height: 22,
                    flexShrink: 0,
                    borderRadius: '50%',
                    display: 'grid',
                    placeItems: 'center',
                    fontSize: '0.6875rem',
                    fontWeight: 700,
                    color: active ? 'primary.contrastText' : done ? 'success.main' : 'text.secondary',
                    bgcolor: active ? 'primary.main' : done ? alpha(theme.palette.success.main, 0.15) : 'action.selected',
                  }}
                >
                  {done ? <TickCircleLinear size={13} color="currentColor" /> : i + 1}
                </Box>
                <Typography sx={{ fontSize: '0.8125rem', fontWeight: active ? 600 : 500, color: active ? 'text.primary' : 'text.secondary' }} noWrap>
                  {s.title}
                </Typography>
              </ButtonBase>
            )
          })}
        </Stack>

        <Stack sx={{ flex: 1, minWidth: 0, minHeight: 0 }}>
          <Stack direction="row" sx={{ alignItems: 'flex-start', px: 3, pt: 2.5, pb: 1.5, gap: 2 }}>
            <Stack sx={{ flex: 1, minWidth: 0 }}>
              <Typography variant="caption" color="text.secondary" sx={{ fontWeight: 600, letterSpacing: 0.4 }}>
                STEP {index + 1} OF {STEPS.length}
              </Typography>
              <Typography sx={{ fontWeight: 700, fontSize: '1.125rem', lineHeight: 1.3 }}>{current.title}</Typography>
              <Typography variant="body2" color="text.secondary">
                {current.sub}
              </Typography>
            </Stack>
            <Box
              component="button"
              type="button"
              aria-label="Close"
              onClick={onClose}
              disabled={creating}
              sx={{ border: 0, bgcolor: 'transparent', color: 'text.secondary', cursor: 'pointer', p: 0.5, borderRadius: '6px', '&:hover': { bgcolor: 'action.hover' } }}
            >
              <CloseLinear size={16} color="currentColor" />
            </Box>
          </Stack>
          <LinearProgress variant="determinate" value={((index + 1) / STEPS.length) * 100} sx={{ height: 2 }} />

          <Stack sx={{ flex: 1, minHeight: 0, overflowY: 'auto', px: 3, py: 2.5, gap: 3 }}>
            {content[step]}
            {error && <Alert severity="error">{error}</Alert>}
          </Stack>

          <Stack direction="row" sx={{ alignItems: 'center', gap: 1, px: 3, py: 1.5, borderTop: 1, borderColor: 'divider' }}>
            {step === 'basics' && !project && onMultiplayer ? (
              <Button color="inherit" size="small" onClick={onMultiplayer} sx={{ textTransform: 'none', color: 'text.secondary' }}>
                Multiplayer project instead
              </Button>
            ) : (
              <Button color="inherit" onClick={() => go(STEPS[index - 1].id)} disabled={index === 0 || creating || drafting}>
                Back
              </Button>
            )}
            <Box sx={{ flex: 1 }} />
            {!['basics', 'kind', 'review'].includes(step) && !answeredCount(step) && (
              <Typography variant="caption" color="text.disabled" sx={{ mr: 1 }}>
                Optional — skip if you’re not sure
              </Typography>
            )}
            {isLast ? (
              <>
                <Button color="inherit" onClick={() => void finish(false)} disabled={!stepReady.review || creating}>
                  {project ? 'Save' : 'Create project'}
                </Button>
                <Button variant="contained" disableElevation onClick={() => void finish(true)} disabled={!stepReady.review || creating}>
                  {creating ? 'Creating…' : project ? 'Save and start' : 'Create and start'}
                </Button>
              </>
            ) : (
              <Button variant="contained" disableElevation onClick={next} disabled={!stepReady[step]}>
                {answeredCount(step) || ['basics', 'kind', 'questions'].includes(step) ? 'Next' : 'Skip'}
              </Button>
            )}
          </Stack>
        </Stack>
      </Stack>
    </Dialog>
  )
}
