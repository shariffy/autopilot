# Threat model

This document defines what "secure" *means* for `envelope`. Security claims are
meaningless without naming the assets, the adversary, and the assumptions — so
those come first, then the specific threats and how the design answers each,
then an honest list of what is **not** yet covered.

The point of the system is **structural trust**: the agent is untrusted, and
safety comes from a small boundary, not from the agent's behaviour. This model is
the yardstick for whether that boundary actually holds.

**Scope.** The agent develops and maintains a **frontend** against an external,
provided backend API. There is no database; the agent touches only the frontend
codebase and the frontend deploy. The backend is out of the agent's reach by
construction.

## Assets (what we protect)

1. **Production integrity** — only healthy, verified, reversible changes reach and
   stay live.
2. **The rules themselves** — the policy/invariants that bound the agent must not
   be editable by the thing they bound.
3. **Sensitive zones** — auth, billing, secrets, and the trusted core's own code.
4. **Integrity of the provided backend contract** — the frontend must not
   silently repoint, fake, or hand-edit the external backend API client, and no
   credentials may be written into the bundle.
5. **The accountability record** — a faithful trail of what was proposed and
   what happened.

## Actors and trust boundaries

| Actor | Trust | Role |
|---|---|---|
| **Agent** | **Untrusted** | Proposes actions. Assumed adversarial: may lie, escalate, or try to bypass. In production this is the [advisor](../../advisor), a Claude-driven process. |
| **Reference monitor** (`policy`, `invariants`, `design`, `worktree`, `runtime`) | **Trusted (the TCB)** | The single chokepoint. Small and auditable on purpose. |
| **Telemetry** (the reading `runtime::monitor` is pointed at) | **Trusted** | The source of truth for production health. The agent has no handle to it and cannot name the file. |
| **Verifier** (`worktree::BuildVerifier` — the outcome's own build, test, e2e, design and audit stages) | **Trusted** | Decides "fit to commit"; runs inside the monitor, not the agent. |
| **The world** (the outcome's git work tree, via `worktree`) | Protected | Only mutated via the monitor — subject to R8. |

There is **one** enaction path: `worktree::{stage, commit}` (and the one-shot
`adjudicate_write` that wraps them), reached only by invoking the `envelope`
binary. It is a single chokepoint from a proposed action to a real effect.

Note what that effect *is*. This system's world is a **git repository**: its
terminal act is a commit, not a deploy. Rollout and the deploy pipeline are out
of band, which is why the only post-commit control here is `runtime::monitor`
reverting an already-deployed commit.

## Assumptions (taken as given; if false, the model breaks)

- **A1.** The trusted core is correct and cannot be modified by the agent. The
  policy is compiled into the trusted binary and there is no runtime channel that
  can alter it: `Action` — the entire grammar of what the agent can ask for — has
  one variant, a path to write, so a policy change is not expressible rather than
  merely refused. The agent also cannot propose a write to the core's own sources
  (`reach`). Both properties are bounded by **R8**: an agent that achieves host
  execution through the build step is outside what any of this constrains.
- **A2.** Telemetry reflects reality and the agent cannot write to it. Today it
  is a file the operator points `envelope monitor` at with `--telemetry`; in
  production it is a monitoring system on infrastructure the agent has no
  credentials for. The agent has no way to name or supply that path — it is an
  operator argument to a subcommand the advisor never invokes.
- **A3.** The error-rate threshold and the invariant set are authored by trusted
  humans, reviewed, and changed only through the normal (human) code path — never
  by the agent.
- **A4.** When the agent runs as a separate process (see Residuals), the seam
  between it and the monitor is authenticated, so a compromised agent cannot
  impersonate the monitor or the telemetry source.
- **A5.** Verification reflects reality and the agent cannot forge it. It is the
  outcome's own build, unit tests, e2e run, design lint and audit check, executed
  by the trusted core (`worktree::BuildVerifier`) rather than reported by the
  agent. Two limits bind it: the gate scripts and the specs are agent-authored at
  Genesis (see **R7**), and the whole verifier runs unsandboxed on the host (see
  **R8**).

## Threats and mitigations

Each threat assumes a capable, adversarial agent.

| # | Threat | Mitigation | Where |
|---|---|---|---|
| T1 | Write into a sensitive zone (secrets, build/deploy config, the core) | Deny-by-default allowlist + explicit forbidden list | `invariants/reach.rs` |
| T2 | Use `../` path traversal to slip from an allowed prefix into a forbidden one | Lexical path normalisation before the prefix check; root-escapes refused | `invariants/reach.rs::normalize` |
| T3 | Ship an unverified change, or self-certify a pass | Verification is the outcome's own build/test/e2e/design/audit stages, run by the trusted core after the agent's work is staged. The agent cannot supply a result — `Action` has no field for one — and a stage that cannot be run fails the changeset rather than being skipped. Bounded by **R8**: these stages execute on the host, after the agent's code has already run once | `worktree.rs::BuildVerifier`, `design.rs` |
| T4 | Tamper with or repoint the provided backend API contract | The backend client path is a forbidden zone (deny-by-default) | `invariants/reach.rs` |
| T5 | **Fake its own health metrics** | Observed metrics are **unrepresentable** in `Action` — it carries a path and nothing else. The runtime trip reads them from a telemetry file the operator names on the command line, via a subcommand the advisor never invokes | `types.rs` (no metric field), `runtime.rs` |
| T6 | Suppress or weaken the threshold it is judged against, to evade the runtime trip | The error-rate ceiling is trusted policy: a compiled-in default, overridable only by the telemetry file or an operator `--threshold` flag. The agent supplies none of the three and cannot invoke `monitor`. **Scope narrowed:** this row previously claimed a per-deploy guardrail gate. No such gate exists in the shipped system — its only implementation was the deleted in-memory simulation. What is real is the runtime trip's single ceiling | `runtime.rs` (`DEFAULT_ERROR_RATE_THRESHOLD`, `monitor`) |
| T7 | Widen its own permissions / rewrite the rules | The policy is compiled into the trusted binary, and the agent's action grammar has no verb for changing it: `Action` is a single variant carrying a path. A policy change is **inexpressible**, not merely denied — a stronger property than the explicit refusal this row previously described. The agent also cannot propose a write to `envelope/`'s sources (`reach`). Bounded by **R8** | `types.rs::Action`, `policy.rs`, `invariants/reach.rs` |
| T8 | Mutate the world without going through the monitor | The agent runs with no filesystem tools and no shell (`advisor/src/loop.ts` strips every built-in); its sole write channel is invoking the `envelope` binary, whose only mutating entry points are `stage`, `commit`, `refresh-deps` and `reset`, each of which writes through `worktree`. **This row is conditional on R8** — the verifier executes the agent's own code on the host, and a build-time side effect writes the filesystem directly without passing through any of this. Until R8 is discharged, T8 holds against a *misbehaving* agent, not against a *hostile* one | `worktree.rs`, `advisor/src/loop.ts`, and **R8** |
| T9 | Let a harmful change quietly persist | A red verifier stage lands nothing and resets the tree; after deploy, `envelope monitor` reverts on an error-rate breach and refuses (changing nothing) when telemetry cannot be read — failing closed in both directions. Every proposal, verdict and retry is recorded in the project's `advisor-audit.jsonl`, and every landed change in the outcome's git history, attributed advisor-as-author / envelope-as-committer (ADR 0010) | `worktree.rs::commit`, `runtime.rs::monitor`, `advisor/src/tools.ts` (journal) |
| T10 | A regression in the real (git-backed) changeset lifecycle — commit-scoping, reset-on-red, the clean-tree precondition — escapes to production because the I/O orchestration around the pure kernel is only smoke-tested | Real-path integration tests drive the compiled `envelope` binary against throwaway git repos with real `git` and `npm`/`tsc` builds: clean establish, commit tracks exactly the staged set, stage auto-opens a changeset, untracked build residue never wedges the next changeset, a failed build lands nothing and `reset` clears it, `begin` refuses a dirty tracked tree | `tests/worktree_lifecycle.rs` |
| T11 | Author a lockfile to smuggle an attacker-controlled `resolved`/`integrity` pair (npm binds integrity to whatever was downloaded, not to the registry's published artifact), or to make dependency lifecycle scripts execute inside the verifier | `package-lock.json`/`pnpm-lock.yaml`/`yarn.lock` are an exact-match never-write zone under **every** clearance, Genesis included; the agent may propose `package.json` intent only, and the trusted core computes the lockfile during verification, with `--ignore-scripts` on resolve and install | `invariants/reach.rs` (`NEVER_WRITE_FILES`), `worktree.rs::BuildVerifier` (ADR 0009) |
| T12 | Introduce a dependency with a known, filed security advisory — directly or transitively | Audit non-regression gate: `npm audit` on the changeset's tree is compared against the same command run against `HEAD`'s manifest and lockfile; a changeset that introduces an advisory ID absent from that baseline fails closed. Pre-existing findings (inherited, or predating this gate) do not themselves block. Where `HEAD` carries no manifest there is no baseline to regress from, so the establishing genesis changeset records its findings for the human launch gate rather than being refused | `worktree.rs::BuildVerifier::run` (ADR 0009), `tests/worktree_lifecycle.rs` |
| T13 | Introduce ad-hoc UI — a raw `<button>`/`<input>`/`<select>`/`<a>` or a hand-rolled `style={{...}}` — bypassing the sanctioned design system, one maintenance changeset at a time (concretely: ADR 0002 added inline styles and named reach as the reason) | Two-part invariant: `src/design-system/` is a frozen `reach` zone under Maintenance (the agent composes from the primitives, cannot fork or edit them), and a content-level verifier stage lints every staged `.tsx` file under `src/pages/`/`src/components/`, rejecting the banned raw tags and inline styles and requiring at least one design-system import — a `reach` invariant alone cannot express the second half, since reach sees a path and a byte count, never the bytes | `invariants/reach.rs` (`FORBIDDEN_WRITE_PREFIXES`), `design.rs`, `worktree.rs::BuildVerifier::run` (ADR 0012), `tests/worktree_lifecycle.rs` |
| T14 | Ship a change that builds green but breaks at runtime (the R7 gap) | Two layers (ADR 0011): a **test stage** runs the outcome's own `npm test` after the build, so a compile-clean change that breaks a tested path fails the changeset pre-commit; and a model-free **runtime trip** (`envelope monitor`) reads trusted telemetry and, on an error-rate breach, `git revert`s the deployed change to last-known-good, restoring service before any advisor is consulted. The reproducer that judges a fix lives in a frozen `tests/contract/` zone the agent can never author. As of ADR 0013 the test stage is **mandatory, not opt-in**: its absence now fails a real changeset the same way a failing script does (it is no longer skipped when the outcome has no test script), so an outcome that never adopted tests at all is caught by this stage too, not exempted from it — see ADR 0013 for why the policy changed and the `changeset_is_open` mechanism that keeps `Establish::Clone` still able to adopt a legacy, test-less predecessor | `runtime.rs`, `worktree.rs::BuildVerifier` (test stage), `invariants/reach.rs` (frozen oracle), `tests/worktree_lifecycle.rs` |
| T15 | Ship a change that builds green and passes unit tests but **throws or fails to render** at runtime, with no seeded reproducer and no prior incident. This is the crash class only — a change that renders successfully but renders the *wrong thing* is **not** covered, and R7 records that as still open | An outcome-authored `tests/e2e/` suite, run as its own stage immediately after the unit-test stage, drives a real (headless) browser against the actually-built artifact (`vite build`'s `dist/`, not a jsdom stand-in) and fails the changeset on any thrown or unhandled render error. Both `test` and `test:e2e` are **mandatory** (ADR 0013) — absence fails the changeset, including the first genesis changeset — so this holds for every changeset this system produces, not conditionally on the outcome having opted in | `worktree.rs::BuildVerifier` (e2e stage, ADR 0013), `tests/worktree_lifecycle.rs` |
| T16 | Replace a gate script with a vacuous no-op (or inject a `pretest`/`posttest` hook that silences it) in the same `package.json` changeset that introduces a bug — the gate runs the tampered command and reports green | Under Maintenance, the `scripts` object in `package.json` must be **byte-identical** to `HEAD`'s — whole object, not just the three gate keys, so an auto-run npm hook cannot sneak past. The clearance is persisted as line 1 of the changeset marker file (`.git/envelope-changeset`, a never-writable zone for the agent) at `open_changeset`/`stage` time and read back by `commit`. As of ADR 0016 the value it freezes comes from the repo's own clearance stamp (T17), not from an argument the caller supplied — preventing a stage-under-Maintenance/commit-claiming-Genesis split. Genesis writes scripts freely; if HEAD has no `package.json` there is no baseline to compare against | `worktree.rs` (`open_changeset`, `changeset_clearance`, `BuildVerifier::run` scripts-freeze step, ADR 0013), `tests/worktree_lifecycle.rs` |
| T17 | Name its own reach clearance — the untrusted advisor passed `--clearance genesis` to `envelope stage` and the trusted core believed it, unfreezing `src/api/`, `src/data/`, `src/design-system/` and `package.json`'s `scripts` on a repo the operator had put under Maintenance | Clearance is a property of the **repo**, not of the request: a persistent stamp at `.git/envelope-clearance` (a never-writable zone under every clearance), written by `establish` and by the operator-only `envelope clearance --set`, read by `begin`/`stage`. `stage` no longer takes a clearance argument at all, and `--clearance` is rejected as an unknown flag rather than silently ignored. Reads **fail closed**: missing, unreadable, or anything but exactly `genesis` resolves to Maintenance, so reach cannot be widened by deleting or corrupting the stamp. Bounded by **R8** — this closes the channel where the agent *names* its clearance, not host execution, since a build-time side effect writes `.git/` without passing through `reach` | `worktree.rs` (`stamped_clearance`, `set_clearance`), `main.rs` (`cmd_stage`, `cmd_clearance`), ADR 0016, `tests/worktree_lifecycle.rs` |

## Residual risks and assumptions to discharge

These are known and **not** yet mitigated. They are the honest edge of the system.

- **R1 — Telemetry integration (discharges A2).** `runtime::monitor` reads an
  operator-supplied file. The model is only as strong as a real, agent-isolated
  monitoring integration. Until that exists, T5 holds *by construction of the
  type* — the agent cannot express a metric — but the "trusted source" is a file
  on the operator's disk, and nothing authenticates it.
- **R2 — In-crate mediation.** T8's guarantee is that every mutating path runs
  through `worktree`'s functions, which are reached only from the CLI
  subcommands. That is a property of how the crate is written, not one the
  compiler enforces: an in-crate mistake — a new subcommand, or a helper that
  writes the tree directly — could bypass it without failing to compile. Hard
  isolation would require splitting the core into its own crate or process,
  deliberately not done to keep the TCB a single small crate. (Until the
  simulation was removed this residual described `&mut World` and `pub(crate)`
  mutators; that machinery no longer exists, and the real path never had it.)
- **R3 — Process seam exists; authentication does not (partially discharges A4).**
  The real agent (the advisor) is now a separate process, and its only write
  channel is invoking the `envelope adjudicate` binary — it cannot reach the
  world any other way. What is *not* built is authentication of that seam: trust
  currently derives from the local process boundary (whoever runs the advisor
  controls the local exec), not from a versioned, authenticated wire protocol (see
  ADR 0002). A network-deployed agent would need that.
- **R4 — One global threshold.** The runtime trip judges a single `error_rate`
  ceiling for the whole outcome. Per-service, per-route or per-change-class
  thresholds are not modeled, and no metric other than error rate is consulted —
  a change that tanks a success rate or a latency budget without raising errors
  does not trip.
- **R5 — No resource bounds.** Nothing limits the rate or volume of agent
  proposals (no throttling, no budget). A flooding agent is not addressed.
- **R6 — Audit trail is not tamper-evident.** It is append-only in memory and
  written to a file; it is not signed or hash-chained, so a compromise of the host
  could rewrite it.
- **R8 — The verifier executes the repo's build (new TCB surface).** Running
  `npm run build` executes whatever the build pipeline does on the host. The build
  *definition* is protected — `package.json`'s `scripts` object is frozen
  byte-for-byte under Maintenance (T16), lockfiles are an exact-match never-write
  zone (T11), and the `scripts` object is writable only under Genesis — so the
  agent cannot alter the gate commands it is judged by after the outcome is
  established. A written source file that the build configuration imports could
  still in principle run at build time. Two consequences are worth stating
  plainly (ADR 0014): **every later gate is downstream of this one** — design,
  audit, test and e2e are each processes spawned after the agent's code has
  already executed once — and **"never-writable" means never-writable through
  adjudication, not on disk**, since a build-time side effect writes `.git/`
  and everything else directly, never passing through `reach`. That bounds what
  any file-based control the envelope keeps on disk can claim, the clearance
  stamp (T17) included. `--ignore-scripts` on resolve and install
  (ADR 0009) narrows this — dependency lifecycle scripts no longer run during those
  steps — but does not close it: the build step itself is unsandboxed, and a source
  file the build imports still executes on the host. Hardening
  (sandboxed/network-isolated build, pinned toolchain) is not yet done; the build
  runs in the local environment.
- **R10 — Registry provenance and publish attestation are not verified.** The
  audit gate (T12) is a non-regression check against **known, filed** advisories —
  `npm audit`'s database. It says nothing about whether the tarball a `resolved`
  URL points to is actually what its maintainer published, or about npm's
  provenance/attestation tooling (`npm audit signatures`, sigstore attestations) —
  neither is checked. A supply-chain compromise with no advisory filed yet is
  invisible to this gate; it defends against *regressing on what is already known*,
  not against a novel, unfiled compromise.
- **R11 — The design-conformance lint (T13) is a subset, not a full composition
  grammar.** `design.rs` is deliberately lint-level — substring/line scanning, not
  a JSX/TSX parser, to keep the crate zero-dependency — and the rule set is
  deliberately small: banned raw tags, inline styles, and import presence. It does
  not know which component may nest inside which, does not check prop-level
  constraints, and cannot distinguish a banned substring appearing inside a string
  literal or comment from one that is real JSX (an accepted false-positive
  tradeoff, not a false negative — such a file still gets rejected, just for a
  reason slightly broader than intended). A change that imports the design system
  and avoids every banned substring but still composes an incoherent or
  off-brand result — e.g. deeply nested primitives in a way no design review
  would approve, or a prop combination the design system technically allows but
  never intends — is not caught. The gate narrows "ad-hoc UI" to "ad-hoc UI built
  from banned raw markup"; it does not make every staged page well-designed.
- **R7 — UI verification is real for crashes, not for behaviour (narrowed by
  ADR 0013; partially discharges A5).** The typecheck/compile half was already
  genuine; ADR 0011 added the test stage and the runtime trip (T14); ADR 0013
  adds a mandatory e2e stage that drives a real browser against the real built
  artifact (T15). **The crash class is now closed** — a change that throws or
  blanks the page is caught pre-commit, with no seeded reproducer needed.
  What R7 named and this does **not** close: a change that renders
  successfully but wrongly. R7's original wording is worth keeping honest
  about — *"most residual frontend risk is visual/UX regressions that compile
  and pass existing tests but still look or behave wrong"* — and that majority
  is untouched by a gate whose only question is "did it throw." The
  *agentic* UI verification R7 explicitly named, judging arbitrary rendered
  behaviour rather than replaying a reproducer, is unbuilt (see R13's shared
  mitigation). Two further limits belong here: a non-throwing wrong-answer
  fault still needs a human to say what correct *is*, and presence of
  `test`/`test:e2e` is a floor rather than a quality bar — nothing stops
  either passing vacuously (an empty suite, a no-op assertion), and the specs
  are themselves agent-authored at Genesis. The human launch gate (ADR 0005)
  currently covers all of that.
- **R13 — No qualitative or security code review.** A change that clears every
  deterministic gate (build, design, test, e2e, audit) can still be poorly
  written or contain an agent-introduced vulnerability no existing gate checks
  for. Named as a sibling gap to R7 — both are the same underlying pattern, an
  agent's work graded only by deterministic checks its own errors can slip
  past — sharing a future mitigation: a deliberately-designed agentic-judge
  pattern, not built yet (see ADR 0013 and `ROADMAP.md`'s "Not on this
  roadmap"). Both are newly exposed to indirect prompt injection the moment
  such a judge reads agent-authored content, which is why neither is rushed in
  ahead of that design work.

## Out of scope

- Compromise of the host or the trusted infrastructure itself.
- Supply-chain risk in dependencies (mitigated in practice by the core having
  **zero** runtime dependencies; CI runs `cargo audit` regardless).
- Side channels and timing attacks.

## How this stays honest

This model is verifiable against the source: every mitigation names the module
that implements it, and **every module it names is on the path the system
actually runs.** That was not previously true. Until the in-memory simulation was
deleted, T3, T5, T6, T8 and T9 cited `harness.rs`, `reversible.rs`,
`verifier.rs`, `telemetry.rs`, `guardrails.rs`, `change_shape.rs` and
`decision_log.rs` — a parallel implementation reachable only from a demo
printout, which no advisor ever invoked and which could not touch a repository.
Those rows have been restated against the real path, and where the real path has
no equivalent (the per-deploy outcome gate of the old T6) the row says so rather
than quietly inheriting the simulation's guarantee.

The pure invariants are unit-tested (T1, T2, T4, T7's reach half, T11 — see the
`reach` test module). T3, T10, T12, T13, T14, T15, T16 and T17 are exercised
end to end against the compiled binary and throwaway git repositories, with a
real `npm audit`, a real `tsc`/`vite build`, and a real headless-browser run
against the actually-built artifact (`tests/worktree_lifecycle.rs`).

T5 and T7 hold at **compile time** rather than by a test: `Action` has exactly
one variant carrying exactly one field, so agent-supplied metrics, verification
results and policy edits are unrepresentable — not refused at runtime, but
unsayable. T9's audit half and T6 rest on the agent having no channel to invoke
`monitor` or supply its inputs.

When a residual is discharged, move it from Residuals to Threats with its
mitigation and a test.

### Two rules for changing this document

Both exist because both have already been broken here, and each cost two
milestones before anyone noticed.

1. **Quote a residual before discharging it.** Paste its current text into
   whatever proposes the discharge, and check the mitigation against every
   clause. M3 was written up as discharging R7 while R7's own text named
   agentic UI verification and called visual/UX regressions *"most residual
   frontend risk"* — neither of which the e2e stage touches. Closing a
   residual's smaller half is not a discharge; the preamble above already
   says so, and it was still missed by everyone who cited R7 without
   re-reading it.
2. **When a change alters what a threat depends on, grep this file for
   claims about it.** ADR 0009 moved `package.json` into the Maintenance
   write surface and left R8, in the same diff, asserting `package.json` was
   *"outside the write allowlist, so the agent cannot alter the verifier it
   is judged by."* The claim was falsified by the commit that shipped it. A
   `grep -n allowlist docs/THREAT_MODEL.md` before writing the ADR is the
   whole fix.
