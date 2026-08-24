# ADR 0015 — Delete the in-memory showcase

- **Status:** Accepted
- **Date:** 2026-08-18
- **Scope:** removes the simulated enaction path (`harness.rs`, `reversible.rs`,
  `agent.rs`, `verifier.rs`, `telemetry.rs`, `guardrails.rs`,
  `decision_log.rs`, `invariants/change_shape.rs`) and the `Action` variants
  that existed only to feed it. Does not change reach, the clearances, the
  changeset lifecycle, or any gate the real path runs.

## Context

The crate contained two enaction paths. The threat model described them as
peers — "two enaction paths, sharing the same policy kernel" — and that framing
was generous to the point of being wrong.

One path is real: `worktree::{stage, commit}`, reached by invoking the
`envelope` binary, operating on a git work tree, running the outcome's own
build, tests, e2e, design lint and audit check.

The other was a simulation. `Harness::enact` moved values around a
`HashMap<String, usize>` — a "world" of paths mapped to byte counts. Its
telemetry and verifier were seeded stubs. Its agent was a hardcoded list of
eight fake proposals. Its `Deploy` action deployed nothing; no advisor could
construct one, because the advisor's only verb is a write. The whole path was
reachable from exactly one place: running the binary with no arguments, which
printed a demo.

They did not share a policy kernel in any load-bearing sense either. They shared
`reach`. Everything the simulation demonstrated *beyond* reach — the verification
gate, the outcome gate, reversibility, the audit trail — existed only inside it.

That would be ordinary dead weight in a normal codebase. Here it was worse,
for two reasons.

**It doubled the audit surface of a crate whose entire claim is that its audit
surface is small.** The pitch is "the TCB is small enough to read top to
bottom." Roughly a thousand of the lines an auditor would read implemented a
system that could not affect anything.

**Five threat-model rows cited it as their mitigation.** T3 pointed at
`verifier.rs` and `change_shape.rs`; T5 at `telemetry.rs`; T6 at
`guardrails.rs::standard`; T8 at `reversible.rs` and `harness.rs`; T9 at all
three. Each row named a module, as this repository requires — and each named a
module that no advisor could reach. The convention that keeps the document
honest ("every mitigation names the module that implements it") was satisfied
in letter while pointing at fiction. A reader checking T8 against the source
would find `World`'s mutators genuinely were `pub(crate)`, and would have
verified nothing about the system that actually writes to their repository.

This is the same failure the repository already knows about in another form:
`CLAUDE.md` warns that reviewing your own plan is not verification, and
THREAT_MODEL's own rules exist because claims drifted from source twice before.
A mitigation that cites a module nobody runs is that drift, pre-installed.

## Decision

Delete the simulation. `Action` keeps one variant, `WriteFile { path }`.

Three consequences of that narrowing are deliberate, not incidental.

**`Action::ModifyPolicy` goes, and T7 gets stronger.** The variant existed to be
refused by `immutable_policy`, demonstrating that the agent cannot widen its own
permissions. But nothing constructed it: the CLI has no such subcommand and the
advisor has no such tool. With it gone, a policy change is not *denied* — it is
*inexpressible*. "The grammar has no word for it" is a better guarantee than "we
check for it and say no."

**`bytes` goes from `WriteFile`.** No surviving invariant read it; only the
deleted harness did. Keeping a dead field alive behind `#[allow(dead_code)]` in
a trusted core is precisely the kind of thing this ADR is deleting. The docs'
phrasing "reach sees a path and a byte count, never the bytes" becomes "reach
sees a path, never the bytes" — the contrast with `design.rs`, which reads the
staged content, is what that sentence was ever for, and it survives intact.

**T6's scope narrows, and the threat model says so.** The old row claimed
guardrails applied "to every deploy." The shipped system has no deploy verb; its
terminal act is a commit, and rollout is out of band. The only real threshold is
the runtime trip's error-rate ceiling. Rather than silently retire the row or
let it inherit the simulation's guarantee, T6 now describes the trip and states
that the per-deploy gate it used to claim never existed outside the simulation.

`Policy` survives as a thin wrapper over `Clearance`, and `Action` survives as a
single-variant enum. Both are now arguably ceremony. They are kept because
`Action`'s shape *is* the claim "the agent can only ask to write a file," and
collapsing it would trade an auditable statement for a saved line. That is the
opposite of this ADR's trade.

## Consequences

- The trusted core drops roughly 925 lines. `cargo test --all-targets` goes from
  51 unit tests to 42; the 34 integration tests over the real git-backed path
  are unchanged, because none of them ever touched the simulation.
- The threat model's actor table, A1, A2, A3, A5, T3, T5, T6, T7, T8, T9, R1,
  R2 and R4 are restated against the real path in the same commit as the
  deletion — per this repository's rule that a change and the claims it
  invalidates travel together.
- **Pointer note.** [ADR 0004](0004-name-the-system.md) cites `immutable_policy`,
  the **verifier**, **guardrails** and **reversibility** as embodiments of the
  "charter" metaphor. Three of those four modules no longer exist. ADR 0004 is an
  accepted record and is not rewritten; the metaphor still holds, but its
  concrete referents are now `reach`, the inexpressibility of a policy change,
  `worktree::BuildVerifier`, and git.
- **The system now has less to show for itself.** The no-argument demo was the
  fastest way to see the boundary refuse something, and it is gone. The
  replacement is the demo outcome's `git log` and the real CLI's verdicts, which
  have the advantage of being true. If a demo is wanted back, it should drive the
  real binary against a throwaway repository — not simulate one.
