import { memo, useMemo } from 'react'
import katex from 'katex'
// \ce{...} and \pu{...}: chemical equations and units inside $...$. Registers
// itself on the one KaTeX this app ships.
import 'katex/contrib/mhchem'

/**
 * A `$...$` or `$$...$$` formula, typeset by KaTeX.
 *
 * remark-math hands the chat these as code nodes; rendering them here with the
 * app's own KaTeX (rather than through rehype-katex, which resolves its own
 * copy of the library) keeps the markup, the stylesheet and the mhchem
 * extension all on one version. `throwOnError: false` shows a malformed
 * formula as red source text instead of failing the message.
 */
export const MathSpan = memo(function MathSpan({ tex, display }: { tex: string; display: boolean }) {
  const html = useMemo(() => katex.renderToString(tex, { throwOnError: false, strict: 'ignore', displayMode: display }), [tex, display])
  return display ? <div dangerouslySetInnerHTML={{ __html: html }} /> : <span dangerouslySetInnerHTML={{ __html: html }} />
})
