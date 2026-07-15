# Deskmate — project instructions

This project is worked on by more than one AI agent (Claude Code and Codex
CLI) at the same time. Before doing anything else in a session here:

1. Read [AGENTS.md](AGENTS.md) — the multi-agent coordination protocol
   (worktree layout, ownership table, commit rules).
2. Read [AGENT-LOG.md](AGENT-LOG.md) — the IN PROGRESS table and the last
   handful of log entries, so you know what the other agent is mid-way
   through right now.
3. Read [HANDOFF.md](HANDOFF.md) for full project history/context and
   [STATUS.md](STATUS.md) for the most recent checkpoint.

Claude Code's default working directory for this project is
`C:\dev\web\deskmate` (branch `master`). Do not work directly in
`C:\dev\web\deskmate-codex` — that is Codex's dedicated worktree.

No `git push` — no remote is configured, and pushing (once one exists) needs
Jakub's explicit go-ahead in the session regardless. Local commits on
`master` are fine and expected.
