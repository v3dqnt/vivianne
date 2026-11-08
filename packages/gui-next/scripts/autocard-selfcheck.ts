/**
 * Checks the report-to-card conversion on replies shaped like real ones:
 *   npx tsx scripts/autocard-selfcheck.ts
 */
import { autoCard } from '../src/modules/ai/lib/autoCard'

let failed = 0
function ok(name: string, cond: boolean, detail = '') {
  if (!cond) failed++
  console.log(`${cond ? 'ok  ' : 'FAIL'} ${name}${cond || !detail ? '' : `: ${detail}`}`)
}

const pc = [
  "I'll create a solid 1080p gaming PC build under $1000. Here's a balanced parts list:",
  '',
  '## \u{1F3AE} **Gaming PC Build ~$980**',
  '',
  '| Part | Description | Price |',
  '|------|-------------|-------|',
  '| **CPU** | AMD Ryzen 5 5600 (6-core, 12-thread) | $120 |',
  '| **GPU** | NVIDIA RTX 3060 12GB | $300 |',
  '| **TOTAL** | **Full build** | **~$1,000** |',
  '',
  '### \u{1F511} **Why This Build?**',
  '- **Ryzen 5 5600**: Excellent value, 6 cores/12 threads, great for gaming + productivity',
  '- **RTX 3060 12GB**: The 12GB VRAM is key, solid 1080p/1440p gaming',
  '- **B550 motherboard**: Supports future CPU upgrades',
  '',
  '### \u{1F4DD} **Notes**',
  '- Prices vary by retailer/region',
  '- Windows not included',
  '',
  '**Want me to tweak anything**, Intel instead of AMD? Bigger SSD?',
].join('\n')

const a = autoCard(pc)
ok('pc build becomes a card', !!a)
if (a) {
  const spec = JSON.parse(a.json) as { title?: string; icon?: string; blocks: { type: string }[] }
  ok('title from first heading', spec.title === 'Gaming PC Build ~$980', String(spec.title))
  ok('icon from its emoji', spec.icon === '\u{1F3AE}', String(spec.icon))
  ok('table block', spec.blocks[0].type === 'table')
  ok('labelled bullets become cards', spec.blocks.some((b) => b.type === 'cards'))
  ok('plain bullets stay a list', spec.blocks.some((b) => b.type === 'list'))
  ok('intro kept above', a.intro.startsWith("I'll create"))
  ok('offer kept below', a.outro.includes('Want me to tweak'))
}

const chat = 'Sure! The capital of France is Paris. It has been the capital for a very long time, and it is also the largest city in the country by a wide margin, so it is where most national institutions sit.'
ok('conversation stays text', autoCard(chat) === null)
ok('short replies stay text', autoCard('## Done\n- a\n- b') === null)
ok('fenced code stays text', autoCard('## A\n```ts\nconst a = 1\n```\n## B\n' + 'x'.repeat(250)) === null)

console.log(failed ? `${failed} failed` : 'all passed')
process.exit(failed ? 1 : 0)
