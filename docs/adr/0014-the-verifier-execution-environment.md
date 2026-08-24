# ADR 0014 — The verifier execution environment

- **Status:** Accepted
- **Date:** 2026-08-18
- **Scope:** where the outcome's own build/test/e2e commands run, and what they
  can reach while running. Re-sequences **R8** ahead of the effector
  abstraction. Does not change the clearances, the changeset lifecycle, reach,
  or any gate's pass/fail logic — only the environment those gates execute in.

## Context

The verifier runs the outcome's own commands — `npm run build`, `npm test`,
`npm run test:e2e` — in the local environment, as the operator's user. This is
carried in the threat model as **R8**, listed alongside residuals like R4
(per-service guardrails) and R6 (a tamper-evident audit trail).

That grouping is wrong, and this ADR exists to say so.

R4 and R6 describe a system that is safe but unfinished: they matter when the
deployment is long-lived or multi-service. R8 describes something different.
The threat model's own actor table says the agent is **untrusted** and "assumed
adversarial: may lie, escalate, or try to bypass." Against *that* agent, R8 is
not a gap in coverage — it is the win condition.

Trace it under Genesis, where reach is the whole workspace minus `secrets/` and
`.git/`. The agent writes application source, `package.json` dependencies, and
the build configuration the bundler loads. The envelope then executes that build
on the host. So:

- A source file the build imports executes at build time, as the operator.
- `--ignore-scripts` (ADR 0009) stops *dependency lifecycle* scripts during
  resolve and install. It does nothing about the build step itself, which is the
  step whose whole job is to run the agent's code.
- The environment that build inherits includes the operator's `HOME`, their
  network, their SSH agent, and — because the advisor authenticates through
  `~/.claude` — the operator's own Claude credentials.

Two consequences follow that the threat model does not currently state.

**First, every other gate is downstream of this one.** Design conformance, the
audit non-regression check, the test stage, the e2e stage: each is a process the
envelope spawns *after* it has already run the agent's code once. A gate cannot
be trusted to grade a program that has already had the opportunity to modify the
grader.

**Second, "never-writable" means never-writable *through adjudication*, not on
disk.** Reach denies the agent a `.git/` write when it proposes one. It does not
stop a build-time side effect from writing `.git/` directly, because that write
never passes through reach at all. This bounds what any file-based control in
`.git/` can claim — including the clearance stamp introduced to close the
argv-supplied-clearance hole. That stamp is a real fix for a real hole: it stops
the untrusted advisor *naming* its own clearance. It is not, and must not be
described as, protection against an agent that has already achieved host
execution. Nothing stored on the host is.

## Decision

### 1. R8 moves ahead of the effector abstraction

The previously-planned order was M5 (abstract the effector off git), then M6
(sandbox + authenticate the seam). R8 is promoted to its own milestone ahead of
both. The effector abstraction is an architectural nicety that makes the system
more general; R8 is the residual that decides whether the system's central claim
is true on the machine it actually runs on. Generality before soundness is the
wrong order.

R3 (authenticating the seam) stays where it was, paired with the network
deployment that makes it bind. It is separable from R8 and should not be
bundled with it.

### 2. The target: the verifier runs in a disposable, network-isolated container

The build, test, and e2e stages execute in a container: no network by default,
the changeset worktree mounted as the only writable path, no host environment
inherited, and a pinned toolchain image rather than whatever `node` the host
has. `npm ci` needs the registry, so resolve/install runs as a separate,
network-enabled step that installs from the envelope-computed lockfile and
produces no execution (it already runs `--ignore-scripts`); the *build* step,
which does execute agent code, gets no network at all.

This is deliberately a container and not an OS sandbox facility. `sandbox-exec`
is deprecated on macOS and `bubblewrap` is Linux-only; picking either buys a
platform-specific mechanism that must be written twice and still cannot pin the
toolchain. The cost is honest: the envelope gains a runtime prerequisite on a
container engine. That is a dependency on the *operator's environment*, not a
crate dependency — the zero-dependency property of the trusted core is about
what an auditor must read, and it is unaffected.

### 3. First increment: strip the environment, which costs nothing

Full containerisation is a milestone. Removing the credential-theft class is not
— it is a change to how the verifier spawns a process. Every command the
verifier runs gets an explicitly-constructed environment: a minimal `PATH`, the
variables the toolchain genuinely needs, and nothing else. No `HOME`, no
`ANTHROPIC_API_KEY`, no `SSH_AUTH_SOCK`, no inherited ambient credentials.

This does not contain a hostile build — it can still read the filesystem and
reach the network. It removes the easiest and highest-value thing such a build
would do, for roughly fifteen lines and no dependency. It ships first, and it is
not to be confused with the milestone.

### 4. Until the milestone lands, this is stated, not implied

Genesis grants an agent the ability to execute code on the machine running the
envelope. The README and threat model say so plainly, in those words, rather
than leaving it inferable from R8's wording. A reader deciding whether to run
this should not have to derive that conclusion from a residual list.

## Consequences

- **R8 is not discharged by this ADR.** This records the decision and the order.
  R8 moves from Residuals to Threats only when the build runs isolated, with a
  test that fails when the isolation is removed — the same bar every other
  discharge in this repository is held to.
- The threat model gains an explicit statement that gates executing after the
  agent's code has run are conditional on R8, and that `.git/`-resident state is
  bounded the same way.
- The environment-stripping increment is cheap enough that leaving it undone
  while citing the container milestone as the reason would be an excuse, not a
  sequencing decision.
- **Residual — a container is not a boundary against a determined escape.**
  Container escapes exist. This raises the cost of host compromise from "write a
  file the build imports" to "find a container escape"; it does not make the
  build step safe to run against a genuinely adversarial, well-resourced agent.
  Named here so the milestone is not read as closing more than it closes.
- **Pointer note.** [ADR 0013](0013-deterministic-e2e-verification.md) refers to
  the effector abstraction as "M5". Under this re-sequencing that milestone is
  **M6**. ADR 0013 is an accepted record and is not rewritten; the correction
  lives here.
