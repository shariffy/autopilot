# ADR 0012 — The design-system invariant

- **Status:** Accepted
- **Date:** 2026-07-24
- **Scope:** how the envelope prevents the agent from smuggling ad-hoc UI
  markup into a maintained app. Adds a content-level verifier stage
  (`envelope/src/design.rs`, invoked from `worktree::BuildVerifier`) and
  freezes `src/design-system/` under Maintenance (`invariants::reach`). Does
  not change the changeset lifecycle, the clearances, or the trust thesis of
  ADR 0005.

## Context

Every UI surface a maintained app exposes is, so far, governed only by
*where* the agent may write (`invariants::reach`) and *whether the result
builds and tests green* (`worktree::BuildVerifier`). Neither constrains
*what a page is made of*. A maintenance changeset that adds a button today can
freely reach for a raw `<button>`, hand-roll its own `style={{...}}`, and the
envelope has nothing to say about it — reach admits the path
(`src/pages/ProductsPage.tsx` is squarely inside the allowlist), the build
compiles it, and no gate anywhere asks whether the result looks or behaves
like the rest of the app.

This already happened once, honestly, in this codebase: ADR 0002 (Phase B)
added `SortableHeader.tsx` with inline styles and explicitly wrote down why —
*"`src/index.css` is outside the maintenance reach boundary, and extracting a
separate CSS file for a single small component would be disproportionate."*
That is not a rogue agent; it is a reasonable engineer inside a boundary that
had no opinion about UI composition, taking the locally-cheapest path. Scaled
across many maintenance changesets from many models over time, "locally
cheapest" is exactly how a fitted, consistent product surface erodes into a
pile of one-off styling and inconsistent controls — not through any single bad
decision, but because nothing in the envelope ever had standing to say no.

The fix has to answer two separate questions:

1. **Where does the sanctioned UI vocabulary live, and who may change it?**
   That is a reach question — the same shape as `src/api/` (the backend
   contract) and `src/data/` (the data contract): infrastructure the agent
   composes against but does not get to redefine.
2. **Does a given staged file actually use that vocabulary, instead of
   routing around it?** That is not a reach question at all.

## Decision

### 1. Freeze the primitives — a `reach` zone, like the API/data contracts

`src/design-system/` joins `FORBIDDEN_WRITE_PREFIXES` in
`invariants::reach.rs`, checked only in `check_maintenance` — exactly the
existing shape of `src/api/`/`src/data/`. Genesis, which is how the primitives
come to exist in the first place, is unaffected: `FORBIDDEN_WRITE_PREFIXES`
is never consulted by `check_genesis`. Once an app has launched, Maintenance
freezes the zone: the agent composes UI from `Button`, `TextInput`, `Card`,
`Badge`, … but cannot fork or edit them, for the same reason it cannot
redefine the backend contract one page at a time — the thing every page
depends on cannot be a thing any single change is trusted to rewrite.

This is necessary but not sufficient. Freezing the primitives stops the agent
from *editing* the design system; it says nothing about whether a page
*uses* it. An agent fully respecting the freeze can still hand-roll a raw
`<button>` right next to an untouched, pristine `src/design-system/Button.tsx`
— reach has no way to object, because reach is not evaluating that file's
content at all.

### 2. Require its use — a content-level verifier stage, not an invariant

**Why this cannot live in `invariants/`.** The reach kernel's whole input is
`Action::WriteFile { path, bytes }` (`types.rs`) — a path and a byte *count*.
It is pure and path-only by design (`reach.rs`'s module doc), and that
purity is precisely why reach can be reasoned about exhaustively: it never
sees the bytes on the wire, so it structurally cannot judge them. "Does this
file import the design system?" and "does it contain a raw `<button>`?" are
questions about *content*, decidable only by reading the staged bytes back
off disk — which is exactly the shape of the build/test/audit stages already
living in `worktree::BuildVerifier`, not the shape of an `invariants` rule.

So `envelope/src/design.rs` is a new **verifier stage**, invoked from
`BuildVerifier::run` right after the build succeeds (ADR 0011's test stage
sits in the same run, immediately after this one). It lints every *staged*
`.tsx` file under `src/pages/`/`src/components/` — the app's UI zones — against
three rules:

- **No raw interactive elements the design system replaces.** `<button`,
  `<input`, `<select`, and `<a ` (a literal anchor; `<Link`/similar JSX
  components are unaffected) are rejected — the design system provides
  `Button`, `TextInput`, `Select`, `Link` instead.
- **No inline styles.** `style={{` is rejected — styling belongs in the
  primitives' own tokens, not hand-rolled per call site (closing exactly the
  gap ADR 0002 opened and named).
- **The design system must actually be used.** A staged UI file must import
  from `src/design-system/` (by relative path or a `@/design-system` alias) —
  otherwise the first two rules could be dodged by hand-rolling an equivalent
  that merely avoids their literal substrings.

A violation fails the changeset exactly like a failed build
(`Disposition::BuildFailed`), and the detail names the specific file and the
specific rule broken — e.g. `` `src/pages/ProductsPage.tsx`: raw `<button` —
compose `Button` from the design system instead `` — so a rejection reads as
an instruction, not a bare refusal.

### 3. Deliberately lint-level, and deliberately small

`design.rs` scans staged file text for substrings and import lines — the same
technique `worktree.rs` already uses for `npm audit --json` and
`package.json`'s `scripts` object (`parse_advisory_ids`, `json_object`) — not
a JSX/TSX parser. The crate stays zero-dependency (`Cargo.toml`); pulling in a
parser for this one stage would be a disproportionate new trusted-computing-base
surface for what is, deliberately, a small and explicit rule set.

That smallness is a scope decision, not an oversight. The rule set catches
*raw markup and inline styling* — the concrete failure mode this ADR's
Context section documents actually happening. It does **not** enforce a full
composition grammar: which component may nest inside which, prop-level
constraints (a `Button` used somewhere a `Card`'s `actions` slot expects), or
distinguishing a banned substring appearing inside a string literal or comment
from one that is real JSX. Building that grammar would mean either a real
parser (against the zero-dependency constraint) or a much larger bespoke rule
set for uncertain marginal benefit. The residual is named, not hidden — see
`docs/THREAT_MODEL.md`.

## Consequences

- `invariants::reach.rs`: `FORBIDDEN_WRITE_PREFIXES` gains
  `src/design-system/`, Maintenance-only, with tests proving Genesis is
  unaffected and Maintenance is blocked.
- `envelope/src/design.rs` (new): the rule set (`BANNED_RAW_TAGS`,
  `INLINE_STYLE_PATTERN`, the design-system import check), `check()` (the
  entry point `BuildVerifier::run` calls), and focused unit tests per rule.
- `worktree.rs`: `BuildVerifier::run` gains the design stage between the build
  and the (ADR 0011) test stage.
- `envelope/tests/worktree_lifecycle.rs`: a conformant staged page (imports
  the design system, no banned patterns) commits green; a page reaching for a
  raw `<button>` with an inline `style={{` is rejected with both rules named
  in the detail; `src/design-system/` is confirmed frozen under Maintenance
  and writable under Genesis.
- `advisor/src/loop.ts`'s `maintenanceSystemPrompt` now tells the agent the
  design system exists, is frozen, and that a violation is a rejection to
  compose around, not a bug to route through.
- A new scenario project, `admin-console-design/` (external to this repo,
  sibling to `admin-console/`), seeds the design system and a design-system
  migration of `ProductsPage.tsx` as trusted baseline infrastructure, and one
  observation asking for a small, primitive-satisfiable change (a "Clear
  filters" button). Proven directly against the `envelope` binary — no advisor
  run, no model cost — per the FREE smoke tests in the run record: a
  conformant hand-written change commits green; the same feature built from a
  raw `<button style={{...}}>` is rejected by the design stage, naming the
  rule; a write to `src/design-system/Button.tsx` under Maintenance is
  rejected by reach. Running the advisor itself against this scenario is a
  separate, paid step, deliberately not taken here — see `docs/ROADMAP.md`
  Phase D.
- `docs/THREAT_MODEL.md` gains this as a mitigation (T13) and records the
  full-grammar gap as a residual (R11).
