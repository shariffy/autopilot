# ADR 0001 — Project name

- **Status:** Accepted
- **Date:** 2026-06-27
- **Scope:** the name of the project and its primary artifact (the trusted
  reference monitor).

## Context

The project is a trusted reference monitor that bounds an autonomous AI agent by
construction. Its thesis is *structural trust*: the agent is free to act, but only
within a fixed, auditable region of permitted actions; anything outside that
region is structurally impossible, and every permitted action is reversible.

The name should name the **headline idea** — a bounded region of safe operation —
and work as a package/binary identifier.

### Criteria, in order of weight
1. **Conceptual fit** with the headline of the thesis (a *bounded region of
   permitted action*) — not a secondary mechanism.
2. **Distinctiveness** — minimal collision with established projects/terms,
   especially in the security domain.
3. **Memorability** and pronounceability; usable as a crate/binary name.

## Options considered

| Name | Metaphor | Fit to thesis | Distinctiveness |
|---|---|---|---|
| **envelope** | flight / operating envelope — the bounded region within which operation is safe | **strong** — *is* the headline | medium — common word; overlaps the cloud term *envelope encryption* |
| **airlock** | a mandatory mediated passage between an untrusted and a protected zone | medium — captures complete mediation + reversibility, not the bounded-region idea | medium — "Airlock" is an existing security/WAF vendor (Ergon) |
| **governor** | a mechanical device that caps output regardless of input drive | medium — captures the outcome cap, not the permitted region | medium — generic; pulled toward the "AI governance" buzzword |
| **aegis / sentinel / gatekeeper / warden** | shield / guard / gate | varies | **low** — heavily overused; "Gatekeeper" is the OPA policy controller and "Kubewarden" a policy engine — direct collisions |

## Decision

**Keep `envelope`.**

The headline of the thesis is a *bounded region of permitted action*: the agent is
free inside it and cannot act outside it. The flight/operating-envelope metaphor
names exactly that, and the design already embodies it — the policy engine defines
the region, the harness makes anything outside it impossible, and reversibility
plus guardrails keep operation inside it.

The competing names describe *mechanisms* the system also has but which are not the
headline: **airlock** is the mediated chokepoint (true of the single `enact`
entry point), and **governor** is the output cap (true of the outcome gate).
Naming the project after a mechanism would undersell the central claim.

### On the overloading objection
`envelope` is a common word and overlaps *envelope encryption*. That is a term
overlap, not a collision with a known project named "Envelope," and the
surrounding context ("the trusted core," "structural trust") disambiguates. The
precision of the metaphor for the central idea outweighs the secondary cost. The
guard/shield family (aegis, sentinel, gatekeeper, warden) was rejected for the
opposite reason: it collides with existing policy tooling and says little
specific.

## Consequences

- The name commits the project to the bounded-region framing; the README and code
  use "envelope" / "the envelope" consistently for the permitted region.
- Searchability is imperfect due to the common word — accepted for this project,
  and mitigated by context.
- If the project later emphasises the mediation/chokepoint aspect over the
  bounded-region aspect, this should be revisited — `airlock` becomes the stronger
  name at that point.
