import { useEffect, useRef } from 'react'
import { Box, useTheme } from '../../../ui'
import { EditorState, type Extension } from '@codemirror/state'
import { EditorView, keymap, lineNumbers, highlightActiveLine, highlightActiveLineGutter } from '@codemirror/view'
import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands'
import { syntaxHighlighting, defaultHighlightStyle, indentOnInput, bracketMatching } from '@codemirror/language'
import { searchKeymap, highlightSelectionMatches } from '@codemirror/search'
import { closeBrackets, closeBracketsKeymap, autocompletion, completionKeymap } from '@codemirror/autocomplete'
import { languageFor } from '../lib/cmLanguage'

/** Same surfaces `CodeBlock`'s Shiki themes use (`github-dark-default` /
 * `github-light`) — a CodeMirror-based editor sitting next to Shiki-rendered
 * read-only snippets elsewhere in the app should not look like a visitor
 * from a different app. */
const DARK = { bg: '#0d1117', fg: '#c9d1d9', gutter: '#6e7681', selection: 'rgba(56,139,253,0.25)', activeLine: 'rgba(110,118,129,0.1)' }
const LIGHT = { bg: '#ffffff', fg: '#1f2328', gutter: '#8c959f', selection: 'rgba(84,174,255,0.25)', activeLine: 'rgba(175,184,193,0.1)' }

function paletteFor(mode: 'light' | 'dark') {
  return mode === 'dark' ? DARK : LIGHT
}

function editorTheme(mode: 'light' | 'dark'): Extension {
  const p = paletteFor(mode)
  return EditorView.theme(
    {
      '&': { color: p.fg, backgroundColor: p.bg, height: '100%', fontSize: '13px' },
      '.cm-content': { fontFamily: '"Geist Mono", ui-monospace, monospace', caretColor: p.fg },
      '.cm-gutters': { backgroundColor: p.bg, color: p.gutter, border: 'none' },
      '.cm-activeLine': { backgroundColor: p.activeLine },
      '.cm-activeLineGutter': { backgroundColor: p.activeLine },
      '.cm-selectionBackground, &.cm-focused .cm-selectionBackground': { backgroundColor: `${p.selection} !important` },
      '.cm-scroller': { overflow: 'auto' },
      '&.cm-focused': { outline: 'none' },
    },
    { dark: mode === 'dark' },
  )
}

/**
 * A CodeMirror 6 editor bound to one file's draft text.
 *
 * Deliberately NOT a fully-controlled component (value prop re-applied every
 * render) — CodeMirror keeps its own document and emits changes via
 * `onChange`; feeding `value` back in on every keystroke would fight the
 * editor's own cursor/selection state and stutter on fast typing. The `path`
 * changing is what tells this component "this is a different file, remount
 * as a fresh document" via the `key` prop the caller applies (see
 * `IdeView.tsx`), not a `value` diff here.
 *
 * `readOnly` is used for the loading/error placeholder in `IdeView` rather
 * than mounting this with an empty string draft, but is exposed for reuse.
 */
export function CodeEditor({
  path,
  initialContent,
  onChange,
  onSave,
  readOnly = false,
}: {
  path: string
  initialContent: string
  onChange: (content: string) => void
  /** Ctrl/Cmd+S inside the editor. Handled here (not just at the panel level)
   * so the shortcut works even while the editor has focus and would
   * otherwise swallow the keydown before it reaches anything else. */
  onSave: () => void
  readOnly?: boolean
}) {
  const theme = useTheme()
  const hostRef = useRef<HTMLDivElement | null>(null)
  const viewRef = useRef<EditorView | null>(null)
  // Held in refs so the extension-building effect (which only depends on
  // `path`/theme/readOnly) always calls the LATEST callback, without having
  // to tear down and rebuild the whole editor state every time a parent
  // re-render hands down a new function identity.
  const onChangeRef = useRef(onChange)
  onChangeRef.current = onChange
  const onSaveRef = useRef(onSave)
  onSaveRef.current = onSave

  useEffect(() => {
    if (!hostRef.current) return
    let cancelled = false
    let view: EditorView | null = null

    void languageFor(path).then((lang) => {
      if (cancelled || !hostRef.current) return
      const extensions: Extension[] = [
        lineNumbers(),
        highlightActiveLineGutter(),
        highlightActiveLine(),
        history(),
        indentOnInput(),
        bracketMatching(),
        closeBrackets(),
        autocompletion(),
        highlightSelectionMatches(),
        syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
        editorTheme(theme.palette.mode),
        EditorView.lineWrapping,
        EditorState.readOnly.of(readOnly),
        keymap.of([
          {
            key: 'Mod-s',
            run: () => {
              onSaveRef.current()
              return true
            },
          },
          indentWithTab,
          ...closeBracketsKeymap,
          ...defaultKeymap,
          ...searchKeymap,
          ...historyKeymap,
          ...completionKeymap,
        ]),
        EditorView.updateListener.of((update) => {
          if (update.docChanged) onChangeRef.current(update.state.doc.toString())
        }),
      ]
      if (lang) extensions.push(lang)

      view = new EditorView({
        state: EditorState.create({ doc: initialContent, extensions }),
        parent: hostRef.current,
      })
      viewRef.current = view
    })

    return () => {
      cancelled = true
      view?.destroy()
      viewRef.current = null
    }
    // `initialContent` is intentionally excluded: it seeds the document once
    // at mount. The caller remounts this component (via `key={path}`) for a
    // different file, and a reload/external-change replaces the tab's
    // content through `IdeView`'s own remount of the same key, not by
    // reacting to this prop changing under a stable mount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [path, theme.palette.mode, readOnly])

  return <Box ref={hostRef} sx={{ height: '100%', minWidth: 0, minHeight: 0, overflow: 'hidden' }} />
}
