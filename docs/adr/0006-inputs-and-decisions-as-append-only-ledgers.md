# ADR 0006 — Inputs and decisions as append-only ledgers

- **Status:** Accepted
- **Date:** 2026-07-11
- **Scope:** how observations enter the system, how the sources they point at are
  named, and how the agent records its reasoning. Supersedes the reference-repo and
  `PLAN.md` specifics of ADR 0005 (§3 and its consequences); leaves the
  changeset/charter core and the trust thesis unchanged.

## Context

ADR 0005 established the changeset core, the lifecycle charters, and that the
strategy is the agent's. In settling those, it fixed three incidental specifics
that do not generalise:

1. A single **reference repo**, read through `list_reference`/`read_reference`. But
   an observation may point at zero, one, or several readable sources, and none is
   privileged over the others. "The reference" is one instance of a general thing.

2. Observations were named but given no **form**. The system is meant to act on
   observations of any origin — a human requirement, a monitoring alert, a failing
   test, a usage signal. If each origin were a different door, the maintenance loop
   would branch on where the work came from; that is accidental complexity for what
   is one concept.

3. The agent's reasoning lives in a single, rewritten **`PLAN.md`**. That is enough
   to bootstrap, but a rewritten plan loses the history of *why* a decision was made
   and later changed. A senior engineer leaves an immutable trail of decision
   records, not one mutable plan.

These are one theme: the system's inputs and its reasoning should be **ledgers** —
append-only, source-tagged, auditable — the same shape as the change ledger the
envelope already keeps.

## Decision

### 1. Observation sources are named read-only roots — 0..n, none privileged

The agent is given a set of **sources**: named, read-only roots it may observe
(`list_source`/`read_source`). There may be none (a pure greenfield task), one, or
several. A source is a **capability** — what the agent may read — configured by the
operator, never chosen by the agent. Which sources exist, and that one happens to be
a git repo worth cloning, is data, not vocabulary in the trusted core or the tool
surface. A live or foreign source (e.g. a deployed predecessor) stays read-only; to build
on it the agent adopts a clone, which the envelope governs.

### 2. Observations are a source-tagged, append-only log — one door for every origin

An **observation** is a uniform event: a statement of what has been noticed or
wanted, tagged with its `source` (a human, a monitor, a test — metadata, not a
branch). Observations accumulate as a **series of numbered, immutable records**
(`observations/NNNN-<slug>.md`) — deliberately the *same form* as the decision-record
series they mirror (§3), not a single rewritten file. A source files an observation
by dropping the next record; nothing edits an existing one. A human requirement and
a CloudWatch alert enter identically and are treated identically. The agent reads the
series and decides; it does not distinguish who filed an observation.

This is safe to leave open — any origin may add a record — precisely because an
observation only **steers**. It carries no authority; the envelope bounds every
change regardless of what prompted it. Opening the input channel cannot widen the
agent's reach, which is what lets the system be *fed* rather than *operated*.

### 3. Decisions are an append-only ADR series in the outcome — not a single plan

The agent records each architecturally significant decision as an immutable
**decision record** (`docs/adr/NNNN-*.md`) in the outcome repo, staged and committed
inside the changeset that implements it. Genesis produces ADR-0001 — the strategy:
what was observed, what was chosen, and why, naming the options rejected — and each
later decision of consequence adds a record. Superseded decisions are marked, never
rewritten. This replaces the single `PLAN.md`: the reasoning becomes a trail with
the same append-only, auditable shape as the commit history it rides on.

### 4. Three ledgers around one seam

The system now keeps three append-only ledgers around the trust boundary, each a
series of immutable, numbered entries in the same form:

- **Observations** — the inputs (`observations/NNNN`; source-tagged; any origin; only steer).
- **Decision records** — the agent's reasoning (`docs/adr/NNNN`, in the outcome).
- **Changesets** — the enforced changes (the envelope-guaranteed commit history).

The symmetry is real but not uniform, and the asymmetry is the point: observations
and decision records are **advisory prose** — they steer and explain, and nothing
enforces them — while only the changeset ledger is the reference monitor. A decision
record is the agent's *claimed* reasoning: reviewable at the launch gate, not
machine-checked. What is verified is that the code builds and stays in reach; the
record of *why* is trustworthy only to the degree a human reads it. That is exactly
why the two prose ledgers may be open while the enforced one stays closed.

## Consequences

- Supersedes the reference-repo specifics of ADR 0005: `list_reference`/
  `read_reference` over a single `referenceRepo` become `list_source`/`read_source`
  over 0..n named sources; the establish mode `clone_reference` becomes
  source-agnostic `clone{source}`. (Landed in the conductor.)
- Supersedes the `PLAN.md` specific of ADR 0005: the agent writes
  `docs/adr/NNNN-*.md` in the outcome, committed with its changeset, in place of a
  single rewritten plan.
- The conductor gains an observation ledger — a directory of numbered, immutable
  records each carrying a `source` — read oldest-first as the standing ask. Running
  the conductor only **reads** the ledger; **filing** an observation is a separate,
  deliberate act — a dedicated `observe` command, or a monitoring adapter dropping
  the same shape. Filing and running are different verbs and do not share a command.
  The ingestion fabric itself is deferred.
- The brief instructs the agent to open ADR-0001 with the genesis strategy and add a
  record per significant decision thereafter.
- The trust thesis and the changeset/charter core are unchanged. Only the
  representation of inputs and reasoning moves to ledgers; authority remains solely
  in the envelope.
