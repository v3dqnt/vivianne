import type { AgentMessage } from './types'
import { contentToText } from './types'

export interface Source {
  url: string
  title: string
}

/** The numbered `title` / `url` entries every search backend's result text
 * uses ("1. Title\n   https://..."). */
export function parseSearchSources(text: string): Source[] {
  const out: Source[] = []
  const lines = text.split(/\r?\n/)
  for (let i = 0; i < lines.length - 1; i++) {
    const head = /^\s*\d+\.\s+(.+?)\s*$/.exec(lines[i])
    const link = /^\s+(https?:\/\/\S+)\s*$/.exec(lines[i + 1])
    if (head && link) out.push({ title: head[1], url: link[1] })
  }
  return out
}

/**
 * Which message each turn's web-search sources belong under: the last
 * assistant reply with text in a turn (the stretch after a user message),
 * carrying every source the searches in that turn returned.
 */
export function sourcesByMessage(messages: AgentMessage[]): Map<number, Source[]> {
  const result = new Map<number, Source[]>()
  let sources: Source[] = []
  let last: number | null = null
  const flush = () => {
    if (last !== null && sources.length) result.set(last, sources)
    sources = []
    last = null
  }
  messages.forEach((m, i) => {
    if (m.type === 'user') flush()
    else if (m.type === 'assistant') {
      if (contentToText(m.content).trim()) last = i
    } else if (m.type === 'toolResult' && m.toolName === 'web_search' && m.isError !== true) {
      for (const s of parseSearchSources(contentToText(m.content))) {
        if (!sources.some((x) => x.url === s.url)) sources.push(s)
      }
    }
  })
  flush()
  return result
}
