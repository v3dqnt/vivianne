# Vivianne

A desktop AI coding agent, built with [Tauri 2](https://tauri.app) (Rust backend) and React + Vite (frontend, in `packages/gui-next`).

## Prerequisites

- **Rust** (stable) — https://rustup.rs
- **Node.js** 18+ and npm
- **Windows**: Visual Studio 2017 or newer, **or** the standalone "Desktop development with C++" Build Tools (the MSVC toolchain). The `npm run tauri` script locates whatever version is installed automatically via `vswhere` — no hardcoded path.
- **macOS**: Xcode Command Line Tools.
- **Linux**: the usual Tauri system deps (`webkit2gtk`, `libsecret` for credential storage, etc. — see the Tauri prerequisites page).

## Develop

```bash
npm install
npm run tauri dev
```

This launches the Rust app with the Vite dev server (`packages/gui-next`) and hot reload.

## Build a release bundle

```bash
npm run tauri build
```

The installer/bundle is written to `src-tauri/target/release/bundle/` (e.g. an `.msi`/`.exe` on Windows, `.dmg` on macOS, `.deb`/`.AppImage` on Linux).

## API keys & credential storage

API keys are stored in the **OS credential store** (Windows Credential Manager, macOS Keychain, or libsecret on Linux) — never in plaintext on disk. If the OS credential backend is unavailable (e.g. a headless environment), the app falls back to a local file and logs a warning; keys are then not encrypted.

Add keys in-app via **Settings → API Keys**.

## Code signing (not configured for v1)

Release builds are currently **unsigned**. On first launch users will see a Windows SmartScreen or macOS Gatekeeper warning; they can proceed past it. To sign, add a Windows code-signing certificate or Apple Developer credentials to `src-tauri/tauri.conf.json`'s `bundle` config and set the appropriate signing env vars.

## Auto-updates

The app checks GitHub Releases for updates on launch (and via **Settings → Usage → App updates**). Before this works you must:

1. Create the GitHub repository and replace `YOUR_GITHUB_USERNAME/vivianne` in `src-tauri/tauri.conf.json`'s `plugins.updater.endpoints` with the real `owner/repo`.
2. Keep the updater **private signing key** (generated at `~/.vivianne-updater.key`) secret — it is **not** in this repo. Set `TAURI_SIGNING_PRIVATE_KEY` (and password, if any) when running `npm run tauri build` so release artifacts get signed.
3. Publish each release's bundle **plus** the generated `latest.json` to GitHub Releases.

Losing the private key means you can no longer ship updates that existing installs will accept — back it up securely.
