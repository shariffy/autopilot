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

**M1 has run — as one continuous history through Phase D.** The published demo
repo is the result of a single redo on 2026-07-25 that superseded three earlier,
separately-published runs: genesis, then three further advisor runs landing
autonomous maintenance, a self-healing incident, and a design-governed feature,
all as sequential commits on the *same* history rather than stitched together
afterward. The earlier, separately-published version needed 8 operator (human)
commits to assemble; this one needed **3**, because the design system landed
under genesis instead of being seeded later — every later phase composed from it
natively, with nothing further to seed. See
[docs/runs/0001-first-light.md](runs/0001-first-light.md) for genesis; the
per-phase summaries below describe what each run did, not which specific run —
the current published history is the latest and only live one.

**Phase B: sensor-driven maintenance.** The advisor (Sonnet 4.6) read a
`posthog` analytics signal against the genesis outcome and landed one green,
bounded maintenance changeset — sortable Product columns, advisor-authored /
envelope-committed, with an ADR — inside the fitted Maintenance allowlist
(`src/pages/` writable, `src/data/` frozen). Published to the demo repo.

**Phase C: the two-loop runtime envelope.** A change that built green but
crashed at runtime (a dangling vendor-catalogue lookup) was caught in production
by the model-free **runtime trip** (`envelope monitor` read a telemetry
error-rate breach and `git revert`ed the deploy to last-known-good), then the
advisor landed a **durable fix** — gated by build plus a **frozen reproducer**
the agent cannot author (`tests/contract/`, ADR 0011). This partially discharges
R7 (the test stage catches build-green/runtime-broken pre-commit; the trip
auto-heals in production); the general case stays open.

**Phase D: the design-system invariant.** Reach freezes `src/design-system/`
under Maintenance, and a content-level verifier stage (`design.rs`, which reach
itself cannot express — reach sees a path and a byte count, never the bytes)
requires staged UI files to compose from the frozen primitives, rejecting raw
`<button>`/`<input>`/`<a>` and inline `style={{}}`. Because genesis had already
built the app's own component library on top of the design system, this phase
needed no further trusted setup at all: the advisor (Sonnet 4.6) landed a "Clear
filters" button on the Products toolbar composed entirely from the existing
design system — no raw HTML, no inline styles — green through the design stage
on the first attempt. The residual (R11): the lint
is a deliberate enforceable subset, not a full composition grammar.

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

**Status: done.** The end-to-end brain → envelope → green-build →
committed-outcome path has run to completion. See
[docs/runs/0001-first-light.md](runs/0001-first-light.md) for the full record —
that record describes the genesis run underlying the currently-published demo
repo, superseding two earlier separately-run and separately-published attempts.

**Why first.** The boundary was built and the ledger seeded, but that path had
never run. Until it did, every claim about the system was a claim about code that
had not been exercised together. This milestone was not a feature; it was turning
the key.

**What landed.** A real run of the advisor against three seeded observations: the
agent read them, chose its own strategy — greenfield React+Vite+TS over local
fixtures, including its own internal UI component library, no predecessor to
adopt (per [ADR 0005](adr/0005-changesets-clearances-and-observation-driven-genesis.md)
§3 the choice is the agent's) — and landed **five** green, atomic, reach-bounded
genesis changesets into a new outcome, opening `docs/adr/0001` in the outcome
with its strategy per [ADR 0006](adr/0006-inputs-and-decisions-as-append-only-ledgers.md)
§3.

**Where it lives.** `advisor/` (the run), the outcome repo (external, not
vendored — see [autopilot-demo-admin-console](https://github.com/shariffy/autopilot-demo-admin-console)),
and the project's audit journal (1 `establish`, 53 `stage`, 7 `commit` attempts
of which 5 landed, and one `refresh_dependencies` call — the agent reaching for
the dependency-maintenance tool from ADR 0009 on its own, unprompted).

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

## Phase B — Sensor-driven maintenance (done)

**Status: done 2026-07-24.** The advisor ran on Sonnet 4.6 against the maintained
app, read a PostHog analytics signal, and landed one green, bounded maintenance
changeset (sortable Product columns) inside the reach allowlist — advisor-authored,
envelope-committed, with an ADR — for $0.26. Published to the demo repo. M1 proved
genesis end to end; every milestone since has hardened that path. Phase B is the first exercise of the *other* clearance ADR 0005
named but M1 never used: Maintenance, against an outcome that already exists and
already builds, driven by a sensor rather than a human requirement.

**Why here.** M2 (dependency maintenance) already puts real writes through the
Maintenance clearance, but only for `package.json` — it says nothing about
whether the clearance's allowlist actually fits a real app's *feature* surface,
or whether the brief that goes with it reads as "maintain," not "build." Both
needed settling before a real sensor-driven run is worth spending money on, and
neither depended on M3–M6.

**What landed.**

- `invariants::reach` is fitted to the concrete shape M1 actually produced:
  `src/pages/` (the app's page components) joins `ALLOWED_WRITE_PREFIXES`
  alongside `src/components/`; `src/data/` (the fixture/data-access contract)
  joins `FORBIDDEN_WRITE_PREFIXES` alongside `src/api/` — frozen under
  Maintenance for the same reason: it is a contract the agent did not write and
  should not redefine one page at a time. `src/App.tsx`, `src/types.ts`, and
  `src/main.tsx` stay outside the allowlist deliberately: a bounded in-page
  change should never need to touch routing or the type contract.
- `advisor/src/loop.ts` gains `maintenanceSystemPrompt`, selected by
  `opts.ctx.clearance` in `runLoop`. It briefs the agent that the outcome
  already exists and already builds (`establish_workspace` is not part of the
  job), to read the sensor source(s) before deciding anything, and to propose
  one bounded change. `docs/adr/` joined the Maintenance allowlist during
  Phase C (below); the brief reflects that once it was true.
- A maintenance project (external to this repo, a sibling of the outcome —
  same convention as ADR 0003) holds a git clone of the genesis outcome as its
  workspace, a `posthog` sensor (`events.json` + `README.md`) framed explicitly
  as a static export standing in for a live feed, one observation naming the
  sensor as its source, and `project.json` set to `"clearance": "maintenance"`.
  The sensor's signal is concrete and single-purpose: heavy, repeated clicks on
  the Products table's Name/Price headers (which do nothing today), search
  queries encoding sort intent the search box can't satisfy, and in-app
  feedback explicitly asking to sort the product list — all pointing at one
  bounded change inside `src/pages/ProductsPage.tsx`.

**Where it lives.** `envelope/src/invariants/reach.rs`, `advisor/src/loop.ts`,
and the maintenance project (external).

**Exit criterion — met.** Proven first with the `envelope` binary directly, not
the advisor: a stage of `src/pages/ProductsPage.tsx` under
`--clearance maintenance` is accepted, a stage of `src/data/products.json` is
rejected, a hand-written trivial edit stages and commits green (`npm ci` +
build + audit non-regression). Then the advisor itself, reading the `posthog`
sensor, landed a real green maintenance changeset — sortable Product columns —
advisor-authored, envelope-committed.

## Phase C — The runtime envelope and the frozen oracle (done)

**Status: done.** M1 through Phase B verify a change before it lands; nothing
protected an outcome once a verified change was already live and behaving
badly at runtime. "Elevated error rate" is a flight-envelope-protection
scenario by name, and the system had no runtime response to it at all — a gap
in the thesis, not just a residual.

**Why here.** R7 (build-real, UI-verification-not) is the load-bearing residual
for a frontend, and this closes the load-bearing half of it — a change that
compiles but breaks the page — with the piece nothing else touches: recovery
*after* a bad change is already live, not just refusal before one lands.
Independent of M3's general case and of M5/M6.

**What landed.**

- **The fast loop — `envelope monitor` (`runtime.rs`).** Reads trusted
  telemetry and, if the error rate breaches a threshold, reverts the deployed
  change with `git revert`, stamped as the envelope's own act — no model
  consulted. This is the flight-envelope-protection response: trip to a known
  safe state immediately, improve later, never in the critical path.
- **The slow loop — a test stage in `worktree::BuildVerifier`.** Runs the
  outcome's own `npm test` after the build, so a change that compiles but
  breaks a tested path fails the changeset before it can ever commit.
- **The frozen oracle — `tests/contract/`.** Never writable by the agent under
  any clearance (`invariants::reach`). The reproducer that says what "fixed"
  means must come from evidence the agent didn't produce — the telemetry
  captures the failing case, the frozen test replays it — or the agent could
  certify its own bug.
- Recorded in [ADR 0011](adr/0011-the-runtime-envelope-and-the-frozen-oracle.md);
  the mitigation is threat T14 in the [threat model](THREAT_MODEL.md). Two
  residuals stay open: a wrong-answer (non-throwing) fault still needs a human
  oracle, and telemetry provenance is assumed, not authenticated.

**Where it lives.** `envelope/src/runtime.rs`, `envelope/src/worktree.rs`,
`envelope/src/invariants/reach.rs`.

**Exit criterion — met.** A change that builds green but crashes at runtime was
deployed as trusted setup (a foreign-key lookup with a dangling reference the
frozen fixtures — not the agent — introduced). `envelope monitor` tripped on
the resulting telemetry breach and reverted it with no model involved,
restoring the frozen reproducer to green. The advisor then read the incident
(via a `cloudwatch`-style sensor carrying the captured failing case) and landed
a durable fix — a safe fallback for the missing case — gated by the same
frozen reproducer it could not itself edit, advisor-authored,
envelope-committed.

## Phase D — The design-system invariant (done)

**Status: done.** Reach bounds *where* the agent may write; nothing bounded
*what a page is made of* before this. The gap was first demonstrated
concretely: an earlier maintenance changeset, built before this invariant
existed, added inline styles to a small component and said why in its own
record — a separate stylesheet felt disproportionate for one component, a
reasonable call inside a boundary with no opinion about UI composition, and
exactly how a fitted product surface erodes one locally-cheap decision at a
time. That is precisely the failure mode this milestone closes; the current
published history no longer contains that specific example, because this
invariant has applied to every maintenance changeset from Phase B onward.

**Why here.** Independent of M3–M6 below — none of them touch UI composition —
and it closes a gap already demonstrated to be real, not theoretical. Waiting
would mean shipping more maintenance changesets through a boundary already
known to tolerate the failure mode.

**What landed.**

- `src/design-system/` joins `FORBIDDEN_WRITE_PREFIXES` in
  `invariants::reach.rs`, Maintenance-only (Genesis, which creates the
  primitives in the first place, is unaffected) — the same shape as
  `src/api/`/`src/data/`: infrastructure the agent composes against but does
  not get to redefine.
- A new content-level verifier stage, `envelope/src/design.rs`, invoked from
  `worktree::BuildVerifier::run` right after the build succeeds. Reach cannot
  express "does this file actually use the design system" — it sees a path
  and a byte *count*, never the bytes — so this lives beside the
  build/test/audit stages instead. It lints every staged `.tsx` file under
  `src/pages/`/`src/components/`: no raw `<button>`/`<input>`/`<select>`/`<a>`,
  no inline `style={{`, and at least one import from `src/design-system/`.
  Lint-level by design (substring/line scanning, no JSX parser — the crate
  stays zero-dependency), and deliberately a small, explicit, documented
  subset of "use the design system," not a full composition grammar — the
  residual is named in `docs/THREAT_MODEL.md` (R11), not hidden.
- `advisor/src/loop.ts`'s `maintenanceSystemPrompt` now tells the agent the
  design system exists, is frozen, and that a rejection here means "compose
  this from the design system," not a bug to route around.
- The capability was first validated against a scenario project seeded with a
  design system and a page migrated onto it as trusted setup — proving,
  directly against the `envelope` binary and free of any model, that a
  conformant change commits, a raw-`<button style={{...}}>` change is rejected
  naming the file and the rule, and `src/design-system/` is frozen under
  Maintenance but writable under Genesis.
- Recorded in [ADR 0012](adr/0012-the-design-system-invariant.md); the
  two-part invariant (frozen primitives + required use) is threat T13 in the
  [threat model](THREAT_MODEL.md), with the lint-vs-full-grammar gap as R11.
- In the current published history, no separate seeding was needed at all:
  genesis had already built the app's own component library on the design
  system (see M1 above), so this phase is purely the advisor composing a
  feature from infrastructure that already existed — the strongest form of the
  demonstration, not a constructed one.

**Where it lives.** `envelope/src/design.rs`, `envelope/src/worktree.rs`,
`envelope/src/invariants/reach.rs`, `advisor/src/loop.ts`.

**Exit criterion — met.** The infrastructure was proven free of any model
first (conformant commits, a raw-HTML change rejected naming the rule, the
frozen-vs-Genesis reach split). Then the advisor itself, reading a direct
observation asking for a "Clear filters" button, landed it composed entirely
from the existing design system — no raw HTML, no inline styles, green through
the design stage on the first attempt — advisor-authored, envelope-committed.

## M3 — Agentic UI verification (discharges R7)

**Why.** Verification today is the outcome's own build, plus — since Phase C — a
test stage and a frozen-reproducer pattern for the *specific* failing case a
sensor already captured (ADR 0011). What is still missing is the *general*
case: nothing drives the rendered page for a change with no seeded reproducer
and confirms it actually works, only that it compiles and passes whatever
tests already exist. [R7](THREAT_MODEL.md) names this the load-bearing piece
that remains.

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
