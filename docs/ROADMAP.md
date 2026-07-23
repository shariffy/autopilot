# Roadmap

This is the ordered backlog for Autopilot. It exists because the direction is real
but scattered: the honest edge of the system lives in the [threat model](THREAT_MODEL.md)
as residuals **R1–R8** (R9 discharged as T10), and the architectural follow-ons
live in the [ADRs](adr).
This document does not invent new work — it *sequences* what those two already
name, so an auditor can see not just what is unfinished but in what order it
should be finished and why.

It follows the threat model's convention for honesty: a milestone is **done** only
when its residual moves from *Residuals* to *Threats* with a mitigation and a test
(see [THREAT_MODEL.md](THREAT_MODEL.md) "How this stays honest"). Prose that claims
a thing is safe without a test that fails when it isn't is not a discharge.

## Where we are

The trust boundary and its two enaction paths — the in-memory `Harness::enact` and
the real git-backed `begin → stage → commit` — are built and tested, both the pure
kernel (`reach`, `policy`, the outcome gate) and, as of M3, the real I/O path
itself. The advisor runs on the Claude Agent SDK with its only write channel being
the `envelope` binary.

**M1 has run.** On 2026-07-23 the advisor read the seeded observations for a small
admin console, chose a greenfield React+Vite+TS strategy over local fixtures, and
landed **5 green, atomic genesis changesets** — plus a self-authored ADR — for
$0.96 over 53 turns. See [docs/runs/0001-first-light.md](runs/0001-first-light.md).
Everything below was ordered around producing that run; it now hardens the pieces
it leaned on.

## The order, and why

Sequencing is driven by dependency, not by residual number. First prove the loop
runs at all (M1); it is the fact everything else is evidence *about*. Then make the
gate it passes through actually mean "the page works," not "the code compiles" (M2)
— the load-bearing residual for a frontend. Then put the real path that M1 exercised
under automated test so it cannot silently rot (M3). Only then generalise the
substrate (M4) and harden for a non-local deployment (M5).

| # | Milestone | Discharges | Depends on |
|---|---|---|---|
| M1 | ✅ First light — one real genesis changeset | — (unblocks all) | nothing |
| M2 | Agentic UI verification | **R7** | M1 |
| M3 | ✅ The real path under test | **R9** | M1 |
| M4 | Abstract the effector off git | ADR 0003 follow-on | M3 |
| M5 | Harden the build sandbox and authenticate the seam | **R8**, **R3** | M1 |

---

## M1 — First light: one real genesis changeset (done)

**Status: done, 2026-07-23.** The end-to-end brain → envelope → green-build →
committed-outcome path has run to completion. See
[docs/runs/0001-first-light.md](runs/0001-first-light.md) for the full record.

**Why first.** The boundary was built and the ledger seeded, but that path had
never run. Until it did, every claim about the system was a claim about code that
had not been exercised together. This milestone was not a feature; it was turning
the key.

**What landed.** A real run of the advisor against the two seeded observations: the
agent read them, chose its own strategy — greenfield React+Vite+TS over local JSON
fixtures, no predecessor to adopt (per [ADR 0005](adr/0005-changesets-clearances-and-observation-driven-genesis.md)
§3 the choice is the agent's) — and landed **five** green, atomic, reach-bounded
genesis changesets into a new outcome (`admin-console/workspace`), opening
`docs/adr/0001` in the outcome with its strategy per [ADR 0006](adr/0006-inputs-and-decisions-as-append-only-ledgers.md)
§3.

**Where it lives.** `advisor/` (the run), the outcome repo (external, not
vendored, at `admin-console/workspace`), and the project's audit journal
(`admin-console/advisor-audit.jsonl`: 1 `establish`, 33 `stage`, 5 `commit`
records).

**Exit criterion — met.** Five committed genesis changesets exist in the outcome,
each build green, and the audit ledger records the establish→stage→commit
sequence that produced them. This had no residual to discharge — it was the
precondition that made M2 and M3 testable against something real, and M3 (below)
is that.

## M2 — Agentic UI verification (discharges R7)

**Why.** Today verification is the outcome's own `npm run build` — a genuine
typecheck + bundle gate the agent cannot self-certify (half of T3 is real). But for
a frontend the residual risk that matters is the change that compiles cleanly and
breaks the page. [R7](THREAT_MODEL.md) names this the load-bearing piece.

**What lands.** A UI verification step inside the monitor, layered *after* the build,
that drives the rendered page and confirms the change actually works before
commit-on-green. It runs as a trusted verifier — inside the envelope, from a source
the agent cannot influence or forge (assumption **A5**), exactly as the build does
now (`worktree::BuildVerifier`).

**Where it lives.** `envelope/src/worktree.rs` (a verifier alongside `BuildVerifier`)
and `envelope/src/invariants/change_shape.rs` (the shape gate already expects
typecheck + tests + UI; this makes the UI half real).

**Exit criterion.** R7 moves to Threats: a change that builds but breaks the rendered
page is rejected by the verifier, demonstrated by a test in which a compiling-but-broken
change fails closed and reverts.

## M3 — The real path under test (discharges R9) (done)

**Status: done.** `envelope/tests/worktree_lifecycle.rs` drives the compiled
`envelope` binary against throwaway git repos with real `git` and `npm`/`tsc`
builds, picked up automatically by `cargo test --all-targets` in CI. R9 has moved
from Residuals to Threats in the [threat model](THREAT_MODEL.md) as **T10**.

**Why.** `worktree::adjudicate_write` and the changeset lifecycle — the code M1
actually ran — were verified only by a manual smoke run, because they need `git`
and `npm`. The pure kernel they reuse was unit-tested; the I/O orchestration
around it was not. [R9](THREAT_MODEL.md) was this gap.

**What landed.** Six tests locking in the real path, including the six fixes made
while landing M1: commit-tracks-only-the-staged-set (not `git add -A`),
stage-auto-opens-a-changeset, untracked-residue-tolerated (the liveness fix — a
real `npm install` byproduct left in the tree must not wedge the next changeset),
reset-on-red, begin-refuses-a-dirty-tracked-tree, and a clean establish baseline.

**Where it lives.** `envelope/tests/worktree_lifecycle.rs`.

**Not in this increment.** The advisor itself still has no automated test surface
(CI runs only `tsc --noEmit`); that remains open and is not claimed as discharged
by M3 — R9 was specifically about the real *envelope* adjudication path, which is
now covered.

**Exit criterion — met.** R9 has moved to Threats: the real adjudication path is
covered by `cargo test` (green build commits, red build reverts, the commit is
exactly the staged set), so a regression in the I/O orchestration fails CI rather
than requiring a manual run.

## M4 — Abstract the effector off git (ADR 0003 follow-on)

**Why.** The reversibility substrate is hardcoded to git. [ADR 0003](adr/0003-one-system-repository-outcome-external.md)
and the root README are explicit that this is a property of *this* outcome (a
codebase), not of the system: an event-log outcome would be append-and-truncate, a
database would be transactions. Outcome-swappability — the whole point of keeping
the outcome external — is not real until the effector is an abstraction.

**What lands.** A reversibility interface behind which git is one implementation,
so a non-code outcome can supply its own atomic-and-reversible effector without
touching the trust kernel (`reach`, `policy`, the outcome gate stay identical).

**Where it lives.** `envelope/src/worktree.rs` and `envelope/src/reversible.rs`
(the git specifics move behind a trait; the kernel depends on the trait).

**Exit criterion.** The kernel adjudicates against the effector abstraction, git is
one implementor, and a second (even a trivial append-log) demonstrates the seam is
real. Not a residual discharge — an architectural follow-on that keeps the tool
honest about being outcome-agnostic.

## M5 — Harden the build sandbox and authenticate the seam (discharges R8, R3)

**Why.** Two residuals that only bind once the system is more than a local installation:

- **[R8](THREAT_MODEL.md)** — running the outcome's build executes whatever the
  build does on the host. The build *definition* is already protected (it's outside
  the write allowlist), but a written source the build imports could run at build
  time. The build needs a sandboxed, network-isolated, pinned-toolchain environment.
- **[R3](THREAT_MODEL.md)** — the process seam exists (the advisor's only write
  channel is the `envelope` binary), but it is not authenticated. Trust derives from
  the local process boundary, not a versioned, authenticated wire (assumption **A4**).
  A network-deployed agent needs that wire.

**Where it lives.** The verifier's execution environment in `envelope/` (sandbox);
the advisor↔envelope invocation and a versioned protocol per [ADR 0002](adr/0002-language-for-the-trusted-core.md) (seam).

**Exit criterion.** R8 and R3 move to Threats: the build runs in an isolated
environment (a build-time side effect cannot touch the host or network), and the
seam authenticates so a process that is not the trusted monitor cannot impersonate
it — each with a test that fails when the property is removed.

---

## Not on this roadmap (and why)

These residuals are known and deliberately deferred; they are not near-term because
nothing above depends on them and each is a current-scope choice, not an oversight.

- **R1 (real telemetry)** and **R4 (per-service guardrails)** — the outcome gate is
  built; wiring a real, agent-isolated monitoring integration is a production concern
  beyond the current scope.
- **  R2 (in-crate mediation)** — a hard isolation boundary (splitting the core into
  its own process) is explicitly traded away to keep the TCB a single small crate.
- **R5 (resource bounds)** and **R6 (tamper-evident audit)** — throttling and a
  signed/hash-chained ledger matter for a long-lived deployment, not for proving the
  boundary holds.
