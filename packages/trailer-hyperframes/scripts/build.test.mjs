import { test } from 'node:test'
import assert from 'node:assert/strict'
import { buildIndex, layout } from './build.mjs'

const manifest = {
  fps: 30,
  width: 1920,
  height: 1080,
  scenes: [
    { id: 's1-intro', src: 'compositions/s1-intro.html', duration: 7, footage: 'assets/footage/city.mp4' },
    { id: 's2-laptop', src: 'compositions/s2-laptop.html', duration: 6, footage: 'assets/footage/laptop.mp4' },
    { id: 's3-agents', src: 'compositions/s3-agents.html', duration: 17 },
    { id: 's4-title-work', src: 'compositions/title-card.html', duration: 4, variables: { line: "Now let’s get to work, shall we?" } },
    { id: 's5-code', src: 'compositions/s5-code.html', duration: 18 },
    { id: 's6-title-buddies', src: 'compositions/title-card.html', duration: 4, variables: { line: 'Wanna destroy some bugs with your buddies?' } },
    { id: 's7-multiplayer', src: 'compositions/s7-multiplayer.html', duration: 14 },
  ],
}
const compsExist = (p) => p.startsWith('compositions/')
const count = (s, needle) => s.split(needle).length - 1
const hostTag = (html, id) => html.match(new RegExp(`<div[^>]*data-composition-id="${id}"[^>]*>`))?.[0] ?? ''

test('layout makes scenes contiguous from their durations', () => {
  assert.deepEqual(layout(manifest).map((s) => s.start), [0, 7, 13, 30, 34, 52, 56])
})

test('no footage files: no video clips, every scene told hasFootage false', () => {
  const html = buildIndex(manifest, { exists: compsExist })
  assert.equal(count(html, '<video'), 0)
  for (const s of manifest.scenes) assert.match(hostTag(html, s.id), /&quot;hasFootage&quot;:false/)
})

test('city footage present: one muted video trimmed to scene 1, scene 1 hasFootage true', () => {
  const html = buildIndex(manifest, { exists: (p) => compsExist(p) || p === 'assets/footage/city.mp4' })
  assert.equal(count(html, '<video'), 1)
  const video = html.match(/<video[^>]*>/)[0]
  assert.match(video, /src="assets\/footage\/city.mp4"/)
  assert.match(video, /\bmuted\b/)
  assert.match(video, /data-start="0"/)
  assert.match(video, /data-duration="7"/)
  assert.match(hostTag(html, 's1-intro'), /&quot;hasFootage&quot;:true/)
  assert.match(hostTag(html, 's2-laptop'), /&quot;hasFootage&quot;:false/)
})

test('missing composition file throws naming the file', () => {
  assert.throws(() => buildIndex(manifest, { exists: (p) => p !== 'compositions/s5-code.html' }), /compositions\/s5-code\.html/)
})

test('title-card hosts carry their line', () => {
  const html = buildIndex(manifest, { exists: compsExist })
  assert.match(hostTag(html, 's4-title-work'), /Now let’s get to work, shall we\?/)
  assert.match(hostTag(html, 's6-title-buddies'), /Wanna destroy some bugs with your buddies\?/)
})

test('root duration is the sum of scene durations', () => {
  const html = buildIndex(manifest, { exists: compsExist })
  assert.match(hostTag(html, 'main'), /data-duration="70"/)
})
