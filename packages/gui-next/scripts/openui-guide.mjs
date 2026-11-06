// Writes the OpenUI guide an agent reads (guide tool, `openui`) before it
// answers with interactive UI. The component reference is generated from the
// library the chat renders with (@openuidev/react-ui's chat library), so the
// two can't drift: rerun this after upgrading the package.
//
//   node packages/gui-next/scripts/openui-guide.mjs
import { createRequire } from 'node:module'
import { writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const here = path.dirname(fileURLToPath(import.meta.url))
const require = createRequire(path.join(here, '..', 'package.json'))
const ui = require('@openuidev/react-ui/genui-lib')

const full = ui.openuiChatLibrary.prompt({ ...ui.openuiChatPromptOptions, inlineMode: true })

// Keep the language and the components; drop the parts that don't fit a
// chat where most replies are text: the "your ENTIRE response is code"
// preamble, the dashboard-editing inline mode, and "generate plausible data"
// (an agent's numbers come from what it looked up, never from thin air).
const from = full.indexOf('## Syntax Rules')
const inline = full.indexOf('## Inline Mode')
const rules = full.indexOf('## Important Rules')
if (from < 0 || inline < 0 || rules < 0) throw new Error('the OpenUI prompt changed shape: update openui-guide.mjs')
const reference = full.slice(from, inline).trim()
const tail = full
  .slice(rules)
  .split('\n')
  .filter((l) => !/realistic|plausible/i.test(l))
  .join('\n')
  .trim()

const head = `# OpenUI answers

Some answers are better seen than read: the weather, a comparison, a trip
plan, search results, a status report, a form to fill in, a chart. In a
conversation with an agent, write those as OpenUI: a small program in
openui-lang inside a fenced block. It renders live in the chat, as a card,
while it streams.

\`\`\`openui-lang
root = Card([header, now, week, more], [src1])
header = CardHeader("Panaji, Goa", "Now, from the forecast [1]")
now = TextContent("**29 °C** · humid, light breeze from the west", "large-heavy")
week = Table([days, highs, skies])
days = Col("Day", ["Fri", "Sat", "Sun"])
highs = Col("High (°C)", [31, 30, 29], "number")
skies = Col("Sky", ["Sunny", "Cloudy", "Showers"])
more = FollowUpBlock([FollowUpItem("Hourly for today"), FollowUpItem("Is it a good week for the beach?")])
src1 = {title: "Panaji forecast", sourceName: "the forecast site", url: "https://..."}
\`\`\`

## When

- Use it when a result has shape: numbers to compare, items to scan, steps,
  a timeline, options to pick from, a form. One fact or a chat reply stays
  plain text.
- Write a sentence before the block if it helps, and nothing after it: the
  card is the answer.
- One card per reply.

## Truth

- Every number, name, price and date in a card comes from your tools in
  this conversation (search, fetch, files, the user). Never invent data to
  fill a layout: leave the field out, or say it's unknown.
- Cite what you looked up: pass the pages as the Card's \`sources\` and mark
  claims with [1], [2].

## Buttons

- \`@ToAssistant("...")\` sends that message back to you in this
  conversation: use it for next steps ("Book the 9 am flight", "Show
  hotels under ₹6,000").
- \`@OpenUrl("https://...")\` opens a page in the user's browser.
- End with a FollowUpBlock of two or three likely next asks.

## Files

A file you made (a PDF, a sheet) is shown by the chat on its own: say its
name in a sentence. Don't put file paths inside the card.

---

The rest of this guide is the language and its components.

`

writeFileSync(path.join(here, '..', '..', 'vivianne-guides', 'openui', 'index.md'), head + reference + '\n\n' + tail + '\n')
console.log('wrote vivianne-guides/openui/index.md', (head + reference + tail).length, 'chars')
