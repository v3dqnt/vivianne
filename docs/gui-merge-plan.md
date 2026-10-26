# Merge `packages/gui` and `packages/gui-next` into one app package

## Why

The app is one Tauri program split over two packages for historical reasons (the React UI was rewritten as `gui-next` next to an older one, and the name stuck):

- `packages/gui`: the Tauri shell. `src-tauri/` (all the Rust), `scripts/dev-env.*` (the build environment), `scripts/viv*` (CLI launchers), a `package.json` whose only job is `@tauri-apps/cli` and `npm run tauri`.
- `packages/gui-next`: the React interface (`src/`, `vite.config.ts`, `index.html`, `overlay.html`, tsconfigs, self-check scripts) and every frontend dependency.

Nothing else uses either half on its own. Two packages means two `package.json`s, `../../gui-next/dist` paths in the Tauri config, an `include_str!` that climbs four directories, two READMEs, and newcomers asking which one is "the app". The standard Tauri layout is one package with `src/` and `src-tauri/` side by side.

## Target

```
packages/gui/
  package.json        one: frontend deps + @tauri-apps/cli; scripts dev, build, tauri, lint, icons
  index.html  overlay.html  vite.config.ts  tsconfig*.json
  src/                the React app (was gui-next/src)
  public/
  scripts/            dev-env.*, viv*, plus the self-checks and gen-icons (no name clashes)
  src-tauri/          unchanged
```

`packages/gui-next` is gone. The package keeps the name `gui` so the Rust paths, the release workflow's `projectPath: packages/gui` and people's muscle memory don't change.

## Steps (one commit, no behaviour change)

1. `git mv` from `gui-next/` into `gui/`: `src`, `public`, `index.html`, `overlay.html`, `vite.config.ts`, `tsconfig.json`, `tsconfig.app.json`, `tsconfig.node.json`, and each file in `scripts/`. Fold `gui-next/README.md` into `gui/README.md`.
2. **`gui/package.json`**: take `gui-next`'s dependencies, devDependencies and scripts, add `@tauri-apps/cli` and `"tauri": "node scripts/dev-env.mjs"`. Keep `"type": "module"` (check `scripts/dev-env.mjs` and `viv` still run as ESM; they are `.mjs`/shell already). Delete `gui-next/package.json`.
3. **`src-tauri/tauri.conf.json`**: `frontendDist: "../dist"`, `beforeDevCommand: "npm run dev"`, `beforeBuildCommand: "npm run build"` (the Tauri CLI runs them from `packages/gui`). Delete `tauri.gui-next.conf.json`: a leftover preview config nothing references.
4. **`src-tauri/src/mods/protocol.rs`**: `include_str!("../../../../gui-next/src/modules/mods/sdk/v1.js")` becomes `"../../../src/modules/mods/sdk/v1.js"`. Grep `include_str!`/`include_bytes!` for any other path into `gui-next`.
5. **Root files**:
   - `tsconfig.json` references `./packages/gui`.
   - `.github/workflows/macos.yml` `working-directory: packages/gui`.
   - `README.md` checks and layout table; `AGENTS.md` if it names the path.
   - `.gitignore`: add `packages/gui/dist/` if `gui-next/dist` was ignored by a pattern that no longer matches.
   - Comments that name `packages/gui-next/...` in Rust (`loop_core.rs`, `ai/types.rs`, `commands.rs`, `instant/mod.rs`, `exec.rs`, `provider_registry.rs`): `sed` to `packages/gui/src/...`.
   - Scripts that say `-w gui-next` (`gen-icons.mjs`) become `-w gui`.
6. **Lockfile**: `npm install --package-lock-only --ignore-scripts` from the root. No new packages, so `min-release-age` doesn't bite; the diff should be the workspace entries moving.
7. **Vite**: the `dedupe`/Radix alias comment in `vite.config.ts` mentions the root hoisting a React "for `packages/gui`". With one package that copy is the app's own; keep `dedupe` (the `trailer` package still brings a second React) and fix the comment.
8. **Release workflow** (`v3dqnt/vivianne-releases/.github/workflows/release.yml`, separate repo): the typecheck step uses `working-directory: packages/gui-next`. Make it work for old and new tags:
   ```yaml
   - name: Typecheck frontend
     run: |
       if [ -d packages/gui-next ]; then cd packages/gui-next; else cd packages/gui; fi
       npx tsc -p tsconfig.app.json --noEmit
     shell: bash
   ```
   `tauri-action`'s `projectPath: packages/gui` is unchanged.
9. **Memory/notes**: the note "tsc checks nothing at root" names `packages/gui-next`; update it to `packages/gui`.

## Separate cleanup (ask first; not part of the move)

`packages/gui` also tracks files that look accidental: `.obsidian/` (an editor vault config), `SKILL (4).md` next to `SKILL.md`, and an untracked `~` folder. Delete them in their own commit if the user agrees.

## Order

After the agent merge (`docs/agent-merge-plan.md`) phase 4 lands, because phase 4 deletes and edits frontend files (`BotChat`, `BotSteps`, `LiveRunCard`) and a rename on top of open edits makes both harder to review. Doing it first is also fine if nobody is mid-edit in `gui-next/src`; `git mv` keeps history either way (`git log --follow`).

## Verification

- `npm ci` from the root, clean.
- `npx tsc -p tsconfig.app.json --noEmit` in `packages/gui`.
- `npm run build` in `packages/gui` (Vite writes `packages/gui/dist`).
- `cargo test --no-default-features --lib` and `cargo check` in `src-tauri` (the moved `include_str!`).
- `npm run tauri dev`: the window opens with the UI and hot reload works; the overlay window (`overlay.html`) loads.
- `npm run tauri build` locally: the installer contains the UI (not a blank window, which is what a wrong `frontendDist` produces).
- The macOS CI check, and one release-workflow run on a test tag before the next real release.
- `git grep gui-next` returns only history (CHANGELOG, old plans).
