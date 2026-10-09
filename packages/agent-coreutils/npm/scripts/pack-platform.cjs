#!/usr/bin/env node
'use strict'
// Builds one binary-only npm package: node npm/scripts/pack-platform.cjs <platform-key> <binary> <out-dir>
// e.g. node npm/scripts/pack-platform.cjs win32-x64 build/acu.exe npm/dist
// The result is <out-dir>/<platform-key>/ (run `npm pack` in it, or `npm publish --access public`).
const fs = require('fs')
const path = require('path')

const [key, binary, outDir] = process.argv.slice(2)
if (!key || !binary || !outDir) {
  console.error('usage: pack-platform.cjs <platform-key> <binary> <out-dir>')
  process.exit(2)
}
const main = require('../agent-coreutils/package.json')
const m = /^(win32|darwin|linux)-(x64|arm64)(?:-(gnu|musl))?$/.exec(key)
if (!m) {
  console.error(`bad platform key '${key}' (examples: win32-x64, darwin-arm64, linux-x64-musl)`)
  process.exit(2)
}
const [, os, cpu, libc] = m
const dir = path.join(outDir, key)
fs.rmSync(dir, { recursive: true, force: true })
fs.mkdirSync(path.join(dir, 'bin'), { recursive: true })
const exe = os === 'win32' ? 'acu.exe' : 'acu'
fs.copyFileSync(binary, path.join(dir, 'bin', exe))
if (os !== 'win32') fs.chmodSync(path.join(dir, 'bin', exe), 0o755)
fs.copyFileSync(path.join(__dirname, '..', '..', 'LICENSE'), path.join(dir, 'LICENSE'))
const pkg = {
  name: `@agent-coreutils/${key}`,
  version: main.version,
  description: `The ${key} binary for agent-coreutils. Install agent-coreutils, not this package.`,
  license: main.license,
  os: [os],
  cpu: [cpu],
  files: ['bin', 'LICENSE'],
}
if (libc) pkg.libc = [libc]
fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify(pkg, null, 2) + '\n')
console.log(`wrote ${dir}`)
