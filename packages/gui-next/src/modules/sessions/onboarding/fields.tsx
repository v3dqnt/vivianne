import { useState, type ReactNode } from 'react'
import { Stack, Box, Typography, Chip, InputBase, alpha, useTheme } from '../../../ui'
import { CloseLinear } from '../../../icons'

/** A question: label, an optional hint underneath, then its input. */
export function Question({ label, hint, children }: { label: string; hint?: ReactNode; children: ReactNode }) {
  return (
    <Stack sx={{ gap: 1 }}>
      <Stack sx={{ gap: 0.25 }}>
        <Typography variant="body2" sx={{ fontWeight: 600 }}>
          {label}
        </Typography>
        {hint && (
          <Typography variant="caption" color="text.secondary" sx={{ lineHeight: 1.45 }}>
            {hint}
          </Typography>
        )}
      </Stack>
      {children}
    </Stack>
  )
}

function OptionChip({ label, selected, onClick, disabled }: { label: string; selected: boolean; onClick: () => void; disabled?: boolean }) {
  const theme = useTheme()
  return (
    <Chip
      label={label}
      onClick={onClick}
      disabled={disabled && !selected}
      variant={selected ? 'filled' : 'outlined'}
      aria-pressed={selected}
      sx={{
        borderRadius: '8px',
        fontSize: '0.8125rem',
        height: 30,
        fontWeight: selected ? 600 : 400,
        ...(selected
          ? {
              bgcolor: alpha(theme.palette.primary.main, 0.18),
              color: 'primary.main',
              border: 1,
              borderColor: alpha(theme.palette.primary.main, 0.45),
              '&:hover': { bgcolor: alpha(theme.palette.primary.main, 0.24) },
            }
          : { borderColor: 'divider' }),
      }}
    />
  )
}

/**
 * Pick from options, with an "Other…" escape hatch.
 *
 * The options are there to make answering fast and to prompt answers the
 * user wouldn't have thought to give; "Other" is there because no list is
 * ever complete, and forcing a nearest-match would put a wrong fact in the
 * charter.
 */
export function Choices(
  props:
    | { options: string[]; multi: true; value: string[]; onChange: (v: string[]) => void; max?: number; allowOther?: boolean }
    | { options: string[]; multi?: false; value: string | null; onChange: (v: string | null) => void; allowOther?: boolean },
) {
  const { options, allowOther = true } = props
  const [otherOpen, setOtherOpen] = useState(false)
  const [otherText, setOtherText] = useState('')

  const selected: string[] = props.multi ? props.value : props.value ? [props.value] : []
  // Custom answers the user typed earlier stay visible as chips of their own.
  const custom = selected.filter((s) => !options.includes(s))
  const atMax = props.multi && props.max !== undefined && selected.length >= props.max

  const toggle = (opt: string) => {
    if (props.multi) {
      props.onChange(selected.includes(opt) ? selected.filter((s) => s !== opt) : [...selected, opt])
    } else {
      props.onChange(props.value === opt ? null : opt)
    }
  }

  const commitOther = () => {
    const text = otherText.trim()
    if (text) {
      if (props.multi) {
        if (!selected.includes(text)) props.onChange([...selected, text])
      } else {
        props.onChange(text)
      }
    }
    setOtherText('')
    setOtherOpen(false)
  }

  return (
    <Stack direction="row" sx={{ flexWrap: 'wrap', gap: 0.75, alignItems: 'center' }}>
      {[...options, ...custom].map((opt) => {
        // A capped multi-choice is a ranking ("pick up to 3, most important
        // first"), and the order clicked is the order recorded — so it has to
        // be visible, or the user can't tell what they ranked first.
        const rank = props.multi && props.max !== undefined ? selected.indexOf(opt) + 1 : 0
        return (
          <OptionChip
            key={opt}
            label={rank > 0 ? `${rank}. ${opt}` : opt}
            selected={selected.includes(opt)}
            onClick={() => toggle(opt)}
            disabled={!!atMax}
          />
        )
      })}
      {allowOther &&
        (otherOpen ? (
          <InputBase
            autoFocus
            value={otherText}
            onChange={(e) => setOtherText(e.target.value)}
            onBlur={commitOther}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault()
                commitOther()
              }
              if (e.key === 'Escape') {
                setOtherText('')
                setOtherOpen(false)
              }
            }}
            placeholder="Type and press Enter"
            sx={{ fontSize: '0.8125rem', px: 1.25, height: 30, border: 1, borderColor: 'primary.main', borderRadius: '8px', minWidth: 200 }}
          />
        ) : (
          <Chip
            label="Other…"
            variant="outlined"
            onClick={() => setOtherOpen(true)}
            disabled={!!atMax}
            sx={{ borderRadius: '8px', fontSize: '0.8125rem', height: 30, borderStyle: 'dashed', color: 'text.secondary' }}
          />
        ))}
      {props.multi && props.max !== undefined && (
        <Typography variant="caption" color="text.disabled" sx={{ ml: 0.5 }}>
          {selected.length}/{props.max}
        </Typography>
      )}
    </Stack>
  )
}

/** A list built one item at a time — for features, exclusions, anything
 * that is a list rather than a choice. */
export function ListInput({
  value,
  onChange,
  placeholder,
  id,
}: {
  value: string[]
  onChange: (v: string[]) => void
  placeholder: string
  id: string
}) {
  const [draft, setDraft] = useState('')
  const add = () => {
    const text = draft.trim()
    if (text && !value.includes(text)) onChange([...value, text])
    setDraft('')
  }
  return (
    <Stack sx={{ gap: 0.75 }}>
      {value.length > 0 && (
        <Stack sx={{ gap: 0.5 }}>
          {value.map((item) => (
            <Stack
              key={item}
              direction="row"
              sx={{ alignItems: 'center', gap: 1, px: 1.25, py: 0.5, borderRadius: '8px', bgcolor: 'action.hover' }}
            >
              <Typography variant="body2" sx={{ flex: 1, fontSize: '0.8125rem' }}>
                {item}
              </Typography>
              <Box
                component="button"
                type="button"
                aria-label={`Remove ${item}`}
                onClick={() => onChange(value.filter((v) => v !== item))}
                sx={{ border: 0, bgcolor: 'transparent', color: 'text.secondary', cursor: 'pointer', display: 'grid', p: 0.25 }}
              >
                <CloseLinear size={12} color="currentColor" />
              </Box>
            </Stack>
          ))}
        </Stack>
      )}
      <InputBase
        id={id}
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={add}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            e.preventDefault()
            add()
          }
        }}
        placeholder={placeholder}
        sx={{ fontSize: '0.8125rem', px: 1.25, py: 0.75, border: 1, borderColor: 'divider', borderRadius: '8px' }}
      />
    </Stack>
  )
}
