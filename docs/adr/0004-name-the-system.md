# ADR 0004 — Name the system

- **Status:** Accepted
- **Date:** 2026-06-29
- **Scope:** the name of the **system as a whole** — the repository that contains
  the trusted core and the brain. Not the names of the components (ADR 0001 names
  the core).

## Context

ADR 0003 made one repository the system: a trusted core (`envelope`) and an
untrusted brain (`conductor`) that together maintain an external outcome. The
repository and its git remote are still called `envelope` — but `envelope` names
only the core (ADR 0001: the *bounded region of permitted action*). Using it for
the whole conflates a part with the whole, and leaves the system itself unnamed.

The system needs its own name. Following ADR 0001's criteria, in order of weight:

1. **Conceptual fit with the system's headline idea** — not a mechanism, and not
   the headline of one component. The core's headline is the *bounded region*
   (`envelope`); the brain's is *driving the loop* (`conductor`). The **system's**
   headline is the relationship between them: an untrusted agent is *granted
   authority to act and produce an outcome, within fixed limits it cannot change.*
2. **Distinctiveness** — minimal collision with established projects, especially in
   the security / agent / policy space.
3. **Does not invert the thesis** — must not name the system after the untrusted
   half. "Trust the envelope, not the brain"; naming the system `conductor` would
   put the brain on the marquee.
4. **Register** — the demo is about what governed autonomy *makes possible*, so a
   generative name is preferred over a purely defensive one.

## Options considered

| Name | Metaphor | Fit to the system's headline | Distinctiveness |
|---|---|---|---|
| **charter** | a grant of authority within fixed, unamendable limits | **strong** — names governed autonomy itself, and maps to the mechanisms (granted scope, unamendable rules, conditional exercise) | medium — common word; no dominant dev/agent project named "Charter" |
| **trellis** | a fixed structure that directs autonomous growth | good — bounded + generative, but speaks to spatial reach only, not the unamendable-rules or verification aspects | medium — collides with Roots "Trellis" |
| **paddock / sandbox** | a bounded space something powerful moves within | medium — bounded autonomy, but "sandbox" is overloaded and undersells the verification/reversibility | low — "sandbox" especially generic |
| **aegis / bastion / warden** | shield / guard | weak — defensive only; undersells the generative point | **low** — overused; collisions (same rejection as ADR 0001) |
| keep **envelope** | bounded region | poor — that is the *core component*; conflates part with whole | n/a |
| **conductor** | orchestration | poor — names the *untrusted* half; inverts the thesis | n/a |

## Decision

**Name the system `Charter`.**

The system's headline is *bounded, unamendable authority granted to an autonomous
agent.* "Charter" names exactly that, and the design already embodies it:

- the **reach** allowlist is the *scope of authority granted*;
- **`immutable_policy`** is the charter being *unamendable* — the chartered agent
  cannot rewrite the terms it operates under;
- the **verifier**, **guardrails**, and **reversibility** are the *conditions on
  every exercise* of that authority.

The agent is *chartered*: free to act and produce an outcome, within fixed limits
it cannot widen, with every action verified and reversible. The name is generative
(one *charters* a venture) rather than merely defensive, which matches what the
demo is meant to show — what becomes possible when autonomy is safe by
construction.

The components keep their names: **`envelope`** is the boundary that *enforces* the
charter; **`conductor`** is the *chartered agent*. Charter names the whole that
arranges them.

### On the overloading objection

"Charter" is a common word (charter schools, charter flights, Charter
Communications). As with `envelope` in ADR 0001, this is a term overlap, not a
collision with a known project of the same name in this domain, and the
surrounding context disambiguates. The guard/shield family (aegis, bastion,
warden) is rejected for the same reasons as in ADR 0001: heavy overuse and direct
collisions with existing policy tooling, plus a purely defensive register.

## Consequences

- The repository and its git remote are renamed `envelope` → `charter`; the local
  working directory is renamed to match. The git remote URL is updated locally.
- The component names are **unchanged**: the Rust crate/binary stays `envelope`
  (invoked as `envelope adjudicate`), and the brain stays `conductor`. Only the
  *system* gains a name. No code, crate, or binary identifiers change.
- ADR 0001 is **not** superseded: it remains the record for the core component's
  name. Its scope is, in effect, narrowed from "the project" to "the trusted core"
  by this ADR.
- The system-root README leads with the name and the one-line thesis; component
  READMEs continue to point at it.
- If the project's emphasis later shifts from *granting bounded authority* to some
  other organizing idea, this should be revisited — but the charter framing is the
  system's headline as long as structural trust is the thesis.
