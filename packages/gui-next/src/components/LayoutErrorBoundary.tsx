import { Component, type ReactNode } from 'react'
import { Stack, Typography, Button } from '../ui'
import { forgetSavedLayout } from '../modules/workspace/layout'

/**
 * Catches a render crash in the panel workspace and falls back to a
 * recoverable screen instead of a permanent blank window.
 *
 * The workspace restores a layout persisted in `localStorage` on every
 * launch. Without this boundary, a panel that crashes on mount — or a saved
 * layout that no longer restores — would blank the ENTIRE app on every future
 * startup: the controls that would fix the layout live inside the tree that
 * just failed to render, so there is no UI left to click. A React error
 * boundary is the only thing that can catch a render exception at all; a
 * try/catch in a function component cannot.
 */
export class LayoutErrorBoundary extends Component<{ children: ReactNode }, { error: Error | null }> {
  state: { error: Error | null } = { error: null }

  static getDerivedStateFromError(error: Error) {
    return { error }
  }

  componentDidCatch(error: Error) {
    // Recovery is an explicit click below rather than done here. Changing
    // app state during the commit React uses to show this fallback can
    // unmount the boundary before the fallback paints — which once made a
    // crash look like a button that silently did nothing.
    console.error('[layout] workspace crashed:', error)
  }

  private recover = () => {
    // The saved layout is what a fresh launch restores before this component
    // ever mounts, so resetting only in-memory state would let this same
    // crash greet the user again on their next restart.
    forgetSavedLayout()
    window.location.reload()
  }

  render() {
    if (this.state.error) {
      return (
        <Stack sx={{ height: '100%', alignItems: 'center', justifyContent: 'center', gap: 1.5, p: 4, bgcolor: 'background.default' }}>
          <Typography variant="body1">The panel layout hit an error.</Typography>
          <Typography variant="caption" color="text.secondary" sx={{ fontFamily: '"Geist Mono", ui-monospace, monospace', maxWidth: 480, textAlign: 'center' }}>
            {this.state.error.message}
          </Typography>
          <Button variant="contained" size="small" onClick={this.recover} sx={{ textTransform: 'none', mt: 1 }}>
            Reset the layout
          </Button>
        </Stack>
      )
    }
    return this.props.children
  }
}
