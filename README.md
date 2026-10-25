<p align="center">
  <a href="https://vivianne.dev">
    <img alt="Vivianne" src="https://vivianne.dev/logo-auto.svg" width="112">
  </a>
</p>

<h1 align="center">Vivianne</h1>

<p align="center">
  A desktop AI agent that works on your projects, your computer and your browser.<br>
  Bring any model: an API key, your ChatGPT plan, or a model running on your own machine.
</p>

<p align="center">
  <a href="https://vivianne.dev">vivianne.dev</a> ·
  <a href="https://github.com/v3dqnt/vivianne/releases">Download</a> ·
  <a href="CHANGELOG.md">Changelog</a>
</p>

---

Vivianne is a native desktop app (Tauri 2, Rust and React). You talk to it in a chat, and it does the work: it reads and changes code, runs commands, browses, controls apps on your computer, and keeps going on long tasks until they're done. Windows is the primary platform; macOS and Linux build, but computer control is Windows-only for now.

## What it does

**Works like a colleague, not a chatbot**
- **Long tasks that survive failure.** Rate limits are waited out, failing models fall back to the next one, network drops are retried, and a task resumes after the app restarts. Set a spend or time limit in the composer if you want one.
- **Progress you can follow.** Each finished step becomes a checkpoint, tool calls fold into readable groups, and a task ends with its time, tokens and cost.
- **Plan mode.** Vivianne researches and writes a plan you can comment on paragraph by paragraph; approving it hands the steps to the team.
- **Memory.** Standing instructions and facts you asked it to keep are always in view; everything else is recalled when relevant. New facts are reconciled with what it already knows instead of piling up.
- **Tools it writes itself.** When a task needs something no tool does, it writes one in Python, tests it, and keeps it for every model to use.

**Teams of agents**
- A project can be a set of threads, a **company** (a master agent with employees), or an **agent team**: teammates you define, each with its own job, model, tools, rules and memory.
- The team shares a group chat with you, and you can DM each teammate. Mention one with `@`, or `@everyone`; they hand work to each other in the chat.
- Teammates run **routines** on a schedule, can check in on the project while you're away, and ask you when they need a decision. Per-teammate rules decide what they may do alone and what they ask first.

**Your computer and your browser**
- **Computer control (DIP).** Uses Windows UI Automation to read and operate apps directly (fast, reliable, and with real keystrokes), with pixel control as a fallback. A notch at the top of the screen shows what it's doing, with a Stop button.
- **Your own browser.** The [Vivianne browser extension](packages/helium-extension) lets Vivianne use the browser you're signed in to (Chrome, Edge, Brave, Arc, Opera, Vivaldi, Helium or any other Chromium browser), in tabs it groups as "Vivianne".

**Files, everywhere**
- File names in the chat open in the Preview panel, which shows code, images, video, audio, 3D models, Markdown, HTML, CSV, PDFs and Office documents (editable).

**Any model**
- Anthropic, OpenAI, Google, xAI, Mistral, Groq, OpenRouter, DeepSeek, Together, Fireworks, Cerebras, NVIDIA, Hugging Face, Azure, Bedrock, Cloudflare and any OpenAI-compatible endpoint.
- **Your ChatGPT plan** (Plus or Pro) through Sign in with ChatGPT, instead of an API key.
- **Local models** through llama.cpp, managed from Settings.

## NVIDIA Nemotron and Nebius

This edition runs on NVIDIA Nemotron models served by **Nebius Token Factory**. Token Factory is one OpenAI-compatible endpoint (`https://api.tokenfactory.nebius.com/v1`) for chat, embeddings and model discovery, so the large and small Nemotron models share one key, and every call's tokens and price land in the same usage view.

| Job | Model or service | Code |
|-----|------------------|------|
| Chat, planning, hard steps | Nemotron Ultra (or Super) on Token Factory | `ai/provider_registry.rs` (the `nebius` provider), `ai/model_discovery.rs` |
| Background work: titles, summaries, memory upkeep, easy steps | Nemotron Nano on Token Factory | `agent/models/routing.rs` |
| Cost-aware routing (`viv-auto`, beta) | Sends each task to Nano, Super or Ultra and shows what it saved against the big model | `agent/models/routing.rs`; Settings → Providers |
| Memory embeddings | Token Factory embeddings, or an NVIDIA Nemotron embed model | `ai/embedding.rs`; Settings → Memory |
| Heavy compute | Nebius AI Serverless Jobs through the `nebius_job` tool: fine-tuning, benchmarks, batch runs on a Nebius GPU. Every submit and cancel asks you first, and a time limit is always set | `models/nebius.rs` |
| Cost | Tokens and cost per model, per day | `agent/models/usage_store.rs`; Settings → Usage by model |
| Skills | An NVIDIA and Nebius skills hub: scanned, pinned to a commit, reviewed before install | Settings → Skills |

Paths are under `packages/gui/src-tauri/src`. More detail in [HACKATHON.md](HACKATHON.md).

## Install

Download the installer from [Releases](https://github.com/v3dqnt/vivianne/releases), then add a provider in **Settings → Providers** (an API key, or **Continue with ChatGPT**).

Optional pieces live in **Settings → Components**: the browser engine for web tools, and Docker for sandboxed code.

### Set up with Nebius and Nemotron

1. Create an API key in [Nebius Token Factory](https://tokenfactory.nebius.com).
2. Paste it into the first-run card (the title-bar badge reopens it), or into **Settings → Providers → Nebius**. Vivianne lists the models on your account, picks Nemotron Ultra for chat and Nemotron Nano for background work, and turns routing on.
3. Optional: choose the routing tiers in **Settings → Providers → Cost-aware routing**, and the cloud embedder in **Settings → Memory**.
4. Optional, for `nebius_job`: install the [Nebius CLI](https://docs.nebius.com/cli) and sign in with `nebius profile create`. Vivianne uses the CLI's own login and never stores a Nebius credential.
5. Optional: add a Tavily key in Settings for web search with sources under each reply.

### Browser extension

1. Open your browser's extensions page: `chrome://extensions`, `edge://extensions`, `brave://extensions` or `helium://extensions`.
2. Turn on **Developer mode**, choose **Load unpacked**, and pick the folder from **Settings → Web & browser** in Vivianne (`~/.viv/helium-extension`).
3. The extension connects whenever Vivianne is running. Only this extension can connect to Vivianne's local bridge.

## Develop

Requirements: Node.js 20+, Rust 1.82+, and on Windows the MSVC build tools (Visual Studio C++ workload).

```bash
npm install                      # from the repository root
cd packages/gui
npm run tauri dev                # the app, with hot reload for the UI
npm run tauri build              # an installer, in packages/gui/src-tauri/target/release/bundle
```

Checks:

```bash
cd packages/gui/src-tauri && cargo test --no-default-features --lib
cd packages/gui-next && npx tsc -p tsconfig.app.json --noEmit
```

### Layout

| Path | What's there |
|------|--------------|
| `packages/gui/src-tauri` | The backend (Rust): the agent loop, tools, providers, memory, teams, computer control |
| `packages/gui-next` | The app's interface (React, Radix-based UI kit) |
| `packages/helium-extension` | The browser extension, for any Chromium browser |
| `packages/gesture-control` | A standalone tray app that turns webcam hand gestures into mouse and keyboard input |
| `docs` | Privacy policy, terms, design notes |

## Privacy and safety

Vivianne runs on your machine. Your conversations, memory and files stay local; requests go only to the model providers you configure. Actions with side effects (running commands, changing files outside the project, controlling apps, using your browser) ask for approval unless you turn that off. See [SECURITY.md](SECURITY.md) and the [privacy policy](docs/privacy-policy.md).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md), and [AGENTS.md](AGENTS.md) for rules that apply to people and agents working in this repository.

## License

[MIT](LICENSE). Vivianne began as a fork of [Pi](https://github.com/badlogic/pi-mono) by Mario Zechner and has since been rewritten as its own app; the original copyright notice is kept in the license as it requires.
