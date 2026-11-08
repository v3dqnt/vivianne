import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { fileIconName, type IconManifest } from './fileIcons.ts'

const manifest = JSON.parse(readFileSync(new URL('./fileIconManifest.json', import.meta.url), 'utf8')) as IconManifest

test('each language gets its own icon', () => {
  const icons = ['main.rs', 'app.ts', 'App.tsx', 'index.js', 'script.py', 'main.go', 'Main.java', 'lib.c', 'x.cpp', 'Prog.cs', 'a.rb', 'i.php', 'v.swift', 'k.kt', 'q.sql', 's.sh', 'p.ps1', 'README.md', 'data.json', 'c.yaml', 'i.html', 'st.css'].map((n) => fileIconName(manifest, n))
  assert.equal(new Set(icons).size, icons.length, `duplicates in ${icons.join(', ')}`)
  assert.ok(icons.every((i) => i !== manifest.file), 'none fall back to the generic file')
})

test('exact file names and longer extensions win', () => {
  assert.notEqual(fileIconName(manifest, 'package.json'), fileIconName(manifest, 'data.json'))
  assert.equal(fileIconName(manifest, '.gitignore'), fileIconName(manifest, '.GITIGNORE'))
  assert.notEqual(fileIconName(manifest, 'types.d.ts'), fileIconName(manifest, 'types.ts'))
  assert.equal(fileIconName(manifest, 'mystery.zzqq'), manifest.file)
  assert.equal(fileIconName(manifest, 'Makefile'), manifest.names['makefile'])
})

test('folders go by name, with an open form', () => {
  assert.notEqual(fileIconName(manifest, 'src', { dir: true }), manifest.folder)
  assert.notEqual(fileIconName(manifest, 'src', { dir: true, open: true }), fileIconName(manifest, 'src', { dir: true }))
  assert.equal(fileIconName(manifest, 'zz-unknown-folder', { dir: true }), manifest.folder)
  assert.equal(fileIconName(manifest, 'zz-unknown-folder', { dir: true, open: true }), manifest.folderOpen)
})
