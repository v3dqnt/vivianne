import { Component, type ReactNode } from 'react'
import { Typography } from '../ui'

/** Catches a component that can't draw here and says why, instead of blanking the page. */
export class Boundary extends Component<{ children: ReactNode }, { error: string | null }> {
  state = { error: null as string | null }
  static getDerivedStateFromError(e: unknown) {
    return { error: e instanceof Error ? e.message : String(e) }
  }
  render() {
    return this.state.error ? (
      <Typography sx={{ fontSize: '0.75rem', color: 'warning.main', p: '8px' }}>Needs props or a backend: {this.state.error}</Typography>
    ) : (
      this.props.children
    )
  }
}
