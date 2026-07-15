# Multi-agent coordination protocol

This repo is worked on by more than one AI coding agent at the same time
(currently: Claude Code and Codex CLI). Both agents can read and write the
same files, so without a protocol they clobber each other's edits — this file
defines how to avoid that. Read this FIRST, before editing anything.

## 1. Worktree layout — each agent gets its own working directory

- `C:\dev\web\deskmate` (branch `master`) — the main worktree. Default for
  Claude Code. This is the integration branch everything eventually merges
  back into.
- `C:\dev\web\deskmate-codex` (branch `codex/work`) — a dedicated git
  worktree for Codex. Codex does ALL its work here, not in the main worktree,
  so its in-progress edits never collide with Claude's.

Both worktrees share the same `.git` history — commits made in one are
visible to the other via normal git commands (`git log`, `git diff`), they
just aren't merged into the other branch until someone explicitly merges.

**Local commits only. No `git push` — there is no remote configured on this
repo, and even if one gets added later, nothing gets pushed without Jakub's
explicit go-ahead in that session.** Committing to your own local branch
(`master` for Claude, `codex/work` for Codex) is expected and fine — that's
how the isolation works. Merging `codex/work` back into `master` happens only
when Jakub asks for it (or a session explicitly says "merge the branches").

If you were told to work in `C:\dev\web\deskmate` directly for a specific
reason (e.g. testing a live dev build, or a session predates this protocol),
finish that task, commit locally on your branch, then switch to your
dedicated worktree for the next task.

## 2. AGENT-LOG.md — append-only, never edit past entries

`AGENT-LOG.md` in the repo root has two parts:

- **"IN PROGRESS" table at the top** — the only part that gets overwritten
  in place. Before touching any file, check this table. If another agent
  claims the area you're about to edit, don't touch it — either work on
  something else or note the conflict for Jakub. When you start work, add
  your row; when you finish, remove it (or update its timestamp if it's an
  ongoing multi-session task).
- **The log below it** — append a new dated entry after every meaningful
  chunk of work (not every single file edit — one entry per task/session is
  enough). Never edit or delete another agent's entry.

## 3. "Do not touch" marker

If a file must not be touched right now (mid manual edit by Jakub, or
something fragile), the first line gets:
```
<!-- DO NOT EDIT — <reason>, <date> -->
```
Any agent seeing this marker skips the file and tells Jakub instead of
working around it.

## 4. Merging

Nobody auto-merges `codex/work` into `master` (or vice versa) without Jakub
asking for it in that session. When asked: review the diff, resolve conflicts
by reading both sides' `AGENT-LOG.md` entries for context, run
`cargo check` + `npx tsc --noEmit` on the merged result before declaring it
done.
