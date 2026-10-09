'use strict'
// postinstall: if npm skipped the optional platform package (--no-optional, an unusual platform, a proxy that
// blocked it), download the matching binary from the GitHub release instead. Never fails the install.
const fs = require('fs')
const https = require('https')
const path = require('path')
const { findBinary, platformKey, exeName } = require('./platform.js')

if (findBinary()) process.exit(0)

const pkg = require('./package.json')
const key = platformKey()
const url = `${pkg.acu.releaseBase}/v${pkg.version}/acu-${key}${process.platform === 'win32' ? '.exe' : ''}`
const dest = path.join(__dirname, 'vendor', exeName)

function get(u, redirects) {
  https
    .get(u, { headers: { 'User-Agent': 'agent-coreutils-install' } }, (res) => {
      if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location && redirects < 5) {
        res.resume()
        return get(res.headers.location, redirects + 1)
      }
      if (res.statusCode !== 200) {
        res.resume()
        return skip(`HTTP ${res.statusCode} from ${u}`)
      }
      fs.mkdirSync(path.dirname(dest), { recursive: true })
      const tmp = `${dest}.download`
      const out = fs.createWriteStream(tmp)
      res.pipe(out)
      out.on('finish', () => {
        out.close(() => {
          fs.renameSync(tmp, dest)
          if (process.platform !== 'win32') fs.chmodSync(dest, 0o755)
        })
      })
      out.on('error', (e) => skip(e.message))
    })
    .on('error', (e) => skip(e.message))
}

function skip(why) {
  process.stderr.write(`agent-coreutils: could not fetch a binary for ${key} (${why}).\n  Set ACU_BINARY or build from source.\n`)
  process.exit(0)
}

get(url, 0)
