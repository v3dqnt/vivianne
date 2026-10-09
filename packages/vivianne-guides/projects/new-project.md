# Starting a new project

The first hour decides the next hundred. Don't start building until you and the user agree on what is being built and how you'll know it worked.

## 1. Understand the intent

Ask, briefly and in one go — skip what the user already said:

- **What** is it, in one sentence? What problem does it solve?
- **Who** is it for? What do they do today instead?
- **What does success look like?** A measurable outcome or a concrete demo ("a stranger can sign up and send an invoice in under two minutes").
- **Scope**: what is in the first version, and explicitly what is not.
- **Constraints**: deadline, budget, platforms, technologies required or forbidden, compliance, data that must stay private.
- **Taste and references**: products or documents they admire; brand, tone.
- **What they'll do themselves** vs. what they expect from you.

If something is ambiguous and matters, ask. If it's a default any expert would pick, pick it and say so.

## 2. Research before you design

- Look at how similar products or documents solve this; note what to copy and what to avoid.
- For software: check current versions, docs and known pitfalls of the candidate technologies on the web. Don't rely on memory for versions or APIs.
- Identify the riskiest assumption (a third-party API, performance, a legal question) and plan to test it first.

## 3. Write the knowledge files

Create them before writing any product code (formats in [knowledge](knowledge.md)):

- `.viv/charter.md` — purpose, audience, success criteria, scope and non-goals, constraints, decisions so far (each with its reason).
- `.viv/map.md` — the planned architecture: components, how they talk, where things will live, the commands to build/run/test.
- `.viv/roadmap.md` — milestones from first slice to v1, each with a definition of done; the first milestone broken into tasks.
- `.viv/journal.md` — the first entry: what was agreed, open questions.

Show the user the charter and the first milestone. Get a yes before building.

## 4. Build the thinnest slice first

Make the smallest end-to-end version that proves the idea works: real data in, real result out, deployed or runnable. Then widen. A walking skeleton beats a beautiful half.

- Set up the basics on day one: version control, a README with how to run it, formatting/linting, one test that runs, and a way to see the thing working.
- Test the riskiest assumption in the first slice.

## 5. Keep the files alive

After every meaningful step: update the map if structure changed, the roadmap if plans changed, the charter if a decision was made, and add a journal entry. See [working](working.md).
