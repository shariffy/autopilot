# ADR 0005 — Changesets, charters, and observation-driven genesis

- **Status:** Accepted
- **Date:** 2026-06-30
- **Scope:** the unit the envelope adjudicates, the policy in force over it, and how
  an outcome comes into being. Extends the per-write effector
  (`envelope/src/worktree.rs`) and the reach invariant
  (`envelope/src/invariants/reach.rs`); does not change the trust thesis.

## Context

The effector adjudicates a **single write**: require a clean tree, write one file,
run the repo's build, commit on green or revert on red. That is sufficient to
*maintain* an app — small, self-contained edits that each keep the build green —
but it cannot *create* one. A fresh scaffold passes through many intermediate
states that do not build (a `package.json` with no `src`, a component importing a
module not yet written). Under per-write verification every such state is a red
build and is rolled back, so the app can never come into existence.

Two further gaps follow from the same root:

1. **Reach is fixed to a maintenance shape.** The allowlist
   (`src/components/`, `src/features/`, `src/routes/`, `src/styles/`,
   `config/flags/`) and the forbidden zones (`src/api/`, `secrets/`, `infra/`,
   `envelope/`) describe what a *maintainer* of an existing app may touch. But
   creating the app means writing exactly the forbidden things — `package.json`,
   the Vite/TS config, the API client, the auth layer. Maintenance reach and
   creation reach are different grants.

2. **Where the work comes from, and what shape it takes, is unmodelled.** The
   system is meant to act on *observations*. The richest observation is a
   predecessor app (every feature in it exists because someone needed it). The
   agent must be able to *read* such a source and then reach the **full range of
   responses a senior engineer would** — from doing little (the need may already be
   met), through extending it, extracting a slice, forking and modernising, or
   migrating incrementally, to a greenfield rebuild. A binary "amend or rebuild" is
   a junior framing; the architecture must not encode the choice at all.

These are one problem: the system needs to express creation as well as
maintenance, with the right authority for each, driven by what it observes — while
leaving *which* response to make entirely to the agent.

## Decision

### 1. The unit of adjudication is a changeset, not a write

A **changeset** is a set of writes that must collectively pass verification and
remain atomically reversible. The envelope adjudicates it in three phases:

- **begin** — assert the precondition (a clean work tree) so the baseline is
  well-defined. `HEAD` is the baseline; nothing is committed until the changeset
  closes, so the baseline needs no separate record.
- **stage** — for each proposed write: check **reach** (immediately, fail-closed),
  then write the file to the tree. No build runs here.
- **commit** — run the verifier (the repo's own build) **once** over the
  accumulated tree. On green, `git add -A && git commit` — the whole changeset
  lands as one verified commit. On red, `git reset --hard HEAD && git clean -fd` —
  the tree returns to the baseline and nothing lands.

The trust boundary is unchanged in character: **reach is enforced on every
write** (deny-by-default, per stage), exactly as before. Only **verification and
reversibility move from the write boundary to the changeset boundary** — which is
the correct boundary for them, because "fit to ship" is a property of a coherent
set of changes, not of each keystroke. Git itself carries the state across the
stateless CLI invocations, so the trusted core gains no new persistent state.

Per-write maintenance becomes the **one-stage special case** (begin → stage one
file → commit). Genesis is the **many-stage case** (begin → stage a whole scaffold
or large edit → commit, build runs once). They are the same mechanism.

### 2. Reach is a charter chosen by lifecycle stage, not by the agent's strategy

The reach lists are not a global constant; they are the **charter** granted over a
specific outcome. The charter is selected by **lifecycle stage** — whether a
governed app has been launched yet — and is the same regardless of the strategy
the agent chooses (§3). The agent's strategy must never select its own authority:

- **Genesis charter** — in force during establishment, before a governed app is
  launched. Broad *within the outcome workspace*: any path except `secrets/` and
  `.git/` (traversal out of the root remains refused by the existing lexical
  normaliser). This is the authority to bring an app into existence or to perform
  structural surgery on adopted code — its config, API client, and auth — and it is
  identical however the workspace was first populated (§3).
- **Maintenance charter** — in force after launch. The narrow allowlist fitted to
  the app's layout; `src/api/`, `secrets/`, `infra/`, and the like are frozen.

The transition **Genesis → Maintenance happens at launch and is human-gated.**
Genesis produces one atomic, verified changeset plus a written plan; the structural
gate is at *launch* — exposing a v0 to real users and data — not at the agent's
strategy decision. A human ratifies, the charter narrows, and the agent operates
autonomously thereafter. (An app already governed and launched is simply in
Maintenance; a strategy that only extends it never needs Genesis at all.)

This division is deliberate and is stated plainly rather than hidden: *you cannot
structurally bound "build me anything,"* and there are no frozen zones to protect
during Genesis because nothing real exists yet. Genesis therefore rests on **a
bounded workspace + atomic reversibility + a launch gate** (nothing reaches real
users until a human ratifies); **Maintenance** is the stage that is bounded *by
construction*, and it is what the structural-trust thesis is about. The agent acts
freely within the Genesis workspace; it cannot widen its authority over anything
real, because the only thing it governs is a workspace, and exposure is gated.

### 3. The strategy is the agent's; the architecture provides capabilities, not a menu

Observations play two distinct roles, and the system uses both:

- **Observation-as-direction** — steers *what* to do (a predecessor app, a brief,
  usage signals). It is an input the agent reads; it carries no authority.
- **Observation-as-gate** — the trusted verifier (and, later, health telemetry):
  it can *reject or roll back* a change. Authority lives here, in the envelope,
  never in the direction.

Because direction carries no authority, the agent may read a predecessor freely
through **read-only reference reach** — separate from, and not gated by, the
envelope, since reading changes nothing. Writes still cross the envelope; reads do
not. The agent observes the source itself; it is given a *pointer*, not a
pre-digested brief, and not a fixed set of options.

**Strategy is the agent's, and is open.** After observing, the agent may land
anywhere a senior engineer would — do little or nothing (the need may already be
met), extend, extract a slice, fork and modernise, migrate incrementally, or
rebuild greenfield. The trusted core encodes **no `amend|rebuild` enum**, and the
conductor pre-selects nothing. The chosen strategy lives in the agent's
human-readable `PLAN.md`, not in policy — the trust boundary is indifferent to it.

What the architecture exposes instead is a **workspace starting-point**, a
mechanical primitive the agent's plan selects and a **trusted setup step**
materialises (the agent cannot reach outside its workspace, so it never does this
itself):

- **empty** — `git init` + one clean commit;
- **a clone of a source** — copied to a clean baseline, the source untouched;
- **a copied subset of a source** — selected paths only;
- **in-place** — the workspace *is* an existing repo, required clean.

These are populations of a workspace, not strategies. In-place is available only
when the repo may be safely written (clean, owned); a live or foreign source — like
a deployed predecessor — is read-only, so the agent must clone, extract, or rebuild from
it. That is a real constraint the agent reasons about, not a default the
architecture bakes in. Whatever the starting-point and strategy, the result is the
same mechanism: **changesets against the workspace**, each green, atomic,
reversible, and reach-bounded, with nothing real exposed until launch. The boundary
holds identically across every strategy, which is exactly what frees the agent to
choose like a senior engineer.

## Consequences

- `worktree.rs` gains the begin/stage/commit phases; `adjudicate_write` is
  re-expressed as the one-stage changeset. The verifier runs once per changeset.
- The verifier is **derived from the outcome**, not hardcoded: it runs the build
  the outcome itself defines (the predecessor's own build when amending; the
  scaffold's `npm run build` when rebuilding), installing dependencies first when
  the workspace has none.
- `reach.rs` gains a charter selector (Genesis vs Maintenance) keyed to lifecycle
  stage, not to the agent's strategy. The narrow fitted allowlist is the
  Maintenance charter; the Genesis charter is workspace-confined minus `secrets/`
  and `.git/`.
- The conductor gains read-only reference tools (`list_reference`/`read_reference`)
  confined to a reference repo, and a changeset lifecycle around the loop. It
  writes a `PLAN.md` as the first staged file so the plan is committed with the
  build it produced.
- The trusted core encodes **no strategy enum**. The agent's plan selects a
  workspace starting-point (empty | clone | copied subset | in-place clean repo);
  a trusted setup step materialises it. The conductor pre-selects nothing.
- The first real run is observation-driven genesis: two observations (an admin tool
  is needed; one already exists as a deployed predecessor) → the agent observes the
  predecessor, **chooses its own strategy** (which may be to build little or
  nothing), records it in `PLAN.md`, and — if building is warranted — lands one
  green changeset for review.
- The human-gated Genesis→Maintenance transition is a named step at *launch*, not
  an automatic one; the threat model records that Genesis safety rests on a bounded
  workspace + atomic reversibility + the launch gate, rather than on structural
  bounding of the build itself.
