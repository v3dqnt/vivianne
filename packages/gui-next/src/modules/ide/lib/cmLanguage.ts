import type { LanguageSupport } from '@codemirror/language'

/**
 * Path → CodeMirror language extension, loaded lazily.
 *
 * Lazy because each `@codemirror/lang-*` package pulls in its own parser
 * (Lezer grammars aren't tiny), and most sessions only ever open files in one
 * or two of these languages — importing all nine eagerly would tax every IDE
 * tab's first paint for languages that particular file isn't even in.
 *
 * Falls back to no language extension (plain text, still editable) for an
 * extension none of these cover, rather than guessing wrong.
 */
export async function languageFor(path: string): Promise<LanguageSupport | null> {
  const ext = path.split('.').pop()?.toLowerCase() ?? ''
  switch (ext) {
    case 'ts':
    case 'tsx':
    case 'mts':
    case 'cts': {
      const { javascript } = await import('@codemirror/lang-javascript')
      return javascript({ typescript: true, jsx: ext === 'tsx' })
    }
    case 'js':
    case 'jsx':
    case 'mjs':
    case 'cjs': {
      const { javascript } = await import('@codemirror/lang-javascript')
      return javascript({ jsx: ext === 'jsx' })
    }
    case 'py': {
      const { python } = await import('@codemirror/lang-python')
      return python()
    }
    case 'rs': {
      const { rust } = await import('@codemirror/lang-rust')
      return rust()
    }
    case 'json': {
      const { json } = await import('@codemirror/lang-json')
      return json()
    }
    case 'html': {
      const { html } = await import('@codemirror/lang-html')
      return html()
    }
    case 'css': {
      const { css } = await import('@codemirror/lang-css')
      return css()
    }
    case 'md':
    case 'mdx': {
      const { markdown } = await import('@codemirror/lang-markdown')
      return markdown()
    }
    case 'yml':
    case 'yaml': {
      const { yaml } = await import('@codemirror/lang-yaml')
      return yaml()
    }
    case 'sql': {
      const { sql } = await import('@codemirror/lang-sql')
      return sql()
    }
    default:
      return null
  }
}
