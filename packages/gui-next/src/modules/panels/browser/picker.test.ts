import test from 'node:test'
import assert from 'node:assert/strict'
import { commentDraft, type PickResult } from './picker.ts'

const pick: PickResult = {
  selector: 'button.cta',
  tag: 'button',
  text: 'Get started',
  rect: { x: 10, y: 20, width: 100, height: 40 },
  url: 'http://localhost:5173/#hero',
  title: 'Home',
}

test('commentDraft names the element and its page', () => {
  assert.equal(
    commentDraft(pick, 'Make this bigger', null),
    'Comment on button.cta "Get started" — http://localhost:5173/#hero\nMake this bigger',
  )
})

test('commentDraft includes a screenshot path when there is one', () => {
  assert.equal(
    commentDraft(pick, '  Too blue  ', 'C:\\Temp\\viv-comments\\1.png'),
    'Comment on button.cta "Get started" — http://localhost:5173/#hero\nToo blue\nScreenshot: C:\\Temp\\viv-comments\\1.png',
  )
})

test('commentDraft falls back to the selector when the element has no text', () => {
  const out = commentDraft({ ...pick, text: '' }, 'Spacing', null)
  assert.equal(out.split('\n')[0], 'Comment on button.cta — http://localhost:5173/#hero')
})
