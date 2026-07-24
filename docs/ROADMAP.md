# Roadmap

This is the ordered backlog for Autopilot. It exists because the direction is real
but scattered: the honest edge of the system lives in the [threat model](THREAT_MODEL.md)
as residuals **R1–R8, R10** (R9 discharged as T10), and the architectural follow-ons
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
kernel (`reach`, `policy`, the outcome gate) and, as of M4, the real I/O path
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
runs at all (M1); it is the fact everything else is evidence *about*. Dependency
maintenance (M2) closes a real capability gap the agent had no way around — it
needed nothing from the sensor work below to land, so it landed next. Then make the
gate the code passes through actually mean "the page works," not "the code
compiles" (M3) — the load-bearing residual for a frontend. Then put the real path
that M1 exercised under automated test so it cannot silently rot (M4). Only then
generalise the substrate (M5) and harden for a non-local deployment (M6).

| # | Milestone | Discharges | Depends on |
|---|---|---|---|
| M1 | ✅ First light — one real genesis changeset | — (unblocks all) | nothing |
| M2 | ✅ Dependency maintenance as a first-class changeset | — (closes a capability gap) | M1 |
| M3 | Agentic UI verification | **R7** | M1 |
| M4 | ✅ The real path under test | **R9** | M1 |
| M5 | Abstract the effector off git | ADR 0003 follow-on | M4 |
| M6 | Harden the build sandbox and authenticate the seam | **R8**, **R3** | M1 |

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
precondition that made M3 and M4 testable against something real, and M4 (below)
is that.

## M2 — Dependency maintenance as a first-class changeset (done)

**Status: done, 2026-07-24.** The envelope gitignored lockfiles and excluded root
files from the Maintenance allowlist, so the agent could not perform a dependency
or transitive-security upgrade at all — a real capability gap, not a structural
edge case. It is closed the way the rest of the system closes gaps: the agent
proposes intent, the trusted core computes the artifact that intent is judged by.

**Why here.** This did not depend on M1 succeeding in any technical sense, but it
needed a working changeset lifecycle to land into — M1 proved that lifecycle
worked. It needed nothing from the sensor work below (M3, agentic UI verification)
and had no reason to wait on it.

**What landed.** `package.json` joins the Maintenance write surface by exact-match
(`ALLOWED_WRITE_FILES`), not by widening the prefix allowlist — a prefix rule would
also admit `package.json.bak`. `package-lock.json`, `pnpm-lock.yaml`, and
`yarn.lock` become an exact-match never-write zone under **every** clearance,
Genesis included: a lockfile's `resolved`/`integrity` pair is attacker-controllable
as a pair, and installing from one runs dependency lifecycle scripts inside the
verifier, on the host — so the agent is never the one to author it. `worktree::BuildVerifier`
gains a resolve step ahead of install (`npm install --package-lock-only
--ignore-scripts`, run only when `package.json` changed or no lockfile exists yet)
and an audit step after build (`npm audit`, compared against the same command run
at `HEAD` — a **non-regression** gate: pre-existing findings never block, only a
newly introduced advisory does). The envelope-computed lockfile joins the
changeset that produced it, and the baseline `.gitignore` no longer excludes it —
it is tracked, like any other envelope-authored artifact. A new `envelope
refresh-deps` operation and advisor tool (`refresh_dependencies`) cover the
pure-transitive case: a fix entirely inside existing ranges, no manifest change.
Recorded in [ADR 0009](adr/0009-dependency-maintenance.md); the audit gate is
threat T12 and the lockfile never-write rule is T11 in the
[threat model](THREAT_MODEL.md).

**Where it lives.** `envelope/src/invariants/reach.rs`, `envelope/src/worktree.rs`,
`envelope/src/main.rs` (`refresh-deps`), `advisor/src/envelope.ts` and
`advisor/src/tools.ts` (`refresh_dependencies`).

**Exit criterion — met.** The agent can propose a `package.json` change or call
`refresh_dependencies` and have the envelope compute and commit a lockfile in the
same changeset; staging a lockfile directly is refused under every clearance; a
changeset that introduces a new advisory is refused, one that leaves pre-existing
findings unchanged is not — each demonstrated by a real-`npm` integration test in
`tests/worktree_lifecycle.rs`.

## M3 — Agentic UI verification (discharges R7)

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

## M4 — The real path under test (discharges R9) (done)

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
by M4 — R9 was specifically about the real *envelope* adjudication path, which is
now covered.

**Exit criterion — met.** R9 has moved to Threats: the real adjudication path is
covered by `cargo test` (green build commits, red build reverts, the commit is
exactly the staged set), so a regression in the I/O orchestration fails CI rather
than requiring a manual run.

## M5 — Abstract the effector off git (ADR 0003 follow-on)

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

## M6 — Harden the build sandbox and authenticate the seam (discharges R8, R3)

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
- **R10 (registry provenance / publish attestation)** — the audit gate (M2, T12) is
  a non-regression check against advisories the registry has already filed; verifying
  that a published tarball is what its maintainer actually published (npm's
  provenance/attestation tooling) is a further hardening step, not a precondition for
  the non-regression property to hold.
