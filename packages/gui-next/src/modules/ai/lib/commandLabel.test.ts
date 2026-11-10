import { test } from 'node:test'
import assert from 'node:assert/strict'
import { commandLabel } from './commandLabel.ts'

test('a leading cd into a folder is dropped, quoted or not', () => {
  assert.equal(commandLabel('cd "C:\\Users\\me\\My Project" && git diff HEAD -- src/App.tsx'), 'git diff HEAD -- src/App.tsx')
  assert.equal(commandLabel("cd '/home/me/app' && npm test"), 'npm test')
  assert.equal(commandLabel('cd packages/gui; cargo check'), 'cargo check')
})

test('everything else is left as written', () => {
  assert.equal(commandLabel('git status'), 'git status')
  assert.equal(commandLabel('npm run build && cd dist'), 'npm run build && cd dist')
  assert.equal(commandLabel('cd src'), 'cd src', 'a bare cd is the whole command')
  assert.equal(commandLabel('cd "a" && '), 'cd "a" &&', 'nothing after the cd: show what was typed')
})
