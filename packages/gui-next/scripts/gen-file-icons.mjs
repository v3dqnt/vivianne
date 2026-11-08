// Builds src/lib/fileIconManifest.json from material-icon-theme: which icon each file name, extension and folder name gets.
// Run after upgrading the package:  node scripts/gen-file-icons.mjs
import { createRequire } from 'node:module'
import { writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

const require = createRequire(import.meta.url)
const { generateManifest } = require('material-icon-theme')
const m = generateManifest()

// VS Code maps many common extensions through language ids rather than extensions, so the theme's own extension table lacks them.
// These are the extensions those language ids cover.
const LANGUAGE_EXTENSIONS = {
  typescript: ['ts', 'mts', 'cts'],
  typescriptreact: ['tsx'],
  javascript: ['js', 'mjs', 'cjs'],
  javascriptreact: ['jsx'],
  python: ['py', 'pyi', 'pyw'],
  java: ['java'],
  c: ['c', 'h'],
  cpp: ['cpp', 'cc', 'cxx', 'hpp', 'hxx', 'hh'],
  csharp: ['cs'],
  go: ['go'],
  ruby: ['rb'],
  php: ['php'],
  swift: ['swift'],
  kotlin: ['kt', 'kts'],
  lua: ['lua'],
  markdown: ['md', 'markdown', 'mdx'],
  json: ['json', 'jsonc', 'json5'],
  yaml: ['yaml', 'yml'],
  html: ['html', 'htm'],
  css: ['css'],
  scss: ['scss'],
  less: ['less'],
  shellscript: ['sh', 'bash', 'zsh'],
  powershell: ['ps1', 'psm1', 'psd1'],
  bat: ['bat', 'cmd'],
  sql: ['sql'],
  xml: ['xml'],
  toml: ['toml'],
  dart: ['dart'],
  r: ['r'],
  scala: ['scala'],
  perl: ['pl', 'pm'],
  haskell: ['hs'],
  elixir: ['ex', 'exs'],
  clojure: ['clj', 'cljs'],
  vue: ['vue'],
  svelte: ['svelte'],
  dockerfile: ['dockerfile'],
  makefile: ['mk'],
  plaintext: ['txt'],
  zig: ['zig'],
  graphql: ['graphql', 'gql'],
  proto: ['proto'],
  terraform: ['tf'],
}

const ext = { ...Object.fromEntries(Object.entries(m.fileExtensions).map(([k, v]) => [k.toLowerCase(), v])) }
for (const [lang, exts] of Object.entries(LANGUAGE_EXTENSIONS)) {
  const icon = m.languageIds?.[lang]
  if (!icon) continue
  for (const e of exts) if (!ext[e]) ext[e] = icon
}

const lower = (o) => Object.fromEntries(Object.entries(o ?? {}).map(([k, v]) => [k.toLowerCase(), v]))
const out = {
  file: m.file,
  folder: m.folder,
  folderOpen: m.folderExpanded,
  ext,
  names: lower(m.fileNames),
  folders: lower(m.folderNames),
  foldersOpen: lower(m.folderNamesExpanded),
  light: {
    file: m.light?.file,
    ext: lower(m.light?.fileExtensions),
    names: lower(m.light?.fileNames),
    folders: lower(m.light?.folderNames),
    foldersOpen: lower(m.light?.folderNamesExpanded),
  },
}
const target = fileURLToPath(new URL('../src/lib/fileIconManifest.json', import.meta.url))
writeFileSync(target, JSON.stringify(out))
console.log(`wrote ${target}: ${Object.keys(out.ext).length} extensions, ${Object.keys(out.names).length} file names, ${Object.keys(out.folders).length} folders`)
