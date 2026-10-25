/**
 * Checks the interactive-answer evaluator: run with
 *   npx tsx scripts/ui-selfcheck.ts
 */
import { evaluate, interpolate } from '../src/modules/ai/lib/uiExpr'
import { toSpeakable } from '../src/modules/ai/lib/speakable'
import { fill } from '../src/modules/automation/fill'

let failed = 0
function eq(name: string, got: unknown, want: unknown) {
  const ok = typeof want === 'number' ? Math.abs(Number(got) - want) < 1e-6 : got === want
  if (!ok) failed++
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${ok ? '' : `: got ${String(got)}, want ${String(want)}`}`)
}
function throws(name: string, src: string) {
  try {
    evaluate(src, {})
    failed++
    console.log(`FAIL ${name}: did not throw`)
  } catch {
    console.log(`ok   ${name}`)
  }
}

eq('precedence', evaluate('1 + 2 * 3 ^ 2'), 19)
eq('unary and parens', evaluate('-(2 + 3) * 2'), -10)
eq('right-assoc power', evaluate('2 ^ 3 ^ 2'), 512)
eq('scope', evaluate('rate * years', { rate: 0.05, years: 10 }), 0.5)
eq('ternary', evaluate('x > 3 ? "big" : "small"', { x: 5 }), 'big')
eq('and/or/not', evaluate('not (a and b) or c', { a: 1, b: 0, c: 0 }), true)
eq('functions', evaluate('round(sqrt(2), 3) + max(1, 4, 2)'), 5.414)
eq('pmt', evaluate('round(pmt(0.05/12, 360, 300000), 2)'), 1610.46)
eq('fv', evaluate('round(fv(0.07/12, 120, 500), 0)'), 86542)
eq('interpolate', interpolate('Total: {a + b}', { a: 2, b: 3 }), 'Total: 5')
eq('interpolate keeps unknowns', interpolate('{nope}', {}), '{nope}')
throws('no property access', 'constructor.constructor')
throws('no unknown functions', 'alert(1)')
throws('no brackets', 'a[0]')
throws('no assignment', 'x = 1')
throws('no trailing junk', '1 2')
throws('no prototype names', 'constructor')
throws('no prototype functions', 'toString()')

eq('speech drops code', toSpeakable('Run this:\n```bash\nnpm test\n```\nThen relax.'), 'Run this:. Then relax.')
eq('speech keeps link words', toSpeakable('See [the docs](https://x.dev/a) now'), 'See the docs now')
eq('speech reads headings and lists', toSpeakable('## Plan\n- one\n- two'), 'Plan. one. two')
eq('speech drops tables', toSpeakable('| a | b |\n|---|---|\n| 1 | 2 |\nDone'), 'Done')
eq('workflow file placeholder', fill('Summarize {{file}} please', 'C:/in/a.pdf'), 'Summarize C:/in/a.pdf please')
eq('workflow no file', fill('{{ file }}', null), '(no file)')


import { isFilePath, splitFilePaths } from '../src/modules/ai/lib/filePaths'
import { parseSearchSources, sourcesByMessage } from '../src/modules/ai/lib/sources'
import { remarkFiles } from '../src/modules/ai/lib/remarkFiles'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import ReactMarkdown from 'react-markdown'

const paths = (s: string) => splitFilePaths(s).filter((p) => typeof p !== 'string').map((p) => (p as { path: string }).path).join(',')
eq('path in sentence', paths('I wrote report.docx and saved it.'), 'report.docx')
eq('path with dirs', paths('See src/app/main.ts, then run it'), 'src/app/main.ts')
eq('windows path', paths('Saved to C:\\Users\\me\\out\\a.pdf.'), 'C:\\Users\\me\\out\\a.pdf')
eq('version is not a file', paths('Upgrade to v1.8.2 or e.g. 3.5'), '')
eq('url is not a file', paths('Open https://example.com/docs/index.html now'), '')
eq('bare host path is not a file', paths('see example.com/page.html'), '')
eq('call is not a file', paths('use foo.bar() here'), '')
eq('inline code path', isFilePath('src/lib/a.rs'), true)
eq('inline code command', isFilePath('npm run build'), false)
eq('absolute path with spaces', isFilePath('C:\\My Docs\\a.txt'), true)

const md = renderToStaticMarkup(
  createElement(ReactMarkdown, { remarkPlugins: [remarkFiles], components: { span: (p: Record<string, unknown>) => createElement('b', null, `[${String(p['data-file'])}]`) } as never }, 'Made `out/plan.md` and notes.txt, not `x.y`.\n\n```\nskip.txt\n```'),
)
eq('markdown links inline code and prose', md.includes('[out/plan.md]') && md.includes('[notes.txt]'), true)
eq('markdown leaves fences', md.includes('[skip.txt]'), false)

const hits = parseSearchSources('Search results for: x\n\n1. First\n   https://a.dev/x\n   snippet\n2. Second\n   https://b.dev/y')
eq('sources parsed', hits.map((h) => h.url).join(' '), 'https://a.dev/x https://b.dev/y')
const text = (t: string) => [{ type: 'text', text: t }]
const turn = [
  { type: 'user', content: text('q') },
  { type: 'assistant', content: text('searching') },
  { type: 'toolResult', toolName: 'web_search', isError: false, content: text('1. T\n   https://a.dev/x') },
  { type: 'assistant', content: text('answer') },
] as never
eq('sources attach to the closing reply', [...sourcesByMessage(turn).keys()].join(','), '3')

import { previewText } from '../src/modules/ai/lib/previewText'
const NL = String.fromCharCode(10)
const FENCE = String.fromCharCode(96).repeat(3)
eq('preview shows card title', previewText('Done.' + NL + FENCE + 'ui' + NL + '{"title": "Weekly check", "blocks": []}' + NL + FENCE), 'Done. ' + String.fromCharCode(0x25a4) + ' Weekly check')
eq('preview shrinks code', previewText('see' + NL + FENCE + 'ts' + NL + 'const a = 1' + NL + FENCE), 'see [ts]')

import { highlights, suggest } from '../src/modules/ai/lib/fileSuggest'
const FILES = [
  { name: 'ChatComposer.tsx', path: 'src/modules/ai/components/ChatComposer.tsx' },
  { name: 'Composer.tsx', path: 'src/modules/ai/components/Composer.tsx' },
  { name: 'index.ts', path: 'src/a/index.ts' },
  { name: 'index.ts', path: 'src/b/index.ts' },
  { name: 'README.md', path: 'README.md' },
]
const sug = (text: string) => suggest(text, text.length, FILES)
eq('complete a prefix', sug('look at ChatComp')?.replacement, 'ChatComposer.tsx')
eq('complete is a completion', sug('look at ChatComp')?.kind, 'complete')
eq('fix a typo', sug('open Composr.tsx')?.replacement, 'Composer.tsx')
eq('fix is a fix', sug('open Composr.tsx')?.kind, 'fix')
eq('known name needs nothing', sug('open Composer.tsx'), null)
eq('short words are left alone', sug('do it'), null)
eq('mentions have their own menu', sug('see @Comp'), null)
eq('a shared name is offered as the name, not a chosen folder', sug('open inde')?.replacement, 'index.ts')
eq('nothing close, nothing offered', sug('open banana.tsx'), null)
const marks = highlights('fix ChatComposer.tsx, then @src/a.ts and Composer.tsx.', FILES).map((m) => 'fix ChatComposer.tsx, then @src/a.ts and Composer.tsx.'.slice(m.start, m.end))
eq('known names and mentions are marked', marks.join('|'), 'ChatComposer.tsx|@src/a.ts|Composer.tsx')

console.log(failed ? `${failed} failed` : 'all passed')
process.exit(failed ? 1 : 0)
