import { test } from 'node:test'
import assert from 'node:assert/strict'
import { autoCard } from './autoCard.ts'

const REPORT = [
  '# Review of the changes',
  '',
  'A long enough introduction paragraph so that this reads as a report and not as a chat reply, with detail.',
  '',
  '## \u{1F534} Critical bugs',
  '',
  '| File | Line | Issue |',
  '| --- | --- | --- |',
  '| `src/bots.rs` | 1495 | **`cmd_bots_room`** now takes a `limit` |',
  '',
  '## \u{1F7E2} Clean changes',
  '',
  '- `modeOf` is still exported and used',
  '- `LABEL` is still exported and used',
].join('\n')

const spec = () => JSON.parse(autoCard(REPORT)!.json) as { blocks: Record<string, unknown>[] }

test('a table cell keeps its code and bold formatting', () => {
  const table = spec().blocks.find((b) => b.type === 'table') as { rows: string[][] }
  assert.equal(table.rows[0][0], '`src/bots.rs`')
  assert.equal(table.rows[0][2], '**`cmd_bots_room`** now takes a `limit`')
})

test('a heading’s emoji becomes a tone and leaves the text', () => {
  const headings = spec().blocks.filter((b) => b.type === 'heading')
  assert.deepEqual(
    headings.map((h) => [h.text, h.tone]),
    [
      ['Critical bugs', 'error'],
      ['Clean changes', 'success'],
    ],
  )
})

test('a heading with no emoji carries no tone', () => {
  const md = REPORT.replace('\u{1F534} ', '')
  const heading = (JSON.parse(autoCard(md)!.json) as { blocks: Record<string, unknown>[] }).blocks.find((b) => b.type === 'heading')
  assert.equal(heading?.tone, undefined)
})
