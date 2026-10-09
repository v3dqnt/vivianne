# Joining an existing project

You are new here. The people who built it know things the code doesn't say. Your first job is to understand the project well enough to change it safely — then to write that understanding down so nobody has to rediscover it.

## 1. Read what's already written

In this order, skimming first:

1. `.viv/` knowledge files, if they exist (`charter`, `map`, `roadmap`, `journal`). Treat them as a starting point, not truth: check them against the project.
2. README, CONTRIBUTING, docs/, ADRs or decision records, CHANGELOG.
3. `AGENTS.md`, `CLAUDE.md`, `.cursorrules` and similar: instructions other agents were given.
4. Manifests: `package.json`, `Cargo.toml`, `pyproject.toml`, `go.mod`… — the stack, scripts and dependencies.
5. CI config: what is built, tested and checked on every change.

## 2. Map the project

- **Structure**: the top-level folders and what each is for; the entry points (main, routes, handlers, jobs); where configuration lives.
- **How it runs**: the commands to install, build, run, test, lint. Run them. A project you haven't seen build and pass its tests is a project you don't understand yet. Note what fails and why.
- **The core flows**: trace one or two of the most important paths end to end (a request, a user action, a job) through the code.
- **Conventions**: naming, error handling, state management, testing style, commit style. Match them; don't introduce your own.
- **History**: recent commits and open branches — what is actively changing, and who/what is working on it.
- **Open work**: issues, TODOs, failing tests, half-finished branches.

Use search, not guesswork: find where things are defined and called before describing them.

## 3. Learn the intent

Code tells you *how*; only the user can tell you *why* and *what next*. Ask, briefly:

- What is this for, and who uses it today?
- What are you trying to achieve next — this week, this quarter?
- What must not break? What is fragile or feared?
- Anything decided that isn't written down? Anything you'd do differently?

## 4. Write it down

Create or update `.viv/charter.md`, `.viv/map.md`, `.viv/roadmap.md` and a first `.viv/journal.md` entry ([knowledge](knowledge.md)). Mark anything you inferred rather than confirmed as `(unconfirmed)`.

Then summarise your understanding to the user in a few lines — what it is, how it's built, what's next, what you're unsure about — and let them correct you. Corrections go straight into the files.

## 5. Start small

Make your first change something small and verifiable, following the existing conventions exactly. It proves your map is right before you rely on it for bigger work.
