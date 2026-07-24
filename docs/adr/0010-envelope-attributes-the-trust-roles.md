# ADR 0010 — The envelope attributes the trust roles on every commit

- **Status:** Accepted
- **Date:** 2026-07-24
- **Scope:** how the git author/committer identity on a commit the envelope
  makes is decided. Extends `worktree.rs`'s `commit` and `establish` paths; does
  not change the changeset lifecycle, clearances, or the trust thesis of ADR
  0005.

## Context

`worktree::commit` and `worktree::establish_empty` ran `git commit` with no
identity of their own — whatever `GIT_AUTHOR_NAME`/`GIT_COMMITTER_NAME` (or
`git config user.name`/`user.email`) happened to be ambient on the host running
the envelope is what landed in the outcome's history. The first maintenance run
surfaced this concretely: a changeset committed under the operator's own git
identity, not under any identity that says what actually happened — that a
proposal came from the advisor and a commit came from the envelope.

This is a gap in the same shape as the ones ADR 0009 closed for dependencies:
the trusted core produced a real artifact (a commit) without deciding a property
of that artifact (who it is attributed to) that only the trusted core is
positioned to decide correctly. An ambient identity is not just cosmetic —
`git log` is part of the accountability record (`THREAT_MODEL.md`, asset 5), and
an accountability record that says "committed by whoever happened to be logged
in" says nothing the boundary itself can vouch for.

## Decision

### 1. Two fixed, hermetic identities — one per trust role

```
ADVISOR_IDENT  = ("Autopilot advisor",  "advisor@autopilot.invalid")
ENVELOPE_IDENT = ("Autopilot envelope", "envelope@autopilot.invalid")
```

These are labels for trust roles, not accounts anyone logs into — the `.invalid`
TLD says so. The advisor never runs `git` itself and holds no identity of its
own to stamp; the envelope decides what role produced each commit and stamps it
accordingly.

### 2. Author and committer say two different things

A commit's author is who proposed the change; its committer is who actually
landed it. The two trust roles map onto that distinction directly:

- **Changeset commits** (`worktree::commit`, the `begin → stage* → commit`
  path): author = advisor, committer = envelope. The advisor proposed the
  diff; the envelope verified it against the outcome's own build and committed
  it only on green. Both are true and both are worth recording.
- **The `establish` baseline commit**: author = committer = envelope. Nothing
  here is the advisor's work — establishing a workspace baseline is trusted
  setup the agent never performs (ADR 0005) — so there is no proposer to
  distinguish from the committer.

### 3. Stamped via `Command` env, not `git config`

`GIT_AUTHOR_NAME`/`GIT_AUTHOR_EMAIL`/`GIT_COMMITTER_NAME`/`GIT_COMMITTER_EMAIL`
are set directly on the `Command` that runs `git commit`, via a small helper
(`git_commit_as`) kept alongside the existing `git` helper — which stays
unchanged for every non-committing git call, since only a commit has an
identity to stamp. `Command::env` always wins over whatever the parent process
inherited and over any `git config` (global, local, or system) on the host, so
the identity on a commit the envelope makes is correct regardless of what runs
the envelope — a CI runner, a laptop with a personal git identity configured, or
anything else. Nothing is asked of the deployment environment; the previous
`establish` failure message that fretted about "is git user.name/email set?" no
longer applies; that fragility is closed by construction, not by a
configuration requirement.

## Consequences

- Every commit the envelope makes now carries an identity that is correct by
  construction, never by the luck of the host's ambient git configuration.
  `git log` on an outcome repo tells an auditor, truthfully, which commits were
  the advisor's proposal (author) landed by the trusted core (committer), and
  which were the envelope's own trusted setup (both).
- `tests/worktree_lifecycle.rs` asserts this against the real `git` binary: a
  changeset commit shows `Autopilot advisor` as author and `Autopilot envelope`
  as committer, and the `establish` baseline shows `Autopilot envelope` as
  both — even though the test harness sets its own ambient `GIT_*` envs on
  every process it spawns, proving the envelope's explicit identity is what
  actually wins.
- This closes a gap, not a vulnerability the threat model already named: no
  threat or residual referenced commit attribution, so there is no entry to
  retire from `THREAT_MODEL.md`. The accountability asset it strengthens
  (asset 5) already existed; this makes one more part of it hold by
  construction rather than by host configuration.
