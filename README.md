<p align="center">
  <a href="https://vivianne.dev">
    <img alt="pi logo" src="https://vivianne.dev/logo-auto.svg" width="128">
  </a>
</p>
<p align="center">
  <a href="https://discord.com/invite/3cU7Bz4UPx"><img alt="Discord" src="https://img.shields.io/badge/discord-community-5865F2?style=flat-square&logo=discord&logoColor=white" /></a>
  <a href="https://www.npmjs.com/package/@v3dqnt/vivianne-agent-ai"><img alt="npm" src="https://img.shields.io/npm/v/@v3dqnt/vivianne-agent-ai?style=flat-square" /></a>
</p>

> New issues and PRs from new contributors are auto-closed by default. Maintainers review auto-closed issues daily. See [CONTRIBUTING.md](CONTRIBUTING.md).

# Vivianne

Vivianne is a desktop AI coding agent. The agent harness, coding-agent core, and terminal UI now live as a Rust backend inside the Tauri desktop app; the legacy TypeScript CLI/TUI packages have been retired.

* **[gui](packages/gui)**: Desktop app — Tauri 2 (Rust backend) + Next.js/React frontend
* **[@v3dqnt/vivianne-agent-ai](packages/ai)**: Unified multi-provider LLM API (OpenAI, Anthropic, Google, …)

## All Packages

| Package | Description |
|---------|-------------|
| **[gui](packages/gui)** | Desktop app: Rust/Tauri backend (agent harness, coding-agent core, orchestrator) + Next.js/React frontend |
| **[@v3dqnt/vivianne-agent-ai](packages/ai)** | Unified multi-provider LLM API (OpenAI, Anthropic, Google, etc.) |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for contribution guidelines and [AGENTS.md](AGENTS.md) for project-specific rules (for both humans and agents).  Longer term plans for Pi can also be found in [RFCs](https://rfc.v3dqnt.com/keyword/pi/).

## Development

```bash
npm install --ignore-scripts  # Install all dependencies without running lifecycle scripts
npm run build        # Build packages/ai
npm run check        # Lint, format, and type check
./test.sh            # Run tests (skips LLM-dependent tests without API keys)
```

See [packages/gui/README.md](packages/gui/README.md) for building and running the desktop app.

## Supply-chain hardening

We treat npm dependency changes as reviewed code changes.

- Direct external dependencies are pinned to exact versions. Internal workspace packages remain version-ranged.
- `.npmrc` sets `save-exact=true` and `min-release-age=2` to avoid same-day dependency releases during npm resolution.
- `package-lock.json` is the dependency ground truth. Pre-commit blocks accidental lockfile commits unless `PI_ALLOW_LOCKFILE_CHANGE=1` is set.
- `npm run check` verifies pinned direct deps and native TypeScript import compatibility.
- CI installs with `npm ci --ignore-scripts`, and a scheduled GitHub workflow runs `npm audit --omit=dev` plus `npm audit signatures --omit=dev`.

## License

MIT

<p align="center">
  <a href="https://vivianne.dev">vivianne.dev</a> domain graciously donated by exe.dev
</p>
