import { test } from 'node:test'
import assert from 'node:assert/strict'
import { rankByName } from './rankByName.ts'
import { SLASH_COMMANDS, matchCommands, parseSlash, slashAt, type SlashContext } from './slashCommands.ts'

test('ranking puts exact, then prefix, then substring, then scattered letters; ties keep order', () => {
  const names = ['model', 'plan', 'permission', 'stop', 'goal']
  assert.deepEqual(rankByName(names, 'p', (n) => n), ['plan', 'permission', 'stop'])
  assert.deepEqual(rankByName(names, 'plan', (n) => n), ['plan'])
  assert.deepEqual(rankByName(names, 'pms', (n) => n), ['permission'])
  assert.deepEqual(rankByName(names, 'zzz', (n) => n), [])
  assert.deepEqual(rankByName(names, '', (n) => n), names)
  assert.deepEqual(rankByName(['xmodel', 'model', 'modelx'], 'model', (n) => n), ['model', 'modelx', 'xmodel'])
})

test('a command word is only recognised at the start of the message with the cursor inside it', () => {
  assert.deepEqual(slashAt('/pl'), { start: 0, end: 3, query: 'pl' })
  assert.deepEqual(slashAt('/'), { start: 0, end: 1, query: '' })
  assert.equal(slashAt('hello /plan'), null)
  assert.equal(slashAt('/plan on', 8), null)
  assert.deepEqual(slashAt('/plan on', 3), { start: 0, end: 5, query: 'plan' })
})

test('only registered commands parse; paths and unknown words are ordinary text', () => {
  assert.equal(parseSlash('/etc/hosts'), null)
  assert.equal(parseSlash('/nope arg'), null)
  assert.equal(parseSlash('hello'), null)
  const p = parseSlash('/goal  ship the release ')
  assert.equal(p?.command.name, 'goal')
  assert.equal(p?.arg, 'ship the release')
  assert.equal(parseSlash('/STOP')?.command.name, 'stop')
  assert.equal(matchCommands('mo')[0].name, 'model')
})

function fake(initial: 'ask' | 'auto' | 'plan' = 'ask') {
  const log: string[] = []
  let mode = initial
  const ctx: SlashContext = {
    mode: () => mode,
    setPlanMode: async (on) => {
      mode = on ? 'plan' : 'ask'
      log.push(`plan:${on}`)
    },
    setAutoApprove: async (on) => {
      mode = on ? 'auto' : 'ask'
      log.push(`auto:${on}`)
    },
    goal: async (a, t) => {
      log.push(`goal:${a}:${t ?? ''}`)
    },
    stop: async () => void log.push('stop'),
    openModelPicker: () => void log.push('picker'),
    notice: (t) => void log.push(`say:${t}`),
  }
  return { ctx, log }
}

const run = (name: string, ctx: SlashContext, arg = '') => SLASH_COMMANDS.find((c) => c.name === name)!.run(ctx, arg)

test('/plan toggles, and honours on and off', async () => {
  const a = fake('ask')
  await run('plan', a.ctx)
  assert.deepEqual(a.log, ['plan:true', 'say:Plan mode is on.'])
  const b = fake('plan')
  await run('plan', b.ctx)
  assert.equal(b.log[0], 'plan:false')
  const c = fake('plan')
  await run('plan', c.ctx, 'on')
  assert.equal(c.log[0], 'plan:true')
  const d = fake()
  await run('plan', d.ctx, 'sideways')
  assert.match(d.log[0], /^say:Use \/plan/)
})

test('/permission switches modes and says the current one when the argument is missing', async () => {
  const a = fake('plan')
  await run('permission', a.ctx, 'auto')
  assert.deepEqual(a.log, ['plan:false', 'auto:true', 'say:Permission mode: auto.'])
  const b = fake('auto')
  await run('permission', b.ctx, '')
  assert.match(b.log[0], /Current mode: auto/)
})

test('/goal creates, falls back to editing, and routes the words', async () => {
  const a = fake()
  await run('goal', a.ctx, 'ship it')
  assert.deepEqual(a.log, ['goal:create:ship it'])
  const b = fake()
  b.ctx.goal = async (act, t) => {
    b.log.push(`goal:${act}:${t ?? ''}`)
    if (act === 'create') throw new Error('exists')
  }
  await run('goal', b.ctx, 'ship it')
  assert.deepEqual(b.log, ['goal:create:ship it', 'goal:edit:ship it'])
  const c = fake()
  await run('goal', c.ctx, 'done')
  assert.deepEqual(c.log, ['goal:complete:'])
  const d = fake()
  await run('goal', d.ctx, '')
  assert.match(d.log[0], /^say:/)
})
