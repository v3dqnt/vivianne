// Parses every openui-lang example in the OpenUI guide against the library
// the chat renders with. Run: node packages/gui-next/scripts/openui-selfcheck.mjs
import { createRequire } from 'node:module'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import path from 'node:path'
const here = path.dirname(fileURLToPath(import.meta.url))
const require = createRequire(path.join(here, '..', 'package.json'))
const core = require('@openuidev/lang-core')
const ui = require('@openuidev/react-ui/genui-lib')
const guide = readFileSync(path.join(here, '..', '..', 'vivianne-guides', 'openui', 'index.md'), 'utf8')
const blocks = [...guide.matchAll(/```openui-lang\n([\s\S]*?)```/g)].map((m) => m[1])
const parser = core.createParser(ui.openuiChatLibrary.toJSONSchema(), ui.openuiChatLibrary.root)
let bad = 0
for (const [i, src] of blocks.entries()) {
  const r = parser.parse(src)
  const problems = [...(r.meta.errors ?? []), ...r.meta.unresolved.map((u) => 'unresolved ' + u), ...r.meta.orphaned.map((o) => 'orphaned ' + o)]
  console.log(`block ${i}: ${r.root ? 'root ' + r.root.typeName : 'NO ROOT'}, ${r.meta.statementCount} statements`, problems.length ? JSON.stringify(problems).slice(0, 500) : 'ok')
  if (!r.root || problems.length) bad++
}
process.exit(bad ? 1 : 0)
