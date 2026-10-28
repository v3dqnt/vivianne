import test from 'node:test'
import assert from 'node:assert/strict'
import { closeTab, hostOf, titleOf, type BrowserTab } from './tabs.ts'

const web = (id: string): BrowserTab => ({ id, kind: 'web', url: 'https://a.test', title: 'a.test', webviewLabel: `live_browser_${id}` })
const file = (id: string): BrowserTab => ({ id, kind: 'file', path: 'C:\\w\\a.png', title: 'a.png' })

const tabs = [web('1'), web('2'), web('3')]

test('closing the active tab activates its right neighbour', () => {
  assert.deepEqual(closeTab(tabs, '2', '2'), { tabs: [tabs[0], tabs[2]], activeId: '3' })
})

test('closing the active last tab activates its left neighbour', () => {
  assert.deepEqual(closeTab(tabs, '3', '3'), { tabs: [tabs[0], tabs[1]], activeId: '2' })
})

test('closing an inactive tab leaves the active one alone', () => {
  assert.deepEqual(closeTab(tabs, '1', '3'), { tabs: [tabs[0], tabs[1]], activeId: '1' })
})

test('closing the only tab leaves nothing active', () => {
  assert.deepEqual(closeTab([file('1')], '1', '1'), { tabs: [], activeId: null })
})

test('titleOf prefers the page title, then the host, then the stored title', () => {
  assert.equal(titleOf(web('1'), 'https://a.test/x', 'Page'), 'Page')
  assert.equal(titleOf(web('1'), 'https://a.test/x', ''), 'a.test')
  assert.equal(titleOf(web('1'), null, null), 'a.test')
})

test('titleOf uses the file name for file tabs', () => {
  assert.equal(titleOf(file('1'), null, null), 'a.png')
})

test('hostOf reads a host, or the raw text when there is none', () => {
  assert.equal(hostOf('https://a.test/x'), 'a.test')
  assert.equal(hostOf('not a url'), 'not a url')
})
