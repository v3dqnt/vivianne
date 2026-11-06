import { memo } from 'react'

/**
 * The Vivianne mark (the tesseract), as a rounded tile. Every agent shares it;
 * the agent's colour is a thin ring around the tile so they stay tellable apart.
 */
function AgentMarkImpl({ color, size = 24, title }: { color: string; size?: number; title?: string }) {
  return (
    <img
      src="/logo.svg"
      width={size}
      height={size}
      alt={title ?? ''}
      aria-hidden={title ? undefined : true}
      draggable={false}
      style={{ display: 'block', flexShrink: 0, borderRadius: '22%', boxShadow: `0 0 0 1px ${color}` }}
    />
  )
}

export const AgentMark = memo(AgentMarkImpl)
