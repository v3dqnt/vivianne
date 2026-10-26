# Git: how to work in a repository

The `<git>` block in your prompt says where the repository stands each turn: the branch, what it tracks, what is unpushed or unpulled, and what is uncommitted. Read it before changing code.

## The workflow

1. **One task, one branch.** Never commit straight to the default branch (`main`/`master`). Before the first change of a task, make a branch with a short, descriptive name: `git switch -c fix-login-timeout`. If the working tree already has someone else's uncommitted changes, ask before touching them.
2. **Small commits, clear messages.** Commit each step that works on its own. The first line says what the commit does, in the imperative, under 72 characters ("Retry the token refresh once before signing out"); a blank line and a few lines of why only when it isn't obvious. Match the style of the repository's recent messages (`git log -8 --format=%s`).
3. **Check before you commit.** `git status` and `git diff --staged`: commit what you meant to, nothing else. Never commit secrets, `.env` files, build output or large binaries; add them to `.gitignore` instead.
4. **Stay in sync.** Pull before you start (`git pull --ff-only`) and before you push. If the branch and its upstream have both moved, rebase onto it (`git pull --rebase`) for a feature branch; never rewrite the default branch.
5. **Push and open a pull request** when the task is done: `git push -u origin HEAD`, then `github_act` to open it. Title in the imperative; body with **What** (bullets), **Why**, and **Testing** (what you ran and its result, against which commit). Keep a pull request reviewable: a few hundred lines; split bigger work.
6. **Watch the checks.** After pushing, read the pull request's checks (`github` action `checks`). When one fails, read its log (`github` action `run`), fix the cause on the same branch, push again. Don't merge with failing checks.
7. **Merge, then tidy.** Squash-merge when the checks pass and the user agrees; delete the branch; switch back to the default branch and pull.

## Parallel work

Two tasks at once go in separate worktrees, not one folder: `git worktree add ../<repo>-<branch> <branch>`. Each has its own files and branch; nothing collides. Remove one when its branch is merged: `git worktree remove ../<repo>-<branch>`.

## Never, without the user asking

- `git push --force` (if you must, `--force-with-lease`, on your own branch only).
- `git reset --hard`, `git clean -fd`, `git checkout -- .`: they destroy uncommitted work.
- Rewriting history that is already pushed to a shared branch.
- Committing with `--no-verify`, or skipping hooks.

## When something goes wrong

- **Merge conflict:** `git status` lists the files; resolve each (keep both sides' intent), `git add` it, then continue (`git rebase --continue` or commit the merge). Tell the user what you chose.
- **Committed to the wrong branch:** `git switch -c right-branch` keeps the commit; then reset the other branch only if it was never pushed.
- **Detached HEAD:** `git switch -c <name>` to keep the work, or `git switch <branch>` to leave.
