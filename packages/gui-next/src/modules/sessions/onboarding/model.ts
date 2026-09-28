/**
 * The onboarding interview: what is asked, the options offered, and how the
 * answers become a charter.
 *
 * Kept separate from the UI because the answers have three consumers — the
 * wizard that collects them, the model that drafts from them, and the
 * model-free charter built when the model can't be reached — and all three
 * must agree on what each answer means.
 */

export type ProjectKind =
  | 'Web app'
  | 'Mobile app'
  | 'Desktop app'
  | 'API / backend service'
  | 'CLI tool'
  | 'Library / SDK'
  | 'Browser extension'
  | 'Game'
  | 'AI / ML project'
  | 'Automation / scripts'
  | 'Website'

export const KINDS: ProjectKind[] = [
  'Web app',
  'Mobile app',
  'Desktop app',
  'API / backend service',
  'CLI tool',
  'Library / SDK',
  'Browser extension',
  'Game',
  'AI / ML project',
  'Automation / scripts',
  'Website',
]

export const STAGES = ['Just an idea', 'There’s a spec or designs', 'A prototype exists', 'Extending an existing product']

const PLATFORMS: Partial<Record<ProjectKind, string[]>> = {
  'Web app': ['Desktop browsers', 'Mobile browsers', 'Installable (PWA)'],
  Website: ['Desktop browsers', 'Mobile browsers'],
  'Mobile app': ['iOS', 'Android'],
  'Desktop app': ['Windows', 'macOS', 'Linux'],
  'CLI tool': ['Windows', 'macOS', 'Linux'],
  'Library / SDK': ['Node.js', 'Browsers', 'Python', 'JVM', 'Native'],
  'Browser extension': ['Chrome', 'Firefox', 'Edge', 'Safari'],
  Game: ['PC', 'Web', 'Mobile', 'Console'],
}

export function platformsFor(kind: string | null): string[] {
  return (kind && PLATFORMS[kind as ProjectKind]) || []
}

export const AUDIENCES = [
  'Just me',
  'Friends & family',
  'My team / company (internal)',
  'Consumers (public)',
  'Businesses (B2B)',
  'Developers',
]

export const SCALES = ['Personal use', 'Up to ~100 users', 'Thousands of users', 'Large scale from day one']

const APP_FEATURES = [
  'User accounts & login',
  'Saved data / database',
  'Payments & subscriptions',
  'File uploads',
  'Search',
  'Notifications (email / push)',
  'Admin dashboard',
  'Real-time updates',
  'Offline support',
  'Sharing & collaboration',
  'Import / export',
  'Multiple languages',
  'Analytics',
  'AI / LLM features',
]

const FEATURES: Record<ProjectKind, string[]> = {
  'Web app': APP_FEATURES,
  'Mobile app': [...APP_FEATURES, 'Camera / photos', 'Location', 'Home-screen widgets'],
  'Desktop app': [...APP_FEATURES, 'System tray', 'Auto-update', 'File system access', 'Keyboard shortcuts'],
  'API / backend service': [
    'REST endpoints',
    'GraphQL',
    'Auth (API keys / OAuth)',
    'Rate limiting',
    'Webhooks',
    'Background jobs',
    'Database',
    'Caching',
    'API docs (OpenAPI)',
    'Admin endpoints',
  ],
  'CLI tool': ['Subcommands', 'Config file', 'Interactive prompts', 'Coloured output', 'JSON output', 'Plugins', 'Shell completion', 'Auto-update'],
  'Library / SDK': ['Typed public API', 'Docs site', 'Examples', 'Published to a registry', 'Zero dependencies', 'Tree-shakeable', 'CLI wrapper'],
  'Browser extension': ['Popup UI', 'Content scripts', 'Options page', 'Synced storage', 'Context menu', 'Keyboard shortcuts', 'Side panel'],
  Game: ['Single player', 'Multiplayer', 'Save games', 'Leaderboards', 'Level editor', 'Controller support', 'Sound & music', 'Achievements'],
  'AI / ML project': ['Data pipeline', 'Model training', 'Fine-tuning', 'Inference API', 'Evaluation / benchmarks', 'RAG / retrieval', 'Agents / tool use', 'Notebooks'],
  'Automation / scripts': ['Scheduled runs', 'Web scraping', 'Spreadsheet / file processing', 'API integrations', 'Email / chat notifications', 'Logging & alerts'],
  Website: ['Blog / CMS', 'Contact form', 'SEO', 'Analytics', 'Multiple languages', 'Newsletter signup', 'Image gallery'],
}

export function featuresFor(kind: string | null): string[] {
  return (kind && FEATURES[kind as ProjectKind]) || APP_FEATURES
}

export interface TechGroup {
  id: string
  label: string
  options: string[]
  multi: boolean
}

const G = {
  language: { id: 'language', label: 'Language', multi: true, options: ['TypeScript', 'JavaScript', 'Python', 'Rust', 'Go', 'Java / Kotlin', 'Swift', 'C#', 'C++'] },
  frontend: { id: 'frontend', label: 'Frontend', multi: false, options: ['React', 'Next.js', 'Vue / Nuxt', 'Svelte / SvelteKit', 'Angular', 'Solid', 'Plain HTML / JS'] },
  mobile: { id: 'mobile', label: 'Mobile framework', multi: false, options: ['React Native / Expo', 'Flutter', 'SwiftUI (iOS native)', 'Jetpack Compose (Android native)', 'Ionic / Capacitor'] },
  desktop: { id: 'desktop', label: 'Desktop framework', multi: false, options: ['Tauri', 'Electron', 'Native (WinUI / AppKit / GTK)', 'Qt', 'Flutter'] },
  backend: { id: 'backend', label: 'Backend', multi: false, options: ['Node (Express / Fastify)', 'Next.js API routes', 'Python (FastAPI / Django)', 'Go', 'Rust (Axum)', 'Java (Spring)', '.NET', 'Serverless functions', 'No backend'] },
  database: { id: 'database', label: 'Database', multi: false, options: ['PostgreSQL', 'SQLite', 'MySQL', 'MongoDB', 'Supabase', 'Firebase', 'Redis', 'No database'] },
  styling: { id: 'styling', label: 'Styling / UI kit', multi: false, options: ['Tailwind', 'MUI', 'shadcn/ui', 'CSS modules', 'Plain CSS'] },
  hosting: { id: 'hosting', label: 'Hosting', multi: false, options: ['Vercel', 'Netlify', 'Cloudflare', 'AWS', 'Google Cloud', 'Azure', 'Fly.io / Render', 'Self-hosted / VPS', 'Not decided'] },
  engine: { id: 'engine', label: 'Game engine', multi: false, options: ['Godot', 'Unity', 'Unreal', 'Bevy', 'Phaser', 'Plain canvas / WebGL'] },
  ml: { id: 'ml', label: 'ML stack', multi: true, options: ['PyTorch', 'TensorFlow', 'scikit-learn', 'Hugging Face', 'LangChain / LlamaIndex', 'OpenAI / Anthropic APIs', 'Local models (llama.cpp / Ollama)'] },
  site: { id: 'site', label: 'Site framework', multi: false, options: ['Astro', 'Next.js', 'Hugo', 'Eleventy', 'Plain HTML'] },
} satisfies Record<string, TechGroup>

const TECH: Record<ProjectKind, TechGroup[]> = {
  'Web app': [G.language, G.frontend, G.styling, G.backend, G.database, G.hosting],
  'Mobile app': [G.language, G.mobile, G.backend, G.database, G.hosting],
  'Desktop app': [G.language, G.desktop, G.frontend, G.styling, G.database],
  'API / backend service': [G.language, G.backend, G.database, G.hosting],
  'CLI tool': [G.language],
  'Library / SDK': [G.language],
  'Browser extension': [G.language, G.frontend, G.styling],
  Game: [G.language, G.engine],
  'AI / ML project': [G.language, G.ml, G.database, G.hosting],
  'Automation / scripts': [G.language, G.hosting],
  Website: [G.site, G.styling, G.hosting],
}

export function techGroupsFor(kind: string | null): TechGroup[] {
  return (kind && TECH[kind as ProjectKind]) || [G.language, G.frontend, G.backend, G.database, G.hosting]
}

export const PRIORITIES = [
  'Ship fast',
  'Code quality & maintainability',
  'Performance',
  'Security & privacy',
  'Accessibility',
  'Polished UI / UX',
  'Low running cost',
  'Simplicity',
]
export const MAX_PRIORITIES = 3

export const TESTING = [
  'Prototype — no tests needed',
  'Tests for the core logic',
  'Thorough (unit + integration)',
  'Full, including end-to-end',
]

export const DEADLINES = ['No deadline', 'This week', 'This month', '1–3 months', 'Longer']

export const DESIGN = [
  'No design yet — the master proposes one',
  'I have a rough idea (see notes)',
  'I have designs or mockups',
  'Match an existing brand or product',
]

export const STYLES = ['Minimal', 'Modern / clean', 'Playful', 'Corporate', 'Dark theme', 'Light theme', 'Dense / data-heavy', 'Bold & colourful']

export const AUTONOMY = [
  'Decide and build — tell me what you chose',
  'Propose a plan first, then build',
  'Ask before every significant decision',
]
export const CHECK_INS = ['Only when done or blocked', 'At each milestone', 'Often — small steps']
export const GIT = ['Commit after each working step', 'Commit at milestones', 'Leave git to me', 'Not a git project']
export const EXPLANATIONS = ['Brief — just what changed', 'Explain the reasoning', 'Teach me as you go']

export interface FollowUp {
  question: string
  options: string[]
  multi: boolean
  answer: string[]
  note: string
}

export interface Answers {
  name: string
  folderMode: 'new' | 'existing'
  parent: string
  existing: string
  pitch: string

  kind: string | null
  platforms: string[]
  stage: string | null

  audience: string[]
  problem: string
  scale: string | null

  features: string[]
  mustHave: string[]
  later: string[]
  outOfScope: string[]

  stackChoice: 'agent' | 'specify'
  tech: Record<string, string[]>
  techNotes: string

  priorities: string[]
  testing: string | null
  deadline: string | null
  design: string | null
  styles: string[]
  constraints: string

  autonomy: string | null
  checkIns: string | null
  git: string | null
  explanations: string | null

  followUps: FollowUp[]
  notes: string
}

export function emptyAnswers(name = ''): Answers {
  return {
    name,
    folderMode: 'new',
    parent: '',
    existing: '',
    pitch: '',
    kind: null,
    platforms: [],
    stage: null,
    audience: [],
    problem: '',
    scale: null,
    features: [],
    mustHave: [],
    later: [],
    outOfScope: [],
    stackChoice: 'agent',
    tech: {},
    techNotes: '',
    priorities: [],
    testing: null,
    deadline: null,
    design: null,
    styles: [],
    constraints: '',
    // The two defaults most people would pick anyway, pre-selected so the
    // step reads as "confirm" rather than "decide from scratch".
    autonomy: AUTONOMY[1],
    checkIns: CHECK_INS[1],
    git: null,
    explanations: EXPLANATIONS[0],
    followUps: [],
    notes: '',
  }
}

function list(items: string[]): string {
  return items.filter((s) => s.trim()).join(', ')
}

function chosenTech(a: Answers): Array<[string, string]> {
  if (a.stackChoice !== 'specify') return []
  return techGroupsFor(a.kind)
    .map((g): [string, string] => [g.label, list(a.tech[g.id] ?? [])])
    .filter(([, v]) => v)
}

/**
 * Everything answered, as labelled lines — what the model drafts from, and
 * what the follow-up questions are generated from. Unanswered fields are left
 * out rather than written as "none", so the model can't mistake "didn't say"
 * for "said no".
 */
export function toBrief(a: Answers, opts: { includeFollowUps?: boolean } = {}): string {
  const lines: string[] = []
  const add = (label: string, value: string | null | undefined) => {
    if (value && value.trim()) lines.push(`${label}: ${value.trim()}`)
  }
  add('Project name', a.name)
  add('In one sentence', a.pitch)
  add('Kind of project', a.kind)
  add('Platforms', list(a.platforms))
  add('Current stage', a.stage)
  add('Who it is for', list(a.audience))
  add('Problem it solves', a.problem)
  add('Expected scale', a.scale)
  add('Features for the first version', list([...a.features, ...a.mustHave]))
  add('Later / nice to have', list(a.later))
  add('Explicitly out of scope', list(a.outOfScope))
  if (a.stackChoice === 'agent') lines.push('Technology: left to the agent to choose')
  for (const [label, value] of chosenTech(a)) add(`Technology — ${label}`, value)
  add('Technology notes', a.techNotes)
  add('Top priorities', list(a.priorities))
  add('Testing expectations', a.testing)
  add('Timeline', a.deadline)
  add('Design', a.design)
  add('Visual style', list(a.styles))
  add('Constraints', a.constraints)
  add('Existing folder', a.folderMode === 'existing' ? a.existing : null)
  if (opts.includeFollowUps !== false) {
    for (const f of a.followUps) {
      const answer = list([...f.answer, f.note])
      if (answer) lines.push(`Q: ${f.question}\nA: ${answer}`)
    }
  }
  add('Other notes', a.notes)
  return lines.join('\n')
}

function section(title: string, body: string[], empty = '_Not specified yet._'): string {
  const content = body.filter((s) => s.trim())
  return `## ${title}\n${content.length ? content.join('\n') : empty}\n`
}

const bullets = (items: string[]) => items.filter((s) => s.trim()).map((s) => `- ${s.trim()}`)

/**
 * The charter built directly from the answers, with no model.
 *
 * Used when the model can't be reached. Because the interview is structured
 * this is a real charter, not a template to fill in — every answered question
 * lands in its section, and what was left unanswered becomes an open question
 * for the master to raise.
 */
export function toCharter(a: Answers): string {
  const open: string[] = []
  if (!a.audience.length) open.push('Who is this for?')
  if (!a.problem.trim()) open.push('What problem does it solve for them?')
  if (!a.features.length && !a.mustHave.length) open.push('Which features does the first version need?')
  if (!a.testing) open.push('How thoroughly should it be tested?')
  for (const f of a.followUps) if (!f.answer.length && !f.note.trim()) open.push(f.question)

  const tech = chosenTech(a)
  const decisions = tech.map(([label, value]) => `- ${label}: ${value} — chosen at onboarding`)

  return [
    `# ${a.name.trim() || 'Untitled project'}\n`,
    section('Purpose', [a.pitch, a.problem ? `\n${a.problem}` : '']),
    section('Users', [
      a.audience.length ? `For: ${list(a.audience)}.` : '',
      a.scale ? `Expected scale: ${a.scale}.` : '',
    ]),
    section('Scope', [
      a.kind ? `A ${a.kind.toLowerCase()}${a.platforms.length ? ` for ${list(a.platforms)}` : ''}.` : '',
      a.stage ? `Starting point: ${a.stage.toLowerCase()}.` : '',
      ...bullets([...a.features, ...a.mustHave]),
      a.later.length ? `\nLater:\n${bullets(a.later).join('\n')}` : '',
    ]),
    section('Out of scope', bullets(a.outOfScope)),
    section('Technical direction', [
      a.stackChoice === 'agent'
        ? 'The stack is left to the master to choose. It records what it picks, and why, under Decisions.'
        : tech.map(([l, v]) => `- ${l}: ${v}`).join('\n'),
      a.techNotes,
    ]),
    section('Constraints', [
      a.priorities.length ? `Priorities, in order: ${a.priorities.join(', ')}.` : '',
      a.deadline ? `Timeline: ${a.deadline}.` : '',
      a.design ? `Design: ${a.design}.` : '',
      a.styles.length ? `Visual style: ${list(a.styles)}.` : '',
      a.constraints,
    ]),
    section('Definition of done', [
      ...bullets([...a.features, ...a.mustHave].map((f) => `${f} works end to end`)),
      a.testing ? `- Testing: ${a.testing}` : '',
    ]),
    section('Decisions', decisions.length ? decisions : ['_None recorded yet._']),
    // "Not specified yet" would read as a skipped question here; an empty
    // list of open questions is an achievement, not a gap.
    section('Open questions', bullets(open), '_None — every question was answered._'),
    ...a.followUps
      .filter((f) => f.answer.length || f.note.trim())
      .map((f, i) => (i === 0 ? '## Clarifications\n' : '') + `- **${f.question}** ${list([...f.answer, f.note])}\n`),
    a.notes.trim() ? section('Notes', [a.notes]) : '',
  ]
    .filter(Boolean)
    .join('\n')
}

/** How the master should work, as standing instructions for `master.md`. */
export function toInstructions(a: Answers): string {
  const lines: string[] = []
  switch (a.autonomy) {
    case AUTONOMY[0]:
      lines.push('Make decisions yourself and keep building; report what you chose and why.')
      break
    case AUTONOMY[1]:
      lines.push('Before starting anything substantial, propose a short plan and wait for the go-ahead.')
      break
    case AUTONOMY[2]:
      lines.push('Ask before every significant decision — architecture, dependencies, data shapes, anything hard to reverse.')
      break
  }
  switch (a.checkIns) {
    case CHECK_INS[0]:
      lines.push('Only check in when the work is done or you are blocked.')
      break
    case CHECK_INS[1]:
      lines.push('Check in at each milestone with what works, what is next, and anything you need decided.')
      break
    case CHECK_INS[2]:
      lines.push('Work in small steps and check in after each one.')
      break
  }
  switch (a.git) {
    case GIT[0]:
      lines.push('Commit after each step that works, with a clear message.')
      break
    case GIT[1]:
      lines.push('Commit at each milestone.')
      break
    case GIT[2]:
      lines.push('Do not commit — the user handles git.')
      break
    case GIT[3]:
      lines.push('This is not a git project; do not initialise one unless asked.')
      break
  }
  switch (a.explanations) {
    case EXPLANATIONS[0]:
      lines.push('Keep explanations brief: what changed and how to see it.')
      break
    case EXPLANATIONS[1]:
      lines.push('Explain the reasoning behind non-obvious choices.')
      break
    case EXPLANATIONS[2]:
      lines.push('Teach as you go: explain concepts and trade-offs, the user wants to learn.')
      break
  }
  if (a.priorities.length) lines.push(`When trade-offs conflict, favour: ${a.priorities.join(' > ')}.`)
  return lines.map((l) => `- ${l}`).join('\n')
}
