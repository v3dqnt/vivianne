import { Component, Suspense, createElement, useEffect, useRef, useState, type ComponentType, type ReactNode } from 'react'
import { Box, Stack, Typography } from '../ui'
import { FIXTURES, NOT_COMPONENTS } from './fixtures'

type Loader = () => Promise<Record<string, unknown>>
type Entry = { path: string; name: string; load: Loader }

const files = import.meta.glob(['../modules/**/*.tsx', '../components/**/*.tsx', '!../**/*.test.tsx']) as Record<string, Loader>

// A component is a function, or a memo / forwardRef object (which carries `$$typeof`); anything else exported in capitals is a constant.
const isComponent = (name: string, v: unknown) =>
  /^[A-Z]/.test(name) && !NOT_COMPONENTS.test(name) && (typeof v === 'function' || (typeof v === 'object' && v !== null && '$$typeof' in v))

class Boundary extends Component<{ children: ReactNode }, { error: string | null }> {
  state = { error: null as string | null }
  static getDerivedStateFromError(e: unknown) {
    return { error: e instanceof Error ? e.message : String(e) }
  }
  render() {
    return this.state.error ? (
      <Typography sx={{ fontSize: '0.75rem', color: 'warning.main' }}>Needs props or a backend: {this.state.error}</Typography>
    ) : (
      this.props.children
    )
  }
}

/** Every card renders itself, but only while it is near the screen: it unmounts when scrolled away (keeping its height), so only a handful of
 * components run animations, timers and listeners at any moment. */
function Card({ path, name, load }: Entry) {
  const [Comp] = useState(() => await_(load, name))
  const [near, setNear] = useState(false)
  const ref = useRef<HTMLDivElement>(null)
  const height = useRef(120)
  useEffect(() => {
    const el = ref.current
    if (!el) return
    const io = new IntersectionObserver(
      ([e]) => {
        if (!e.isIntersecting) height.current = Math.round(e.boundingClientRect.height)
        setNear(e.isIntersecting)
      },
      { rootMargin: '300px' },
    )
    io.observe(el)
    return () => io.disconnect()
  }, [])
  return (
    <Box ref={ref} sx={{ border: 1, borderColor: 'divider', borderRadius: '8px', p: '12px', display: 'flex', flexDirection: 'column', gap: '8px', minWidth: 0, minHeight: near ? undefined : height.current }}>
      <Box sx={{ minWidth: 0 }}>
        <Typography noWrap sx={{ fontSize: '0.8125rem', fontWeight: 500 }}>{name}</Typography>
        <Typography noWrap sx={{ fontSize: '0.6875rem', color: 'text.disabled' }}>{path.replace('../', '')}</Typography>
      </Box>
      <Box sx={{ position: 'relative', minHeight: 40, maxHeight: 420, overflow: 'auto', p: '8px', borderRadius: '8px', bgcolor: 'action.hover' }}>
        {near && (
          <Boundary>
            <Suspense fallback={<Typography variant="caption">Loading…</Typography>}>
              <Comp />
            </Suspense>
          </Boundary>
        )}
      </Box>
    </Box>
  )
}

/** A component that loads its module on first render, so the card needs no separate loading state. */
function await_(load: Loader, name: string): ComponentType {
  let mod: Record<string, unknown> | null = null
  let pending: Promise<void> | null = null
  return function Lazy() {
    if (!mod) {
      pending ??= load().then((m) => void (mod = m))
      throw pending
    }
    const C = mod[name] as ComponentType
    return createElement(C, FIXTURES[name])
  }
}

/** Every component under `modules/` and `components/`, found from the source files, each rendered on demand with no props. */
export function AppSection() {
  const [entries, setEntries] = useState<Entry[] | null>(null)
  const [query, setQuery] = useState('')
  useEffect(() => {
    void (async () => {
      const out: Entry[] = []
      for (const [path, load] of Object.entries(files)) {
        const mod = await load().catch(() => ({}) as Record<string, unknown>)
        for (const [name, v] of Object.entries(mod)) if (isComponent(name, v)) out.push({ path, name, load })
      }
      setEntries(out.sort((a, b) => a.path.localeCompare(b.path)))
    })()
  }, [])
  const shown = (entries ?? []).filter((e) => `${e.name} ${e.path}`.toLowerCase().includes(query.toLowerCase()))
  return (
    <Stack sx={{ gap: '12px' }}>
      <input
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        placeholder={entries ? `Filter ${entries.length} components` : 'Finding components…'}
        style={{ height: 36, padding: '0 12px', borderRadius: 8, border: '1px solid rgba(128,128,128,0.4)', background: 'transparent', color: 'inherit' }}
      />
      <Box sx={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(320px, 1fr))', gap: '10px' }}>
        {shown.map((e) => <Card key={`${e.path}:${e.name}`} {...e} />)}
      </Box>
    </Stack>
  )
}
