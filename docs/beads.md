# Beads workflow

Beads (`bd`) is the only task tracker. Read `AGENTS.md`, `spec.md`, and
`docs/engineering.md` first. Use `.agents/skills/beads/SKILL.md` for CLI guidance.

## Start and claim

```sh
bd prime
bd ready
bd show <id>
bd update <id> --claim
```

Claim atomically before editing. Read requirement IDs, acceptance criteria, and
blockers. Parent epics group work; explicit dependencies determine execution order.
Never implement a blocked task by guessing an unresolved contract. Use `bd remember`
for durable knowledge; do not create markdown task lists.

## Create and finish

Each implementation task includes scope, spec requirement/acceptance IDs, likely
files, validation, and explicit prerequisites. `bd dep add <task> <prerequisite>`
makes the first task wait for the second. Record discoveries as beads.

Close with `bd close <id> --reason="..."` only after acceptance criteria and checks
pass. If review or integration was explicitly required, keep the task open until
that requirement is satisfied. Local work does not require a remote merge. Record
changes, checks/results, branch/base commit, remaining risks, and the exact next
action in the bead. Report uncommitted work accurately.

## Git authority

This repository is local-only and has no remote. Do not push, pull, or run Dolt
remote sync. Do not commit, merge, publish, or deploy without explicit authority.
Beads database transactions/history are part of normal issue operations. At handoff
run `git status --short`; preserve unrelated user changes.

## Health and recovery

```sh
bd doctor
bd lint
bd dep cycles
bd backup status
```

The source of truth is the local Dolt database, not Git or an optional JSONL export.
A local Dolt-native backup is configured at `.beads/backup`. Run `bd backup sync`
after backlog changes and before database maintenance; this local backup command
does not contact a remote. Use `bd backup init /absolute/local/backup/path` to change
the destination. A same-disk backup does not protect against disk loss; the user may
copy it off-device. JSONL exports are not full database backups.

For recovery, preserve the damaged directory first and consult `bd backup restore
--help`. Restore into an initialized recovery workspace; verify with `bd doctor`
and `bd list --json`. Never use `--force` against the working database without
explicit approval. Restore drills must use a disposable workspace.

If a sandbox blocks localhost, request access to the existing local Dolt server;
do not reinitialize the project or assume the database is broken. Check `bd dolt
status` before starting a server. Runtime logs and backups stay ignored.
