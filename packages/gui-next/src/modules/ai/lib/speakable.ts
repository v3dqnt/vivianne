/**
 * A reply as it should sound: prose, not markup. Code and tables are left
 * out (read aloud they are noise), links keep their words, and headings and
 * list items become sentences.
 */
export function toSpeakable(markdown: string): string {
  return (
    markdown
      // Fenced and indented-by-fence code.
      .replace(/```[\s\S]*?```/g, ' ')
      .replace(/~~~[\s\S]*?~~~/g, ' ')
      // Tables.
      .replace(/^\s*\|.*\|\s*$/gm, ' ')
      .replace(/^\s*[-: |]{3,}\s*$/gm, ' ')
      // Images vanish; links keep their text.
      .replace(/!\[[^\]]*\]\([^)]*\)/g, ' ')
      .replace(/\[([^\]]+)\]\([^)]*\)/g, '$1')
      // Inline code keeps its words.
      .replace(/`([^`]+)`/g, '$1')
      // Emphasis, headings, quotes, list markers.
      .replace(/(\*\*|__)(.*?)\1/g, '$2')
      .replace(/(\*|_)(.*?)\1/g, '$2')
      .replace(/^\s{0,3}#{1,6}\s+(.*)$/gm, '$1.')
      .replace(/^\s*>\s?/gm, '')
      .replace(/^\s*(?:[-*+]|\d+[.)])\s+/gm, '')
      // Bare URLs.
      .replace(/https?:\/\/\S+/g, ' ')
      .replace(/\s*\n\s*/g, '. ')
      .replace(/\.\s*\./g, '.')
      .replace(/\s{2,}/g, ' ')
      .replace(/^[\s.]+/, '')
      .trim()
  )
}
