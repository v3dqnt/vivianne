'use strict'
// Shared by bin/acu.js and install.js: which prebuilt binary belongs to this machine, and where it is.
const fs = require('fs')
const path = require('path')

function platformKey() {
  const { platform, arch } = process
  if (platform === 'linux') {
    let musl = false
    try {
      const header = process.report && process.report.getReport().header
      musl = !(header && header.glibcVersionRuntime)
    } catch {
      musl = false
    }
    return `linux-${arch}-${musl ? 'musl' : 'gnu'}`
  }
  return `${platform}-${arch}`
}

const exeName = process.platform === 'win32' ? 'acu.exe' : 'acu'

// Returns the path of a usable binary, or null. Order: $ACU_BINARY, the platform package, a downloaded copy.
function findBinary() {
  if (process.env.ACU_BINARY && fs.existsSync(process.env.ACU_BINARY)) return process.env.ACU_BINARY
  const key = platformKey()
  try {
    const pkgJson = require.resolve(`@agent-coreutils/${key}/package.json`)
    const bin = path.join(path.dirname(pkgJson), 'bin', exeName)
    if (fs.existsSync(bin)) return bin
  } catch {
    // not installed: fall through
  }
  const vendored = path.join(__dirname, 'vendor', exeName)
  if (fs.existsSync(vendored)) return vendored
  return null
}

module.exports = { platformKey, exeName, findBinary }
