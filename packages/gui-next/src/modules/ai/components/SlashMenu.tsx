import { Box, useTheme } from '../../../ui'
import { MagicpenLinear, ArrowRightLinear } from '../../../icons'
import type { SlashCommand } from '../../../lib/slashCommands'
import { MentionRow, MentionSection, MentionShell, type MentionableSkill } from './MentionMenu'

export type SlashItem = { kind: 'command'; command: SlashCommand } | { kind: 'skill'; skill: MentionableSkill }

const MONO = '"Geist Mono", ui-monospace, monospace'

/** The `/` picker: commands first, then skills, in the same card as the `@` picker. */
export function SlashMenu({ items, activeIndex, onSelect, onHover }: { items: SlashItem[]; activeIndex: number; onSelect: (item: SlashItem) => void; onHover: (index: number) => void }) {
  const theme = useTheme()
  if (items.length === 0) return null
  const commands = items.filter((i): i is Extract<SlashItem, { kind: 'command' }> => i.kind === 'command')
  const skills = items.filter((i): i is Extract<SlashItem, { kind: 'skill' }> => i.kind === 'skill')
  let at = 0
  return (
    <MentionShell title="Commands">
      <Box sx={{ maxHeight: 300, overflowY: 'auto', px: 0.5, pb: 0.5 }}>
        {commands.length > 0 && <MentionSection label="Commands" count={commands.length} />}
        {commands.map((it) => {
          const idx = at++
          return (
            <MentionRow
              key={`cmd-${it.command.name}`}
              active={idx === activeIndex}
              onSelect={() => onSelect(it)}
              onHover={() => onHover(idx)}
              tint={theme.palette.text.secondary}
              icon={<ArrowRightLinear size={13} color="currentColor" />}
              title={
                <Box component="span" sx={{ fontFamily: MONO }}>
                  /{it.command.name}
                  {it.command.args && <Box component="span" sx={{ color: 'text.disabled' }}> {it.command.args}</Box>}
                </Box>
              }
              subtitle={it.command.description}
            />
          )
        })}
        {skills.length > 0 && <MentionSection label="Skills" count={skills.length} />}
        {skills.map((it) => {
          const idx = at++
          return (
            <MentionRow
              key={`skill-${it.skill.name}`}
              active={idx === activeIndex}
              onSelect={() => onSelect(it)}
              onHover={() => onHover(idx)}
              tint={theme.palette.primary.main}
              icon={<MagicpenLinear size={13} color="currentColor" />}
              title={<Box component="span" sx={{ fontFamily: MONO }}>@{it.skill.name}</Box>}
              subtitle={it.skill.description}
            />
          )
        })}
      </Box>
    </MentionShell>
  )
}
