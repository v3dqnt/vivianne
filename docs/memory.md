# Memory

How Vivianne remembers, what each layer does, what it costs, and how to control it. Settings → Memory holds every switch named here.

Parts of the design come from [Hindsight](https://github.com/vectorize-io/hindsight) (MIT). Each ported file says so. The plan they followed is in `docs/hindsight-integration-plan.md`.

## The layers

| Layer | What it is | Made by | Costs a model call? |
|---|---|---|---|
| **Facts** | One thing remembered: a preference, a decision, a correction. | You (`remember`), or extraction after a turn. | Extraction: one batched call. |
| **Entities** | The people, projects and libraries a fact mentions, linked to it. | The same extraction call. | No extra call. |
| **Two times** | When a fact happened, as opposed to when it was learned. | The same extraction call. | No extra call. |
| **Observations** | A belief that several facts add up to, citing them, with its earlier wording kept. | The sleep pass. | One batched call per pass, only for facts that did not clearly restate a belief. |
| **Pages** | A named question of yours, answered in bullets and kept current. | The sleep pass. | One call per page, only when its evidence changed. |

Everything is in one SQLite file per project (and one for you across projects). Nothing needs a server.

## What happens when you ask

`recall` runs keyword search and meaning search over the project's store and yours, then adds:

- **Entities.** If the question names a known entity, everything linked to it joins the ranking.
- **Time.** A time in the question ("last spring", "in March 2025", "three weeks ago") becomes a window. What happened in it joins the ranking, and facts from that time rank a little higher, far ones a little lower (at most 12.5% either way).
- **The graph.** Related memories are followed a few hops.
- **A reranker,** if you configured a model for it.

`recall` takes two optional settings: `depth` (`low` is a quick lookup with no reranker and one hop, `high` is a thorough search) and `max_tokens` (100 to 8000). Results are packed best-first into the budget, and the answer says how many were left out.

`reflect` answers a question that needs several memories put together. It looks at pages, then beliefs, then facts, in at most six steps, and returns the answer with the memories it rests on. A citation counts only if it names something the search actually found.

## Safety

- **Memory defense.** Passwords, API keys, tokens, private-key headers, connection strings with a password, and card numbers (checked with the Luhn test) are replaced with `[REDACTED:type]` before anything is saved. Settings → Memory → *Refuse secrets* makes it refuse the memory instead. The count of what was hidden is shown. A secret with no recognisable shape (a password typed in a sentence) is not found.
- **Inferred never rewrites stated.** Beliefs and pages are their own rows. Nothing that builds them edits, closes or deletes a fact, in particular one you asked to be remembered.
- **Stale is visible.** A belief whose evidence was later corrected or archived is flagged.
- **Merges can be undone.** When two entity names are judged the same, the merge is recorded, not applied destructively. The rules are conservative: a wrong merge is worse than a missed one.
- **Upgrade backup.** The first time a store is opened by a build that changed its layout, a copy is kept beside it as `memory.db.before-v2`. The changes only add tables and columns.

## Pages

Settings → Memory → *Pages*: a name and a question per page, up to 12. While the app is idle, each page whose nearby evidence changed is updated by small edits (add, change or remove a bullet), not rewritten. At most three pages are refreshed per pass. Only the names appear in the always-on block; the agent finds a page's content with `recall`.

## Connecting a Hindsight server (optional)

Settings → Memory → *Hindsight server*. Give the address, a bank, and a key if the server needs one. The key is kept in the system keychain. The address must be `https` unless it is on this machine. While on, a recall also asks the server, and its answers join the ranking marked *from Hindsight*. A server that is down, slow or wrong never breaks recall. Nothing local is sent to it unless you also switch on *Send what I save to the server too*, and then only text that has passed memory defense.

## What is not built

- Entities are not yet filled in for memories saved before they existed (no backfill pass).
- The Memory panel does not show a belief's evidence, history or stale flag, or the entity list.
- There is no way yet to undo an entity merge from the app; the function exists.
- No local cross-encoder reranker, and none of Hindsight's trained decision models.
- Not verified against a real model, a real embedding backend, or a real Hindsight server: the logic is tested with scripted ones.
