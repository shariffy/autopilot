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
| **Agent** | **Untrusted** | Proposes actions. Assumed adversarial: may lie, escalate, or try to bypass. |
| **Reference monitor** (`harness`, `policy`, `invariants`, `guardrails`, `reversible`) | **Trusted (the TCB)** | The single chokepoint. Small and auditable on purpose. |
| **Telemetry** (`telemetry::Telemetry`) | **Trusted** | The source of truth for production health. The agent has no handle to it. |
| **The world** (frontend repo + deploy — `reversible::World`) | Protected | Only mutated via the monitor. |

The trust boundary is the `Harness::enact` chokepoint: the only path from a
proposed action to a real effect.

## Assumptions (taken as given; if false, the model breaks)

- **A1.** The trusted core is correct and cannot be modified by the agent. The
  agent cannot edit files under `envelope/` (enforced by the `reach` invariant)
  and cannot change policy at runtime (enforced by `immutable_policy`).
- **A2.** Telemetry reflects reality and the agent cannot write to it. In the demo
  this is a seeded stub; in production it is a monitoring system behind the
  `Telemetry` trait, on infrastructure the agent has no credentials for.
- **A3.** Guardrail thresholds and the invariant set are authored by trusted
  humans, reviewed, and changed only through the normal (human) code path — never
  by the agent.
- **A4.** When the agent runs as a separate process (see Residuals), the seam
  between it and the monitor is authenticated, so a compromised agent cannot
  impersonate the monitor or the telemetry source.
- **A5.** Verification reflects reality and the agent cannot forge it. In the demo
  this is a seeded stub; in production it is CI plus agentic UI verification on
  infrastructure the agent cannot influence.

## Threats and mitigations

Each threat assumes a capable, adversarial agent.

| # | Threat | Mitigation | Where |
|---|---|---|---|
| T1 | Write into a sensitive zone (secrets, build/deploy config, the core) | Deny-by-default allowlist + explicit forbidden list | `invariants/reach.rs` |
| T2 | Use `../` path traversal to slip from an allowed prefix into a forbidden one | Lexical path normalisation before the prefix check; root-escapes refused | `invariants/reach.rs::normalize` |
| T3 | Ship an unverified change, or self-certify a pass | Verification comes from the trusted verifier (CI + agentic UI checks); the agent cannot supply a result (unrepresentable in `Action`); fails closed | `verifier.rs`, `invariants/change_shape.rs` |
| T4 | Tamper with or repoint the provided backend API contract | The backend client path is a forbidden zone (deny-by-default) | `invariants/reach.rs` |
| T5 | **Fake its own health metrics** | Observed metrics are **unrepresentable** in `Action`; they are read from trusted telemetry | `types.rs` (no metric field), `telemetry.rs` |
| T6 | Ship with no guardrails to evade the outcome gate | Guardrails are trusted policy, not agent-supplied; applied to every deploy | `guardrails.rs::standard` |
| T7 | Widen its own permissions / rewrite the rules | `ModifyPolicy` is denied without inspection; the agent cannot reach the policy | `policy.rs::immutable_policy` |
| T8 | Mutate the world without going through the monitor | `World` exposes only `&` views publicly; mutators are crate-private and `&mut World` is held only inside `enact` | `reversible.rs`, `harness.rs` |
| T9 | Let a harmful change quietly persist | Outcome gate fails **closed** (unmeasured ⇒ breach) and auto-reverts; every step is logged | `guardrails.rs`, `harness.rs`, `decision_log.rs` |

## Residual risks and assumptions to discharge

These are known and **not** yet mitigated. They are the honest edge of the demo.

- **R1 — Telemetry integration (discharges A2).** The demo seeds a stub. The model
  is only as strong as a real, agent-isolated monitoring integration. Until that
  exists, T5 holds *by construction of the type* but the "trusted source" is the
  demo author.
- **R2 — In-crate mediation.** T8's guarantee within a single crate is "the world's
  mutators are `pub(crate)` and `&mut World` is held only in `enact`", so an
  in-crate mistake could still reach them. Hard isolation would require splitting
  the core into its own crate or process — deliberately not done, to keep the demo
  a single small crate.
- **R3 — No process seam yet (discharges A4).** The agent is an in-process stub.
  The real boundary is a separate process speaking a versioned, validated wire
  protocol (see ADR 0002). Authentication of that seam is unbuilt.
- **R4 — Global guardrails only.** SLOs are uniform across services; per-service or
  per-change-class thresholds are not modeled.
- **R5 — No resource bounds.** Nothing limits the rate or volume of agent
  proposals (no throttling, no budget). A flooding agent is not addressed.
- **R6 — Audit trail is not tamper-evident.** It is append-only in memory and
  written to a file; it is not signed or hash-chained, so a compromise of the host
  could rewrite it.
- **R7 — Verifier integration (discharges A5).** The demo seeds a stub. The gate
  is only as strong as a real CI + agentic-UI-verification pipeline the agent
  cannot influence. Until that exists, T3 holds *by construction of the type*
  (no self-certification) but the "trusted source" is the demo author. This is the
  load-bearing piece for a frontend: most of the risk is visual/UX regressions
  that only UI verification catches.

## Out of scope

- Compromise of the host or the trusted infrastructure itself.
- Supply-chain risk in dependencies (mitigated in practice by the core having
  **zero** runtime dependencies; CI runs `cargo audit` regardless).
- Side channels and timing attacks.

## How this stays honest

This model is verifiable against the source: every mitigation names the module
that implements it. The test suite exercises the policy denials and the outcome
gate at runtime — T1, T2, T3, T4, T5, T7, and T9 (see the `reach` and `harness`
test modules). T6 and T8 are enforced at **compile time** rather than by a test:
agent-supplied verification, metrics, and guardrails are all unrepresentable in
`Action`, and `&mut World` is never exposed — so they hold by construction. When a
residual is discharged, move it from Residuals to Threats with its mitigation and
a test.
