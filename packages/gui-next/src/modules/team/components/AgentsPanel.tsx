import { useEffect, useMemo, useState } from 'react'
import { Box, Stack, Typography, Button, ButtonBase, IconButton, InputBase, Select, MenuItem, Checkbox, Tooltip, CircularProgress, alpha } from '../../../ui'
import { AddLinear, ArrowLeftLinear, EditLinear, TrashLinear, AgentsLinear, UserPlusLinear } from '../../../icons'
import { call } from '../../../ipc'
import { useProjectsStore, modeOf, GENERAL_PROJECT_ID } from '../../sessions/store/projectsStore'
import { useGeneralAgents } from '../store/generalAgentsStore'
import { useMasterStore } from '../../sessions/store/masterStore'
import { ListInput } from '../../sessions/onboarding/fields'
import { SUBAGENT_COLORS, SUBAGENT_COLOR_HEX } from '../../settings/lib/subagentProfiles'
import { useTeamStore, blankAgent, type TeamAgent } from '../store/teamStore'
import { Avatar, Tag, SectionLabel, colorOf } from './bits'
import { AgentPage } from './AgentPage'
import { BotPage } from './BotPage'
import { ModelPicker } from './ModelPicker'

interface ToolInfo {
  name: string
  description?: string
}

const EMPTY: TeamAgent[] = []

/** The project's team: its master, the specialists, and hires the master has
 * proposed. Specialists are `.viv/agents/*.md` files — see `agent::team`. */
export function AgentsPanel() {
  const activeProjectId = useProjectsStore((s) => s.activeProjectId)
  const projects = useProjectsStore((s) => s.projects)
  const project = projects.find((p) => p.id === activeProjectId)
  // General's agents have a folder of their own; a project's live in it.
  const isGeneral = activeProjectId === GENERAL_PROJECT_ID
  const generalRoot = useGeneralAgents((s) => s.root)
  const ensureGeneral = useGeneralAgents((s) => s.ensure)
  useEffect(() => {
    if (isGeneral) void ensureGeneral()
  }, [isGeneral, ensureGeneral])
  const root = isGeneral ? generalRoot : (project?.path ?? null)
  // A team project has no master: the agents are peers the user built. So
  // are General's: each one works on its own.
  const teamMode = isGeneral || modeOf(project) === 'team'
  const agents = useTeamStore((s) => (root ? (s.byRoot[root] ?? EMPTY) : EMPTY))
  const loaded = useTeamStore((s) => (root ? root in s.byRoot : false))
  const load = useTeamStore((s) => s.load)
  const master = useMasterStore((s) => (root ? s.byRoot[root] : null))
  const loadMaster = useMasterStore((s) => s.load)
  const [editing, setEditing] = useState<TeamAgent | 'new' | null>(null)
  const focused = useTeamStore((s) => s.focused)
  const focus = useTeamStore((s) => s.focus)
  const runs = useTeamStore((s) => s.runs)

  useEffect(() => {
    if (!root) return
    void load(root)
    void loadMaster(root)
    setEditing(null)
  }, [root, load, loadMaster])

  if (!root) {
    return (
      <Centered title="No project open" body="Agents belong to a project. Open or create one from the rail on the left." />
    )
  }
  const focusedAgent = focused && focused.root === root ? agents.find((a) => a.id === focused.agentId) : undefined
  if (!editing && focusedAgent && focusedAgent.status === 'active') {
    // An agent team's members are bots: their own chat, routines and rules.
    // In a company the master hands out the tasks, so the page shows runs.
    return teamMode ? (
      <BotPage key={focusedAgent.id} root={root} agent={focusedAgent} onBack={() => focus(root, null)} onEdit={() => setEditing(focusedAgent)} />
    ) : (
      <AgentPage root={root} agent={focusedAgent} onBack={() => focus(root, null)} onEdit={() => setEditing(focusedAgent)} />
    )
  }
  if (editing) {
    return (
      <AgentForm
        root={root}
        agent={editing === 'new' ? null : editing}
        team={agents}
        masterName={master?.name ?? 'Master'}
        teamMode={teamMode}
        onClose={() => setEditing(null)}
      />
    )
  }

  const proposed = agents.filter((a) => a.status === 'proposed')
  const active = agents.filter((a) => a.status === 'active')

  return (
    <Box sx={{ height: '100%', overflow: 'auto' }}>
      <Stack sx={{ p: 1.5, gap: 2 }}>
        <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
          <Stack sx={{ flex: 1, minWidth: 0 }}>
            <Typography variant="body2" sx={{ fontWeight: 600 }}>
              Agents
            </Typography>
            <Typography variant="caption" color="text.secondary" sx={{ fontSize: '0.6875rem' }}>
              {teamMode
                ? 'Your team. Give it a task in the chat — they work on it together. Saved in .viv/agents.'
                : 'Specialists the master delegates to. Saved in .viv/agents.'}
            </Typography>
          </Stack>
          <Button size="small" variant="contained" disableElevation startIcon={<AddLinear size={14} color="currentColor" />} onClick={() => setEditing('new')} sx={{ borderRadius: '8px', flexShrink: 0 }}>
            New agent
          </Button>
        </Stack>

        {!teamMode && (
        <Stack sx={{ gap: 0.75 }}>
          <SectionLabel>Master</SectionLabel>
          {master ? (
            <Row>
              <Avatar name={master.name} color={SUBAGENT_COLOR_HEX.blue} />
              <Stack sx={{ minWidth: 0, flex: 1 }}>
                <Typography variant="body2" sx={{ fontWeight: 600, fontSize: '0.8125rem' }} noWrap>
                  {master.name}
                </Typography>
                <Typography variant="caption" color="text.secondary" sx={{ fontSize: '0.6875rem' }}>
                  Runs the project · every specialist reports to it
                </Typography>
              </Stack>
            </Row>
          ) : (
            <Typography variant="caption" color="text.secondary" sx={{ px: 0.5 }}>
              No master yet. Brief one from the sidebar so it can plan the work and propose hires. You can still create agents
              yourself; the chat can delegate to them with the task tool.
            </Typography>
          )}
        </Stack>
        )}

        {proposed.length > 0 && (
          <Stack sx={{ gap: 0.75 }}>
            <SectionLabel>Waiting for your approval · {proposed.length}</SectionLabel>
            {proposed.map((a) => (
              <ProposalCard key={a.id} root={root} agent={a} onReview={() => setEditing(a)} />
            ))}
          </Stack>
        )}

        <Stack sx={{ gap: 0.75 }}>
          <SectionLabel>Team{active.length ? ` · ${active.length}` : ''}</SectionLabel>
          {!loaded ? (
            <CircularProgress size={16} sx={{ alignSelf: 'center', my: 2 }} />
          ) : active.length === 0 ? (
            <Stack sx={{ alignItems: 'center', textAlign: 'center', gap: 1, py: 3, px: 2, border: 1, borderColor: 'divider', borderStyle: 'dashed', borderRadius: '10px' }}>
              <AgentsLinear size={22} color="currentColor" />
              <Typography variant="body2" sx={{ fontWeight: 600, fontSize: '0.8125rem' }}>
                {teamMode ? 'No agents yet' : 'No specialists yet'}
              </Typography>
              <Typography variant="caption" color="text.secondary" sx={{ maxWidth: 300 }}>
                {teamMode
                  ? 'Create each member of your team: what it’s responsible for, its instructions, its model and its tools.'
                  : 'Create one for an area of the project — frontend, API, tests — or ask the master to propose the team it needs.'}
              </Typography>
              <Button size="small" startIcon={<UserPlusLinear size={14} color="currentColor" />} onClick={() => setEditing('new')} sx={{ borderRadius: '8px' }}>
                Create an agent
              </Button>
            </Stack>
          ) : (
            active.map((a) => (
              <AgentRow
                key={a.id}
                root={root}
                agent={a}
                team={agents}
                masterName={master?.name ?? 'Master'}
                working={(runs[`${root}::${a.id}`] ?? []).some((r) => r.status === 'running')}
                onOpen={() => focus(root, a.id)}
                onEdit={() => setEditing(a)}
              />
            ))
          )}
        </Stack>
      </Stack>
    </Box>
  )
}

function Centered({ title, body }: { title: string; body: string }) {
  return (
    <Stack sx={{ height: '100%', alignItems: 'center', justifyContent: 'center', textAlign: 'center', gap: 0.75, p: 3 }}>
      <AgentsLinear size={22} color="currentColor" />
      <Typography variant="body2" sx={{ fontWeight: 600 }}>
        {title}
      </Typography>
      <Typography variant="caption" color="text.secondary" sx={{ maxWidth: 280 }}>
        {body}
      </Typography>
    </Stack>
  )
}

function Row({ children, onClick }: { children: React.ReactNode; onClick?: () => void }) {
  return (
    <Stack
      direction="row"
      onClick={onClick}
      sx={{
        alignItems: 'center',
        gap: 1,
        px: 1,
        py: 0.75,
        borderRadius: '8px',
        cursor: onClick ? 'pointer' : 'default',
        '&:hover': onClick ? { bgcolor: 'action.hover' } : undefined,
        '&:hover .row-actions': { opacity: 1 },
      }}
    >
      {children}
    </Stack>
  )
}

function reportsLabel(agent: TeamAgent, team: TeamAgent[], masterName: string): string {
  if (!agent.reportsTo || agent.reportsTo === 'master') return masterName
  return team.find((t) => t.id === agent.reportsTo)?.name ?? agent.reportsTo
}

function AgentRow({
  root,
  agent,
  team,
  masterName,
  working,
  onOpen,
  onEdit,
}: {
  root: string
  agent: TeamAgent
  team: TeamAgent[]
  masterName: string
  working: boolean
  onOpen: () => void
  onEdit: () => void
}) {
  const remove = useTeamStore((s) => s.remove)
  const [confirming, setConfirming] = useState(false)
  return (
    <Row onClick={onOpen}>
      <Avatar name={agent.name} color={colorOf(agent)} />
      <Stack sx={{ minWidth: 0, flex: 1, gap: 0.25 }}>
        <Stack direction="row" sx={{ alignItems: 'center', gap: 0.75, minWidth: 0 }}>
          <Typography variant="body2" sx={{ fontWeight: 600, fontSize: '0.8125rem' }} noWrap>
            {agent.name}
          </Typography>
          {working && (
            <Typography component="span" sx={{ fontSize: '0.6875rem', fontWeight: 700, color: 'primary.main', flexShrink: 0 }}>
              ● working
            </Typography>
          )}
        </Stack>
        {agent.role && (
          <Typography variant="caption" color="text.secondary" sx={{ fontSize: '0.6875rem' }} noWrap>
            {agent.role}
          </Typography>
        )}
        <Stack direction="row" sx={{ gap: 0.5, flexWrap: 'wrap' }}>
          <Tag>reports to {reportsLabel(agent, team, masterName)}</Tag>
          {agent.owns.slice(0, 3).map((p) => (
            <Tag key={p}>{p}</Tag>
          ))}
          {agent.owns.length > 3 && <Tag>+{agent.owns.length - 3}</Tag>}
          {agent.model && <Tag>{agent.model.name}</Tag>}
          {agent.allowedTools && <Tag>{agent.allowedTools.length} tools</Tag>}
        </Stack>
      </Stack>
      <Stack direction="row" className="row-actions" sx={{ opacity: confirming ? 1 : 0, transition: 'opacity 0.12s', flexShrink: 0 }} onClick={(e) => e.stopPropagation()}>
        {confirming ? (
          <>
            <Button size="small" color="error" onClick={() => void remove(root, agent.id)} sx={{ minWidth: 0, borderRadius: '6px' }}>
              Delete
            </Button>
            <Button size="small" color="inherit" onClick={() => setConfirming(false)} sx={{ minWidth: 0, borderRadius: '6px' }}>
              Keep
            </Button>
          </>
        ) : (
          <>
            <Tooltip title="Edit">
              <IconButton size="small" onClick={onEdit}>
                <EditLinear size={14} color="currentColor" />
              </IconButton>
            </Tooltip>
            <Tooltip title="Delete">
              <IconButton size="small" onClick={() => setConfirming(true)}>
                <TrashLinear size={14} color="currentColor" />
              </IconButton>
            </Tooltip>
          </>
        )}
      </Stack>
    </Row>
  )
}

function ProposalCard({ root, agent, onReview }: { root: string; agent: TeamAgent; onReview: () => void }) {
  const approve = useTeamStore((s) => s.approve)
  const remove = useTeamStore((s) => s.remove)
  const [busy, setBusy] = useState<'approve' | 'decline' | null>(null)
  const act = async (kind: 'approve' | 'decline') => {
    setBusy(kind)
    try {
      if (kind === 'approve') await approve(root, agent.id)
      else await remove(root, agent.id)
    } finally {
      setBusy(null)
    }
  }
  return (
    <Stack sx={{ gap: 1, p: 1.25, borderRadius: '10px', border: 1, borderColor: (t) => alpha(t.palette.primary.main, 0.35), bgcolor: (t) => alpha(t.palette.primary.main, 0.05) }}>
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
        <Avatar name={agent.name} color={colorOf(agent)} />
        <Stack sx={{ minWidth: 0, flex: 1 }}>
          <Typography variant="body2" sx={{ fontWeight: 600, fontSize: '0.8125rem' }} noWrap>
            {agent.name}
          </Typography>
          <Typography variant="caption" color="text.secondary" sx={{ fontSize: '0.6875rem' }}>
            Proposed by {agent.proposedBy === 'master' ? 'the master' : 'you'}
          </Typography>
        </Stack>
      </Stack>
      {agent.role && (
        <Typography variant="body2" sx={{ fontSize: '0.8125rem' }}>
          {agent.role}
        </Typography>
      )}
      {agent.reason && (
        <Typography variant="caption" color="text.secondary">
          Why: {agent.reason}
        </Typography>
      )}
      {agent.owns.length > 0 && (
        <Stack direction="row" sx={{ gap: 0.5, flexWrap: 'wrap' }}>
          {agent.owns.map((p) => (
            <Tag key={p}>{p}</Tag>
          ))}
        </Stack>
      )}
      <Stack direction="row" sx={{ gap: 0.75 }}>
        <Button size="small" variant="contained" disableElevation disabled={!!busy} onClick={() => void act('approve')} sx={{ borderRadius: '8px' }}>
          {busy === 'approve' ? <CircularProgress size={14} /> : 'Approve'}
        </Button>
        <Button size="small" color="inherit" disabled={!!busy} onClick={onReview} sx={{ borderRadius: '8px' }}>
          Review and edit
        </Button>
        <Box sx={{ flex: 1 }} />
        <Button size="small" color="inherit" disabled={!!busy} onClick={() => void act('decline')} sx={{ borderRadius: '8px', color: 'text.secondary' }}>
          Decline
        </Button>
      </Stack>
    </Stack>
  )
}

// ── The form ─────────────────────────────────────────────────────────────

function Field({ label, hint, htmlFor, children }: { label: string; hint?: string; htmlFor?: string; children: React.ReactNode }) {
  return (
    <Stack sx={{ gap: 0.5 }}>
      <Stack sx={{ gap: 0.125 }}>
        <Typography component="label" htmlFor={htmlFor} variant="body2" sx={{ fontWeight: 600, fontSize: '0.8125rem' }}>
          {label}
        </Typography>
        {hint && (
          <Typography variant="caption" color="text.secondary" sx={{ fontSize: '0.6875rem' }}>
            {hint}
          </Typography>
        )}
      </Stack>
      {children}
    </Stack>
  )
}

const inputSx = { fontSize: '0.8125rem', px: 1.25, py: 0.75, border: 1, borderColor: 'divider', borderRadius: '8px', '&.ui-focused': { borderColor: 'primary.main' } } as const
const selectSx = { fontSize: '0.8125rem', borderRadius: '8px', '& .ui-Select-select': { py: 0.875 } } as const

function AgentForm({
  root,
  agent,
  team,
  masterName,
  teamMode = false,
  onClose,
}: {
  root: string
  agent: TeamAgent | null
  team: TeamAgent[]
  masterName: string
  teamMode?: boolean
  onClose: () => void
}) {
  const save = useTeamStore((s) => s.save)
  const [allTools, setAllTools] = useState<ToolInfo[]>([])
  const [toolQuery, setToolQuery] = useState('')
  const base = agent ?? blankAgent()
  const [name, setName] = useState(base.name)
  const [role, setRole] = useState(base.role)
  const [color, setColor] = useState<string>(base.color ?? SUBAGENT_COLORS[team.length % SUBAGENT_COLORS.length])
  const [reportsTo, setReportsTo] = useState(base.reportsTo ?? 'master')
  const [model, setModel] = useState(base.model ?? null)
  const [owns, setOwns] = useState<string[]>(base.owns)
  const [toolMode, setToolMode] = useState<'all' | 'custom'>(base.allowedTools ? 'custom' : 'all')
  const [tools, setTools] = useState<Set<string>>(new Set(base.allowedTools ?? []))
  const [instructions, setInstructions] = useState(base.instructions)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const isProposal = agent?.status === 'proposed'

  useEffect(() => {
    void call<ToolInfo[]>('cmd_list_tool_names').then((t) => setAllTools(t ?? []))
  }, [])

  const shownTools = useMemo(() => {
    const q = toolQuery.trim().toLowerCase()
    return q ? allTools.filter((t) => t.name.toLowerCase().includes(q)) : allTools
  }, [allTools, toolQuery])

  // An agent can't report to itself; everyone else active is a candidate.
  const managers = team.filter((t) => t.status === 'active' && t.id !== agent?.id)

  const submit = async (status: 'active' | 'proposed') => {
    if (!name.trim()) return
    setSaving(true)
    setError(null)
    try {
      await save(
        root,
        {
          ...base,
          name: name.trim(),
          role: role.trim(),
          color,
          status,
          reportsTo: teamMode || reportsTo === 'master' ? null : reportsTo,
          model,
          allowedTools: toolMode === 'all' ? null : Array.from(tools),
          owns,
          instructions,
        },
        agent?.id ?? null,
      )
      onClose()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setSaving(false)
    }
  }

  return (
    <Box sx={{ height: '100%', overflow: 'auto' }}>
      <Stack sx={{ p: 1.5, gap: 2, maxWidth: 620 }}>
        <ButtonBase onClick={onClose} sx={{ alignSelf: 'flex-start', gap: 0.5, color: 'text.secondary', fontSize: '0.75rem', borderRadius: '6px', px: 0.5 }}>
          <ArrowLeftLinear size={13} color="currentColor" />
          All agents
        </ButtonBase>

        <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
          <Avatar name={name || '?'} color={colorOf({ color })} size={30} />
          <Stack>
            <Typography variant="body2" sx={{ fontWeight: 600 }}>
              {agent ? (isProposal ? `Review proposed hire: ${agent.name}` : `Edit ${agent.name}`) : 'New agent'}
            </Typography>
            <Typography variant="caption" color="text.secondary" sx={{ fontSize: '0.6875rem' }}>
              {isProposal && agent?.reason
                ? `Why the master wants it: ${agent.reason}`
                : teamMode
                  ? 'A member of your team. It works on every task the team gets, alongside the others.'
                  : 'A specialist the master can hand work to.'}
            </Typography>
          </Stack>
        </Stack>

        <Field label="Name" htmlFor="agent-name">
          <InputBase id="agent-name" autoFocus={!agent} value={name} onChange={(e) => setName(e.target.value)} placeholder="Frontend Engineer" sx={inputSx} />
        </Field>

        <Field
          label="Role"
          hint={teamMode ? 'One sentence: what it is responsible for. Its teammates see this.' : 'One sentence: what it is responsible for. The master reads this when deciding who gets a task.'}
          htmlFor="agent-role"
        >
          <InputBase id="agent-role" value={role} onChange={(e) => setRole(e.target.value)} placeholder="Builds and maintains the React UI" sx={inputSx} />
        </Field>

        <Stack direction="row" sx={{ gap: 1.5, flexWrap: 'wrap' }}>
          {!teamMode && (
          <Box sx={{ flex: '1 1 180px' }}>
            <Field label="Reports to">
              <Select size="small" value={reportsTo} onChange={(e) => setReportsTo(e.target.value)} sx={selectSx}>
                <MenuItem value="master" sx={{ fontSize: '0.8125rem' }}>
                  {masterName} (master)
                </MenuItem>
                {managers.map((m) => (
                  <MenuItem key={m.id} value={m.id} sx={{ fontSize: '0.8125rem' }}>
                    {m.name}
                  </MenuItem>
                ))}
              </Select>
            </Field>
          </Box>
          )}
          <Box sx={{ flex: '1 1 180px' }}>
            <Field label="Model">
              <ModelPicker value={model} onChange={setModel} fullWidth />
            </Field>
          </Box>
        </Stack>

        <Field label="Colour">
          <Stack direction="row" sx={{ gap: 0.75 }}>
            {SUBAGENT_COLORS.map((c) => (
              <ButtonBase
                key={c}
                aria-label={c}
                onClick={() => setColor(c)}
                sx={{
                  width: 20,
                  height: 20,
                  borderRadius: '50%',
                  bgcolor: SUBAGENT_COLOR_HEX[c],
                  outline: color === c ? '2px solid' : 'none',
                  outlineColor: 'text.primary',
                  outlineOffset: 2,
                }}
              />
            ))}
          </Stack>
        </Field>

        <Field label="Owned paths" hint="Folders or files it is responsible for, relative to the project. It edits inside these and reports anything outside them back to the master.">
          <ListInput id="agent-owns" value={owns} onChange={setOwns} placeholder="src/components/ — press Enter to add" />
        </Field>

        <Field label="Tools">
          <Select size="small" value={toolMode} onChange={(e) => setToolMode(e.target.value as 'all' | 'custom')} sx={{ ...selectSx, alignSelf: 'flex-start', minWidth: 220 }}>
            <MenuItem value="all" sx={{ fontSize: '0.8125rem' }}>
              Every tool
            </MenuItem>
            <MenuItem value="custom" sx={{ fontSize: '0.8125rem' }}>
              Only the tools I pick
            </MenuItem>
          </Select>
          {toolMode === 'custom' && (
            <Stack sx={{ gap: 0.75, p: 1, border: 1, borderColor: 'divider', borderRadius: '8px' }}>
              <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
                <InputBase value={toolQuery} onChange={(e) => setToolQuery(e.target.value)} placeholder="Filter tools" sx={{ ...inputSx, flex: 1, py: 0.5 }} />
                <Typography variant="caption" color="text.secondary" sx={{ flexShrink: 0 }}>
                  {tools.size} selected
                </Typography>
              </Stack>
              <Box sx={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(150px, 1fr))', maxHeight: 220, overflow: 'auto' }}>
                {shownTools.map((t) => (
                  <Tooltip key={t.name} title={t.description ?? ''} placement="top" enterDelay={500}>
                    <Stack
                      direction="row"
                      onClick={() =>
                        setTools((prev) => {
                          const next = new Set(prev)
                          if (next.has(t.name)) next.delete(t.name)
                          else next.add(t.name)
                          return next
                        })
                      }
                      sx={{ alignItems: 'center', cursor: 'pointer', borderRadius: '6px', pr: 0.5, '&:hover': { bgcolor: 'action.hover' } }}
                    >
                      <Checkbox size="small" checked={tools.has(t.name)} sx={{ p: 0.5 }} />
                      <Typography variant="body2" noWrap sx={{ fontSize: '0.75rem' }}>
                        {t.name}
                      </Typography>
                    </Stack>
                  </Tooltip>
                ))}
              </Box>
            </Stack>
          )}
        </Field>

        <Field
          label={teamMode ? 'Prompt' : 'Instructions'}
          hint={
            teamMode
              ? 'Its standing prompt: how it thinks, what it does on every task, how it checks its own work.'
              : 'Standing instructions it gets with every task: conventions, the decisions that apply to its area, how to check its own work.'
          }
          htmlFor="agent-instructions"
        >
          <InputBase
            id="agent-instructions"
            multiline
            minRows={6}
            value={instructions}
            onChange={(e) => setInstructions(e.target.value)}
            placeholder={'Use the components in src/ui before writing new ones.\nRun `npm test` and `npm run lint` before reporting back.'}
            sx={{ ...inputSx, alignItems: 'flex-start', lineHeight: 1.5 }}
          />
        </Field>

        {error && (
          <Typography variant="caption" color="error.main">
            {error}
          </Typography>
        )}

        <Stack direction="row" sx={{ gap: 1, pb: 1 }}>
          <Button
            variant="contained"
            disableElevation
            disabled={saving || !name.trim()}
            onClick={() => void submit('active')}
            sx={{ borderRadius: '8px' }}
          >
            {saving ? <CircularProgress size={14} /> : isProposal ? 'Approve and save' : agent ? 'Save' : 'Create agent'}
          </Button>
          {isProposal && (
            <Button color="inherit" disabled={saving || !name.trim()} onClick={() => void submit('proposed')} sx={{ borderRadius: '8px' }}>
              Save, decide later
            </Button>
          )}
          <Button color="inherit" onClick={onClose} sx={{ borderRadius: '8px', color: 'text.secondary' }}>
            Cancel
          </Button>
        </Stack>
      </Stack>
    </Box>
  )
}
