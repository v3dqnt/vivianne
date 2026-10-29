import { useState } from 'react'
import { Box, ButtonBase, CircularProgress, Dialog, InputBase, Stack, Typography, alpha, useTheme } from '../../../ui'
import { SUBAGENT_COLORS } from '../../settings/lib/subagentProfiles'
import { useTeamStore, blankAgent, type TeamAgent } from '../store/teamStore'
import type { FaceStyle } from '../store/faceStore'
import { ModelPicker } from '../../ai/components/ModelPicker'
import { FaceControls, FaceStage, randomLook, shellHex } from './FaceEditor'

/** One field style for the whole form: a soft well, ringed in the agent's
 * colour while you type in it. */
const fieldSx = (ring: string) =>
  ({
    width: '100%',
    fontSize: '14px',
    px: '12px',
    py: '10px',
    borderRadius: '10px',
    bgcolor: 'action.hover',
    boxShadow: 'inset 0 0 0 1px transparent',
    transition: 'box-shadow 0.12s, background-color 0.12s',
    '&.ui-focused': { bgcolor: 'background.paper', boxShadow: `inset 0 0 0 1.5px ${ring}` },
  }) as const

function Field({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <Stack sx={{ gap: '6px' }}>
      <Stack direction="row" sx={{ alignItems: 'baseline', gap: '8px' }}>
        <Typography sx={{ fontSize: '12px', fontWeight: 500, color: 'text.secondary' }}>{label}</Typography>
        {hint && <Typography sx={{ fontSize: '12px', color: 'text.disabled' }}>{hint}</Typography>}
      </Stack>
      {children}
    </Stack>
  )
}

/** Make a 24/7 agent, or edit one: who it is and what it does on the right,
 * and how it looks, live, on the left. Schedules, inbox and phone are set on
 * its page once it exists. */
export function NewAgentDialog({
  root,
  open,
  count = 0,
  agent = null,
  title = 'New 24/7 agent',
  onClose,
  onCreated,
}: {
  root: string
  open: boolean
  count?: number
  /** Given, the dialog edits this agent instead of making a new one. */
  agent?: TeamAgent | null
  title?: string
  onClose: () => void
  onCreated?: (agent: TeamAgent) => void
}) {
  const theme = useTheme()
  const save = useTeamStore((s) => s.save)
  const [name, setName] = useState(agent?.name ?? '')
  const [role, setRole] = useState(agent?.role ?? '')
  const [color, setColor] = useState<string>(agent?.color ?? SUBAGENT_COLORS[count % SUBAGENT_COLORS.length])
  const [face, setFace] = useState<FaceStyle | null>(agent?.face ?? null)
  const [model, setModel] = useState<TeamAgent['model']>(agent?.model ?? null)
  const [instructions, setInstructions] = useState(agent?.instructions ?? '')
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const hex = shellHex(color)
  const onHex = theme.palette.getContrastText(hex)

  const look = (next: { color: string; face: FaceStyle | null }) => {
    setColor(next.color)
    setFace(next.face)
  }

  const create = async () => {
    if (!name.trim()) return
    setSaving(true)
    setError(null)
    try {
      const made = agent
        ? await save(root, { ...agent, name: name.trim(), role: role.trim(), color, face, model, instructions }, agent.id)
        : await save(root, { ...blankAgent(), name: name.trim(), role: role.trim(), color, face, model, instructions, status: 'active', proposedBy: 'user' }, null)
      if (!agent) {
        setName('')
        setRole('')
        setInstructions('')
      }
      onCreated?.(made)
      onClose()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setSaving(false)
    }
  }

  return (
    <Dialog open={open} onClose={onClose} maxWidth="md" fullWidth slotProps={{ paper: { sx: { p: 0, overflow: 'hidden', borderRadius: '16px', maxWidth: 860 } } }}>
      <Box sx={{ display: 'grid', gridTemplateColumns: { xs: '1fr', md: '280px 1fr' }, height: { md: 'min(640px, calc(100vh - 96px))' } }}>
        {/* The stage: the head, large, lit in its own shell colour, with the
            name and job as they will read in the agent list. */}
        <Stack
          sx={{
            alignItems: 'center',
            justifyContent: 'center',
            gap: '20px',
            p: '28px',
            background: `radial-gradient(90% 70% at 50% 35%, ${alpha(hex, 0.32)}, ${alpha(hex, 0.06)} 70%, transparent)`,
            borderRight: { md: 1 },
            borderColor: { md: 'divider' },
          }}
        >
          <FaceStage name={name} color={color} face={face} />
          <Stack sx={{ alignItems: 'center', gap: '4px', textAlign: 'center', maxWidth: 220 }}>
            <Typography sx={{ fontSize: '18px', fontWeight: 600, lineHeight: 1.25, wordBreak: 'break-word' }}>{name || 'Unnamed'}</Typography>
            <Typography sx={{ fontSize: '13px', color: 'text.secondary', display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical', overflow: 'hidden' }}>
              {role || 'What it is there to do'}
            </Typography>
          </Stack>
          <ButtonBase
            onClick={() => look(randomLook())}
            sx={{ height: 30, px: '14px', borderRadius: '999px', fontSize: '12px', fontWeight: 500, bgcolor: alpha(hex, 0.18), color: 'text.primary', '&:hover': { bgcolor: alpha(hex, 0.28) }, '&:active': { transform: 'scale(0.97)' } }}
          >
            Surprise me
          </ButtonBase>
        </Stack>

        {/* The form, with its actions pinned under it. */}
        <Stack sx={{ minHeight: 0 }}>
          <Stack sx={{ flex: 1, minHeight: 0, overflowY: 'auto', px: '28px', pt: '24px', pb: '8px', gap: '20px' }}>
            <Typography sx={{ fontSize: '18px', fontWeight: 600 }}>{agent ? `Edit ${agent.name}` : title}</Typography>
            <Field label="Name">
              <InputBase autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="Inbox Assistant" sx={fieldSx(hex)} />
            </Field>
            <Field label="Job" hint="One sentence">
              <InputBase value={role} onChange={(e) => setRole(e.target.value)} placeholder="Reads my email each morning and drafts replies" sx={fieldSx(hex)} />
            </Field>
            <Field label="Look">
              <FaceControls name={name} color={color} face={face} onChange={look} />
            </Field>
            <Field label="Model">
              <ModelPicker mode="agent" variant="field" value={model} onChange={setModel} />
            </Field>
            <Field label="Instructions" hint="How it should work, every time">
              <InputBase
                multiline
                minRows={4}
                maxRows={10}
                value={instructions}
                onChange={(e) => setInstructions(e.target.value)}
                placeholder={'Check the inbox at 8am.\nNever send anything without asking me first.'}
                sx={{ ...fieldSx(hex), alignItems: 'flex-start', lineHeight: 1.5 }}
              />
            </Field>
          </Stack>
          <Stack direction="row" sx={{ alignItems: 'center', gap: '8px', px: '28px', py: '16px', borderTop: 1, borderColor: 'divider' }}>
            <Typography sx={{ flex: 1, minWidth: 0, fontSize: '12px', color: 'error.main' }} noWrap title={error ?? undefined}>
              {error}
            </Typography>
            <ButtonBase onClick={onClose} sx={{ height: 36, px: '16px', borderRadius: '10px', fontSize: '13px', fontWeight: 500, color: 'text.secondary', '&:hover': { bgcolor: 'action.hover', color: 'text.primary' } }}>
              Cancel
            </ButtonBase>
            <ButtonBase
              onClick={() => void create()}
              disabled={saving || !name.trim()}
              sx={{ height: 36, minWidth: 96, px: '18px', borderRadius: '10px', fontSize: '13px', fontWeight: 600, bgcolor: hex, color: onHex, '&:hover': { filter: 'brightness(1.08)' }, '&:active': { transform: 'scale(0.98)' }, '&:disabled': { opacity: 0.45 } }}
            >
              {saving ? <CircularProgress size={14} sx={{ color: onHex }} /> : agent ? 'Save' : 'Create'}
            </ButtonBase>
          </Stack>
        </Stack>
      </Box>
    </Dialog>
  )
}
