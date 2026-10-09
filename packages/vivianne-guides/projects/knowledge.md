# Project knowledge

Four files in the project's `.viv/` folder hold what an agent needs to know. They are short, current and specific. Anyone — the user, another agent, you next week — should be able to read them in five minutes and work confidently.

## `.viv/charter.md` — what and why

The founding document. Changes rarely, deliberately.

```markdown
# <Project name>
<One sentence: what it is and for whom.>

## Purpose
The problem, who has it, why now.

## Success
How we'll know it worked: measurable outcomes, the demo that proves it.

## Scope
In: …  ·  Out (not now): …

## Constraints
Deadlines, platforms, budgets, compliance, things that must not change.

## Decisions
- <Decision> — <why>. (<date>)
```

Every decision that shapes later work goes under **Decisions** with its reason. An unwritten decision gets silently reversed.

## `.viv/map.md` — how it's built

The working map of the project. Updated whenever structure changes.

- **Stack**: languages, frameworks, key libraries (with versions that matter).
- **Structure**: folders and what lives where; entry points.
- **Architecture**: the components and how data flows between them (a short list or a small diagram).
- **Commands**: install, build, run, test, lint, deploy — exactly as typed.
- **Conventions**: naming, patterns, error handling, testing, commits.
- **Gotchas**: things that surprised you, fragile areas, workarounds and why they exist.

## `.viv/roadmap.md` — where it's going

See [roadmap](roadmap.md). Objectives, milestones with definitions of done, what's next, what's done.

## `.viv/journal.md` — what happened

A dated log, newest first. One entry per working session or significant task:

```markdown
## 2026-10-07 — Onboarding redesign, step 2
Done: new sign-up form wired to the API; tests pass (`npm test`).
Decided: email verification moves after first use (see charter).
Next: invite flow.
Unsure: whether the old /welcome route is still linked from emails.
```

Keep the last ~20 entries; fold older ones into a one-paragraph summary at the bottom.

## Rules

- **Short beats complete.** Each file should fit on a screen or two. Link to code and docs instead of copying them.
- **Specific beats general.** "Dates are stored as UTC ISO strings in `created_at`" beats "we handle dates consistently".
- **True beats tidy.** Mark guesses `(unconfirmed)`. Delete what's no longer true. Stale knowledge is worse than none.
- **No secrets.** Never write keys, passwords or personal data into these files.
- **The user can edit them.** Treat their edits as authoritative.
