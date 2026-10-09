#!/usr/bin/env node
// Cross-platform entry point for the Tauri CLI.
//
// Both desktop platforms need environment preparation before cargo runs,
// because `whisper-rs-sys` compiles whisper.cpp through CMake and runs bindgen
// (its shipped bindings.rs is generated on Linux and references glibc types,
// so it can never compile on Windows or macOS — bindgen MUST run locally).
// What that preparation IS differs completely per platform:
//
//   Windows — activate the MSVC developer environment so CMake's Ninja
//     generator finds cl.exe/link.exe. Delegated to dev-env.cmd, which locates
//     whatever Visual Studio the machine actually has via vswhere.
//
//   macOS — override the four Windows-specific values that
//     src-tauri/.cargo/config.toml pins in its `[env]` table. Cargo's `[env]`
//     has no cfg support and applies on every platform, but its entries do NOT
//     force: a variable already present in the environment wins. Setting them
//     here is therefore the whole macOS fix, and it leaves the Windows path
//     byte-for-byte unchanged.
//
//   Linux — nothing special; system clang and cmake are found normally, but
//     the same config.toml overrides still have to be neutralised.
import { spawn, spawnSync } from 'node:child_process'
import { existsSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const args = process.argv.slice(2)

if (process.platform === 'win32') {
  // A .cmd file cannot be spawned without a shell on Windows, but
  // `shell: true` concatenates argv into the command line without
  // quoting — and this repo's own path contains a space ("Project
  // Versa"), which split the script path into two tokens and failed with
  // 'C:\Users\...\Project' is not recognized'. Quote every token
  // ourselves and pass one pre-built command string.
  const quoted = [join(here, 'dev-env.cmd'), ...args].map((a) => `"${a}"`).join(' ')
  const child = spawn(quoted, { stdio: 'inherit', shell: true })
  child.on('exit', (code) => process.exit(code ?? 1))
} else {
  process.exit(runUnix())
}

/** Reads a command's trimmed stdout, or null if it isn't runnable. */
function capture(cmd, cmdArgs) {
  const r = spawnSync(cmd, cmdArgs, { encoding: 'utf8' })
  if (r.status !== 0 || !r.stdout) return null
  const out = r.stdout.trim()
  return out.length > 0 ? out : null
}

function runUnix() {
  const env = { ...process.env }

  if (process.platform === 'darwin') {
    // Vivianne supports macOS 10.15 Catalina. Without this, C dependencies
    // (whisper.cpp, sqlite) are built for the build machine's macOS and
    // the app refuses to start on anything older. Apple Silicon's floor is
    // 11.0 regardless, which the toolchain applies by itself.
    env.MACOSX_DEPLOYMENT_TARGET ||= '10.15'
    // libclang.dylib ships inside the Xcode/CLT toolchain. `xcode-select -p`
    // resolves for both a full Xcode and the standalone Command Line Tools.
    if (!env.LIBCLANG_PATH) {
      const devDir = capture('xcode-select', ['-p'])
      const candidates = [
        devDir && join(devDir, 'Toolchains/XcodeDefault.xctoolchain/usr/lib'),
        '/Library/Developer/CommandLineTools/usr/lib',
        '/opt/homebrew/opt/llvm/lib',
        '/usr/local/opt/llvm/lib',
      ].filter(Boolean)
      const hit = candidates.find((p) => existsSync(join(p, 'libclang.dylib')))
      if (hit) env.LIBCLANG_PATH = hit
      else console.warn('[dev-env] libclang.dylib not found — run `xcode-select --install`.')
    }
    // bindgen shells out to clang without the SDK that the driver would
    // normally infer, so stdio.h and friends go missing without this.
    if (!env.BINDGEN_EXTRA_CLANG_ARGS) {
      const sdk = capture('xcrun', ['--sdk', 'macosx', '--show-sdk-path'])
      env.BINDGEN_EXTRA_CLANG_ARGS = sdk ? `-isysroot ${sdk}` : ''
    }
  } else if (!env.BINDGEN_EXTRA_CLANG_ARGS) {
    // Neutralise the Windows include paths from config.toml.
    env.BINDGEN_EXTRA_CLANG_ARGS = ''
  }

  if (!env.CMAKE) {
    const cmake = capture(process.platform === 'darwin' ? 'which' : 'which', ['cmake'])
    if (cmake) env.CMAKE = cmake
    else console.warn('[dev-env] cmake not found on PATH — install it (brew install cmake).')
  }
  // config.toml pins Ninja for a Windows-specific CMake bug. Elsewhere, use it
  // only when it is genuinely installed; otherwise fall back to the generator
  // every Unix has.
  if (!env.CMAKE_GENERATOR) {
    env.CMAKE_GENERATOR = capture('which', ['ninja']) ? 'Ninja' : 'Unix Makefiles'
  }

  // No shell needed: npm puts node_modules/.bin on PATH for lifecycle
  // scripts, and skipping the shell means arguments stay properly
  // separated regardless of spaces in the project path.
  const child = spawn('tauri', args, { stdio: 'inherit', env })
  child.on('exit', (code) => process.exit(code ?? 1))
  return 0
}
