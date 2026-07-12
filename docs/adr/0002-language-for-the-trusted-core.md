# ADR 0002 — Language for the trusted core

- **Status:** Accepted
- **Date:** 2026-06-27
- **Scope:** the *reference monitor* — the trusted computing base (policy engine,
  reversible-effect contracts, outcome gate). Not the agents, orchestration,
  telemetry, or the web admin tool.

## Context

`envelope` is a trusted reference monitor that bounds an autonomous AI agent by
construction. Its thesis is *structural trust*: the agent is untrusted and
fallible, and trust comes from a small, auditable boundary rather than from the
agent's behaviour.

That thesis raises, not lowers, the bar on the boundary. A claim that "trust is by
construction, the boundary is real" is only as credible as the boundary actually
is. So **soundness of the trusted core is a requirement of the claim itself**:
there is no version of this project in which a weaker boundary is acceptable,
because the boundary is the thing being demonstrated.

### What this component is
1. A **closed-world model**: a finite set of actions and a finite set of verdicts,
   where the correctness goal is "handle every case, provably."
2. A **security boundary over untrusted input**: it parses actions proposed by an
   LLM-driven agent — untrusted, serialized across a process boundary.
3. **Small and stable by design**: it should change rarely. It is the one part of
   the system that must not churn.
4. **Decoupled from the agent**: it runs as a separate process the agent calls
   over a versioned wire protocol, so the agent cannot reach into it. That
   interface seam *is* the trust boundary.

## Options considered

| Language | Sum types + exhaustiveness | Type soundness | Input boundary (no type erasure) | Minimal static TCB | Iteration speed | Ecosystem fit (agents are TS/Python) |
|---|---|---|---|---|---|---|
| **Rust** | yes (enforced) | sound | parse *is* validation (serde) | yes (small static binary) | slower | weaker |
| **TypeScript** | yes, but **unsound** (`any`/`as` escape hatches) | unsound | erased; needs separate runtime validators | no (Node + V8 + GC + npm) | fast | strong |
| **Go** | **no sum types**; type-switch, not exhaustiveness-checked | mostly, `nil` exists | needs separate validation | yes | fast | medium |
| **Python** | no (gradual, optional typing) | unsound | runtime checks only | no | fastest | strongest |
| **OCaml/Haskell** | yes (enforced) | sound | parse *is* validation | yes | medium | poor |

> *Memory safety* is intentionally **not** a column. Node and Go are memory-safe
> too, so it does not distinguish Rust from the realistic alternatives here — it
> would only matter against C/C++.

### Why exhaustiveness is the decisive feature
The component's correctness model is "a closed set of actions and verdicts, handle
every case, compiler enforces completeness." That makes sum types with
*compile-time-checked exhaustiveness* the single most relevant language feature.
- Rust and OCaml/Haskell have it, soundly.
- TS has it but **unsoundly** — escape hatches mean a reviewer must verify nobody
  used `any`/`as` to bypass a case; the guarantee becomes a lint-and-review burden
  rather than a compiler guarantee.
- Go and Python **lack it** outright.

### Why "same language as the agent" is not a factor
The boundary is a *separate process* the agent calls, not in-process function
calls — that isolation is a security requirement. Because the two communicate over
a wire protocol, language homogeneity between agent and monitor buys nothing, so
the natural pull toward TS/Python (the agents' ecosystem) does not apply to the
TCB.

## Decision

**Rust for the trusted core; TS/Python for everything around it** — a deliberate
polyglot split along the trust boundary.

- **Rust** for the reference monitor, the one component whose *job is to be a
  guarantee*. In order of weight: (1) sound, compiler-enforced exhaustive matching
  over the closed action/verdict model; (2) no type erasure at the input boundary —
  deserialization *is* validation, so there is nothing separate to keep in sync and
  trust; (3) a minimal static TCB; (4) the component is stable, so Rust's slower
  iteration barely bites.
- **TS/Python** for the agents, orchestration, telemetry, and web admin tool,
  where iteration speed and ecosystem fit (Agent SDK, web stack) dominate and the
  unsound-typing cost is acceptable.

The seam between them is a versioned, validated wire protocol — desirable, because
that seam *is* the trust boundary, and making it explicit is good security
hygiene rather than incidental overhead.

**On iteration speed.** The usual argument for a looser language is velocity. It
does not bite here: Rust's slower iteration falls on fast-moving code, but the
trusted core is small and changes rarely, and the fast-moving surfaces are
TS/Python regardless. Rust for the core costs almost nothing in velocity.

## Where strict TypeScript falls short

Strict-mode TS (`any` banned, `as` lint-blocked, `noUncheckedIndexedAccess`) is a
strong configuration and closes a lot. It still falls short for *this* component
in specific ways:

1. **Type erasure at the input boundary.** The agent's proposed actions arrive as
   bytes/JSON from another process. `JSON.parse` yields `any`; TS gives **zero**
   compile-time guarantee the runtime value matches the `Action` type. You must
   hand-write runtime validators (zod/io-ts) and keep them in lockstep with the
   types — and those validators, plus the trust that they match, become part of the
   TCB. In Rust, `serde` deserialization is simultaneously the parse, the
   validation, and the type: one sound step, nothing to keep in sync.
2. **Escape hatches survive `strict`.** `as`, `any`, and `@ts-ignore` /
   `@ts-expect-error` are not removed by strict mode. They can be lint-banned, but
   a lint rule is not the compiler and can be disabled per line. The guarantee
   becomes "the lint config held and nobody bypassed it," verified by CI/review —
   not "the compiler proved it." For a trusted core that distinction is the whole
   game.
3. **Exhaustiveness is opt-in, per switch.** TS only flags a missing case if you
   add the `const _: never = x` trick to that specific switch. Forget it once and a
   newly-added action variant compiles clean with a silently unhandled case.
   Rust's `match` is exhaustive by default, everywhere, with no ceremony.
4. **Structural typing erases domain distinctions.** A file path, a service name,
   and a build id are all just `string` and freely interchangeable unless you
   manually brand them. Rust newtypes make `Path` and `ServiceName` genuinely
   distinct at no runtime cost — relevant when the rules reason about exactly these
   kinds.
5. **TCB size.** TS brings Node + V8 + a GC + the npm/stdlib surface as the thing
   you are asking people to trust. The Rust core here is a small static binary with
   zero dependencies — a far smaller audit surface for a component whose entire
   value proposition is "small enough to fully trust."

## Consequences

- Operational complexity of a polyglot system and a serialization seam — accepted,
  and reframed as the explicit trust interface.
- Smaller hiring pool for the TCB than a JS/Python-only stack — acceptable for a
  small, rarely-changing component.
- The TCB's guarantees hold at the compiler level, removing "did anyone use an
  escape hatch?" from every audit of the boundary.
