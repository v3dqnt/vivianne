# Version bump checklist

Every place the app version string lives. Bump all of these together, then build/release.

## App repo (`viv-main-gui-exp`)

1. `packages/gui/src-tauri/Cargo.toml` — `version = "x.y.z"` (top of file)
2. `packages/gui/src-tauri/tauri.conf.json` — `"version": "x.y.z"`
3. `packages/gui/package.json` — `"version": "x.y.z"`
4. `CHANGELOG.md` — add a new `## [x.y.z] — YYYY-MM-DD` section at the top (below the `# Changelog` header, above the previous entry)

Not version-tracked (leave alone — independent/auto-generated):
- Root `package.json` (`"version": "0.0.3"`) — unrelated workspace placeholder, doesn't follow the app version
- `packages/gui-next/package.json` (`"version": "0.0.0"`) — same
- `package-lock.json` files — regenerate automatically, never hand-edit

## Site repo (`vivianne-site`)

5. `src/app/downloads/page.tsx` — the `builds` array: both `href` URLs (`.../releases/download/vX.Y.Z/Vivianne_X.Y.Z_x64-setup.exe` and the `.msi` one) and both `size` labels (actual file size after building). Also the hero `<DownloadLink href=...>` — same exe URL, duplicated.
   - The hero copy ("v1.7, for Windows") only needs updating on a **minor** version bump (1.7 → 1.8), not a patch bump (1.7.0 → 1.7.2) — it intentionally doesn't carry the patch digit.
6. `src/app/changelog/page.tsx` — the `releases` array is grouped by **minor** version, not patch. A patch release (1.7.0 → 1.7.2) adds bullet points to the existing `id: "v1-7"` entry's `notes` array — it does not get its own accordion row. Only a minor bump (1.8.0) gets a new entry.

## GitHub release convention

- Releases are published on **`v3dqnt/Vivianne-site`** (the site repo), not `v3dqnt/vivianne` (the app source repo) — matches the existing v1.0.0/v1.5.0/v1.7.0 releases.
- Tag/title: `vX.Y.Z`
- Assets: `Vivianne_X.Y.Z_x64-setup.exe` (NSIS) and `Vivianne_X.Y.Z_x64_en-US.msi` (MSI), built from `packages/gui/src-tauri/target/release/bundle/`
- `targetCommitish: main`
- Body: condensed markdown changelog for that version

## Order of operations

1. Bump all 4 app-repo locations above.
2. Add the CHANGELOG.md entry.
3. Commit + push the app repo.
4. Build the release bundle (`cargo tauri build` from `packages/gui`, or the Tauri build task) — produces the `.exe`/`.msi` under `src-tauri/target/release/bundle/`.
5. Create the GitHub release on the **site** repo with both installer assets.
6. Update the site repo's `downloads/page.tsx` (URLs + sizes) and `changelog/page.tsx` (notes), commit + push.
