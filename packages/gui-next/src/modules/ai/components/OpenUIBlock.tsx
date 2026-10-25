import { Component, lazy, Suspense, type ReactNode } from 'react'
import { Box, Typography } from '../../../ui'
import { CodeBlock } from './CodeBlock'

const OpenUIRender = lazy(() => import('./OpenUIRender'))

/** A program that throws while rendering shows its source rather than
 * taking the message down with it. */
class Fallback extends Component<{ code: string; children: ReactNode }, { failed: boolean }> {
  state = { failed: false }
  static getDerivedStateFromError() {
    return { failed: true }
  }
  render() {
    if (!this.state.failed) return this.props.children
    return (
      <Box>
        <Typography variant="caption" color="text.secondary">
          This card couldn't be drawn; here is what was sent.
        </Typography>
        <CodeBlock code={this.props.code} lang="text" />
      </Box>
    )
  }
}

/** A ```openui-lang fence: an interactive OpenUI card (see OpenUIRender). */
export function OpenUIBlock({ code }: { code: string }) {
  return (
    <Fallback code={code}>
      <Suspense fallback={<Box sx={{ height: 120, borderRadius: 2, bgcolor: 'action.hover', my: 1 }} />}>
        <OpenUIRender code={code} streaming={false} />
      </Suspense>
    </Fallback>
  )
}
