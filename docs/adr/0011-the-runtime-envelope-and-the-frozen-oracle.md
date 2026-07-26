# ADR 0011 — The runtime envelope and the frozen oracle

- **Status:** Accepted
- **Date:** 2026-07-24
- **Scope:** how the envelope handles a change that passes the build gate but
  fails in operation (the R7 gap). Adds a runtime trip (`runtime.rs`,
  `envelope monitor`), a test stage in the verifier (`worktree.rs`), and a
  frozen-oracle reach zone (`reach.rs`). Does not change the changeset
  lifecycle, the clearances, or the trust thesis of ADR 0005.

## Context

Verification to date was the outcome's own `npm run build` — a real typecheck
and bundle the agent cannot self-certify, but blind to runtime behaviour. A
change can build green, commit, deploy, and still throw in production: the
classic "compiles but breaks the page" gap, carried openly in the threat model
as R7.

The system is named for flight-envelope protection, and that metaphor is exact
about where this belongs. A flight autopilot does not rewrite its control laws
in the air; when something leaves the safe envelope, protection *trips to a safe
state* immediately, and the durable fix happens later, on the ground, through a
verification gate. "Elevated error rate" therefore has two responses, not one,
and the previous design had neither.

## Decision

Two loops, and a rule about who may write the thing that judges a fix.

### 1. The fast loop: a runtime trip that takes no model input

`envelope monitor --repo <dir> --telemetry <file>` reads trusted telemetry and,
if the error rate breaches the SLO, reverts the deployed change to
last-known-good — a `git revert` stamped as the envelope's own act (ADR 0010).
No advisor is consulted; the trusted core acts alone on the telemetry.

That the safety response takes **no** model input is the point, not an
economy. The whole thesis is that nothing depends on the untrusted brain
behaving; making production recovery wait on it would contradict that at the
worst possible moment. The trip restores service first; the brain is invited to
improve things afterwards, never in the critical path.

### 2. The slow loop: a test stage in the verifier

`BuildVerifier` runs the outcome's own `npm test` after the build. A change that
compiles but breaks the page fails the changeset — the pre-commit half of the
same protection, catching before commit what the trip catches after deploy.
Skipped when the outcome has no test script, so it never blocks an outcome that
has not adopted tests. **Superseded by ADR 0013**: the test script became
mandatory rather than opt-in (its absence now fails a real changeset the same
way a failing script does), alongside a new, browser-level `test:e2e` sibling
stage — see that ADR for the policy change and why it does not also foreclose
`Establish::Clone` adopting a legacy, test-less predecessor.

### 3. The oracle must be trusted, so the agent may never author it

A test only certifies a fix if the agent cannot write the test. A lockfile-style
hole otherwise opens: the agent proposes both the fix and a test that passes for
the wrong reason. So the reproducer — the test that encodes what "fixed" means —
lives in `tests/contract/`, frozen under **every** clearance (`reach.rs`), and
its evidence comes from the sensor, not the agent: the telemetry captures the
failing case, and the frozen test replays it. The agent proposes the fix; the
trusted, frozen test judges it. This is the same discipline as the API contract
in `src/api/` and the computed lockfile of ADR 0009 — the thing that grades the
work is never the thing being graded.

## Consequences

- R7 is **partially** discharged: a build-green / runtime-broken change is now
  caught pre-commit by the reproducer and, in production, by the trip. It is not
  fully closed — see residuals.
- The two envelopes are now both real: the change-time envelope (reach,
  verification, reversibility) and the runtime envelope (trip-to-safe), the
  latter no longer a stub.
- **Residual — the oracle is only as good as its evidence.** Here the reproducer
  is a seeded, rendered test and the failing input is known from the telemetry.
  A crash-class fault (something *throws*) yields a self-validating oracle
  ("replaying this input must stop throwing"); a wrong-answer fault that does not
  throw still needs a human to say what correct is. The envelope does not invent
  correctness.
- **Residual — telemetry provenance is assumed.** The trip trusts the telemetry
  reading. A real deployment needs that feed to be authenticated and
  tamper-evident; a forged low reading would suppress a legitimate trip, a forged
  high one would revert a healthy deploy. Out of scope here; named, not hidden.
