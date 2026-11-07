import { test } from 'node:test'
import assert from 'node:assert/strict'
import { applyFileMention, applySkillMention, cursorAfterMention, formatMention, getSkillMentionQuery, mentionAt } from './mentions.ts'

test('a mention at the end of the text is found, and an email address is not one', () => {
  assert.equal(getSkillMentionQuery('look at @src/ag'), 'src/ag')
  assert.equal(getSkillMentionQuery('@'), '')
  assert.equal(getSkillMentionQuery('mail me at a@b.com'), null)
  assert.equal(getSkillMentionQuery('no mention here'), null)
})

test('a mention in the middle of the text is found from the cursor', () => {
  const text = 'compare @src/a and then explain'
  assert.equal(getSkillMentionQuery(text, 'compare @src/a'.length), 'src/a')
  assert.equal(getSkillMentionQuery(text, text.length), null, 'the cursor at the end is outside the mention')
})

test('a quoted path keeps its spaces and stays open until closed', () => {
  assert.equal(getSkillMentionQuery('see @"My Docs/read'), 'My Docs/read')
  assert.equal(mentionAt('see @"My Docs/read')?.quoted, true)
  assert.equal(formatMention('My Docs/read me.md'), '@"My Docs/read me.md"')
  assert.equal(formatMention('src/app.ts'), '@src/app.ts')
})

test('completing a mention replaces the whole word and keeps what follows', () => {
  const text = 'check @src/ag now'
  const cursor = 'check @src/ag'.length
  assert.equal(applyFileMention(text, 'src/agent/loop.ts', cursor), 'check @src/agent/loop.ts now')
  assert.equal(applyFileMention('open @My', 'My Folder/a.md'), 'open @"My Folder/a.md" ')
  assert.equal(applySkillMention('use @fro', 'frontend-design'), 'use @frontend-design ')
})

test('completing with the cursor inside the word replaces the rest of the word', () => {
  assert.equal(applyFileMention('see @src/ag-old here', 'src/new.ts', 'see @src/ag'.length), 'see @src/new.ts here')
})

test('the cursor lands after the completed mention', () => {
  assert.equal(cursorAfterMention('hi @sr there', 'src/a.ts', 'hi @sr'.length), 'hi @src/a.ts '.length)
})
