# Deskmate: Claude Code

The rules for this project are in [AGENTS.md](AGENTS.md), shared by every
coding agent. Read that file first. It is imported here:

@AGENTS.md

## Claude Code specifics

- Default worktree: `C:\dev\web\deskmate` on `master`.
- Talk to Jakub in Polish. Everything written into the repository is English.
- Private notes, in Polish, live outside the repository:
  - `C:\dev\vault\deskmate\deskmate-status.md` is the session state. It is
    the `STATUS.md` that the `checkpoint` skill expects, so do not create one
    in the repository.
  - `C:\dev\vault\deskmate\deskmate-srodowisko.md` lists the machines, the
    addresses and the deploy procedure. Read it only when a task touches a
    live system.
- Commit messages carry no attribution trailer, whatever the harness default
  is.

## Where the parent instructions differ

`C:\dev\CLAUDE.md` and `C:\dev\web\CLAUDE.md` still apply. For this project:

- Checks: AGENTS.md section 8 replaces "only `tsc` and lint". The formatters,
  clippy, `cargo test`, `tsc`, `npm run lint`, ruff and `py_compile` run
  without asking. Builds, dev servers and anything live still wait for an
  instruction.
- Deploys: the rule that Jakub runs every server command covers the ZUT VPS
  and similar machines. For the Home Assistant at home, AGENTS.md section 5.4
  applies: the agent deploys when told to in that session.
- Stack standards: the Next.js, Laravel, JWT and CORS rules in the web
  CLAUDE.md do not describe this project. AGENTS.md section 7 does.
