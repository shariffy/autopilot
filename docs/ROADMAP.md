# Roadmap

This is the ordered backlog for Charter. It exists because the direction is real
but scattered: the honest edge of the system lives in the [threat model](THREAT_MODEL.md)
as residuals **R1–R9**, and the architectural follow-ons live in the [ADRs](adr).
This document does not invent new work — it *sequences* what those two already
name, so an auditor can see not just what is unfinished but in what order it
should be finished and why.

It follows the threat model's convention for honesty: a milestone is **done** only
when its residual moves from *Residuals* to *Threats* with a mitigation and a test
(see [THREAT_MODEL.md](THREAT_MODEL.md) "How this stays honest"). Prose that claims
a thing is safe without a test that fails when it isn't is not a discharge.

## Where we are

The trust boundary and its two enaction paths — the in-memory `Harness::enact` and
the real git-backed `begin → stage → commit` — are built and unit-tested (the pure
kernel: `reach`, `policy`, the outcome gate). The conductor runs on the Claude Agent
SDK with its only write channel being the `envelope` binary. The observation ledger
is seeded for the first real task (a small admin web app; a predecessor deployment
already exists).

What has **not** happened: the system has never run a genesis loop to a committed
outcome. A project created with `charter init` has no established workspace yet,
and its audit journal holds only rejected dry-run probes. Everything below is
ordered around closing that gap first, then hardening the pieces the first real
run will lean on.

## The order, and why

Sequencing is driven by dependency, not by residual number. First prove the loop
runs at all (M1); it is the fact everything else is evidence *about*. Then make the
gate it passes through actually mean "the page works," not "the code compiles" (M2)
— the load-bearing residual for a frontend. Then put the real path that M1 exercised
under automated test so it cannot silently rot (M3). Only then generalise the
substrate (M4) and harden for a non-local deployment (M5).

| # | Milestone | Discharges | Depends on |
|---|---|---|---|
| M1 | First light — one real genesis changeset | — (unblocks all) | nothing |
| M2 | Agentic UI verification | **R7** | M1 |
| M3 | The real path under test | **R9** | M1 |
| M4 | Abstract the effector off git | ADR 0003 follow-on | M3 |
| M5 | Harden the build sandbox and authenticate the seam | **R8**, **R3** | M1 |

---

## M1 — First light: one real genesis changeset

**Why first.** The boundary is built and the ledger is seeded, but the end-to-end
brain → envelope → green-build → committed-outcome path has never run to completion.
Until it does, every claim about the system is a claim about code that has not been
exercised together. This milestone is not a feature; it is turning the key.

**What lands.** A real run of the conductor against the seeded observations: the
agent reads the predecessor deployment, chooses its own strategy (per [ADR 0005](adr/0005-changesets-charters-and-observation-driven-genesis.md)
§3 the choice is the agent's, and "do little" is a legitimate outcome), and — if it
elects to build — lands **one** green, atomic, reach-bounded genesis changeset into a
new outcome (the project's `workspace/`), opening `docs/adr/0001` in the outcome
with its strategy per [ADR 0006](adr/0006-inputs-and-decisions-as-append-only-ledgers.md)
§3.

**Where it lives.** `conductor/` (the run), the new outcome repo (external, not
vendored), and the project's audit journal (which should gain its first
`establish` / `stage` / `commit` records instead of only dry-run rejections).

**Exit criterion.** A committed genesis changeset exists in the outcome, its build
is green, and the audit ledger records the establish→stage→commit sequence that
produced it. The run is written up (what was observed, what strategy was chosen)
so the next person can reproduce it. This has no residual to discharge — it is the
precondition that makes M2 and M3 testable against something real.

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

## M3 — The real path under test (discharges R9)

**Why.** `worktree::adjudicate_write` and the changeset lifecycle — the code M1
actually runs — are verified today only by a manual smoke run, because they need
`git` and `npm`. The pure kernel they reuse is unit-tested; the I/O orchestration
around it is not. [R9](THREAT_MODEL.md) is this gap. The conductor has no tests at
all (CI runs only `tsc --noEmit`).

**What lands.** Automated coverage of the real path — begin/stage/commit,
commit-on-green, reset-on-red, package-manager detection, and the establish modes —
against a throwaway git fixture, wired into CI (which already runs `cargo test`).
A first test around the conductor's envelope seam.

**Where it lives.** `envelope/` tests exercising `worktree.rs`; a test surface for
`conductor/` and a `test` script in `conductor/package.json`; `.github/workflows/ci.yml`.

**Exit criterion.** R9 moves to Threats: the real adjudication path is covered by
`cargo test` (green build commits, red build reverts, reach is enforced per stage),
so a regression in the I/O orchestration fails CI rather than a manual run.

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
- **[R3](THREAT_MODEL.md)** — the process seam exists (the conductor's only write
  channel is the `envelope` binary), but it is not authenticated. Trust derives from
  the local process boundary, not a versioned, authenticated wire (assumption **A4**).
  A network-deployed agent needs that wire.

**Where it lives.** The verifier's execution environment in `envelope/` (sandbox);
the conductor↔envelope invocation and a versioned protocol per [ADR 0002](adr/0002-language-for-the-trusted-core.md) (seam).

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
