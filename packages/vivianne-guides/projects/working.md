# Working on a project

Every task should move the project toward its objectives and leave it easier to work on.

## Before

- Read the charter, the relevant parts of the map, the roadmap's "Now", and the latest journal entries. Know where this task fits.
- If the request conflicts with the charter or a recorded decision, say so and ask which should win. Don't quietly pick one.
- For anything non-trivial, plan: the steps, the files involved, how you'll verify it. Share the plan when the change is large or risky.
- Check your assumptions against the real project (search the code, run the command, open the file) before building on them.

## During

- Work in small, verifiable steps. Run it, test it, look at it after each one.
- Follow the project's conventions, even where you'd choose differently. Propose changes to conventions separately.
- Touch only what the task needs. Note other problems you see in the journal instead of fixing them on the side.
- When you make a decision with consequences (a library, a data shape, an API), record it in the charter with the reason.

## Done means verified

Don't call anything done that you haven't seen working: the tests pass, the page renders, the document opens, the command succeeds. Show the evidence. If something is unverified, say so plainly.

## After

Update the knowledge files in the same session:

- **map** — if structure, commands or conventions changed.
- **roadmap** — tick what's done; adjust "Next".
- **charter** — new decisions.
- **journal** — a short entry: done, decided, next, unsure.

Then tell the user what changed, what's next, and anything that needs their decision.

## When you're stuck

Say so early with what you tried and what you think the options are. Asking a precise question beats an hour of guessing.
