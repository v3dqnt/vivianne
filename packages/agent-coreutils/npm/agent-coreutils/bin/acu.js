#!/usr/bin/env node
'use strict'
const { spawnSync } = require('child_process')
const { findBinary, platformKey } = require('../platform.js')

const bin = findBinary()
if (!bin) {
  process.stderr.write(
    `acu: no prebuilt binary for ${platformKey()}.\n` +
      '  Reinstall with optional dependencies enabled (npm i -g agent-coreutils), set ACU_BINARY to a binary,\n' +
      '  or build from source: https://github.com/agent-coreutils/agent-coreutils\n',
  )
  process.exit(2)
}

const res = spawnSync(bin, process.argv.slice(2), { stdio: 'inherit', windowsHide: true })
if (res.error) {
  process.stderr.write(`acu: could not run ${bin}: ${res.error.message}\n`)
  process.exit(2)
}
if (res.signal) process.kill(process.pid, res.signal)
process.exit(res.status === null ? 1 : res.status)
