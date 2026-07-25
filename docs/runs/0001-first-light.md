# Run 0001 — First light

**Date:** 2026-07-25
**Milestone:** [M1](../ROADMAP.md#m1--first-light-one-real-genesis-changeset-done)
**Project:** `admin-console` (external project directory, per [ADR 0003](../adr/0003-one-system-repository-outcome-external.md))
**Outcome:** [autopilot-demo-admin-console](https://github.com/shariffy/autopilot-demo-admin-console) (external, not vendored into this repo)

This is the write-up M1's exit criterion asked for: what was observed, what
strategy the advisor chose, what landed, and what it cost. It is the record of
the genesis run whose outcome is the currently-published demo repo — the app
built here is the same one autonomous maintenance, a self-healing production
incident, and design governance were later demonstrated against, all in one
continuous history.

## What was observed

Three human-filed observations were in the project's ledger before the run:

1. **Observation 1** — staff manage users and products by hand against the
   database; a small internal admin console is needed. The first thing that
   matters is *seeing* the data — a searchable list and detail view for each —
   with editing explicitly deferred.
2. **Observation 2** — greenfield: no predecessor to adopt, no backend to
   integrate with yet. The tool must read from local fixture data and
   build/run standalone, on a small, conventional stack, with `npm run build`
   kept as the build so verification stays meaningful.
3. **Observation 3** — the console will grow over time, built by whoever is
   free, not always the same engineer. Rather than have every future screen
   re-derive its own buttons and inputs, build this first version on a small
   internal set of reusable UI building blocks.

`project.json` set `clearance: "genesis"` and pointed at an empty `./workspace`.

## The strategy the agent chose

Per [ADR 0005](../adr/0005-changesets-clearances-and-observation-driven-genesis.md)
§3, the strategy is the agent's choice. Here it chose **React 18 + TypeScript +
Vite + React Router**, CSS Modules with design tokens, and — driven by
Observation 3 — a small **hand-rolled internal component library**
(`src/components/ui/`) built on an even lower-level design-system layer
(`src/design-system/`), rather than a third-party UI kit. It recorded this
reasoning itself, in the outcome, as `docs/adr/0001-react-vite-internal-component-library.md`.

Building the component library into genesis — rather than seeding it later as
trusted setup — meant every subsequent maintenance change (including features
added on entirely separate runs afterward) composed from it from day one, and
the envelope's design-conformance stage (added to the system after this run,
see [ADR 0012](../adr/0012-the-design-system-invariant.md)) applies to the
*whole* history, not a narratively-introduced slice of it.

## What landed: five changesets, each green on its own

| # | Commit | What it did |
|---|---|---|
| — | `baseline` | (trusted setup) `establish` initialised the workspace |
| 1 | `398500a` | Bootstrap: Vite + React 18 + TypeScript, global CSS design tokens, ADR 0001 |
| 2 | `3b276c6` | Internal UI component library — `Button`, `Input`, `SearchInput`, `Card`, `Badge`, `PageHeader`, `Table` + family, `Shell` layout — built on the design-system primitives |
| 3 | `31114a5` | Domain types and fixture data: 15 fictional users, 15 fictional products, for a fictional company |
| 4 | `fc23f7a` | Users feature: searchable list and detail view, routing wired into `Shell` |
| 5 | `388fdaa` | Products feature: searchable list and detail view, complete routing |

Each commit's message carries `[envelope] verified by \`npm install
--package-lock-only --ignore-scripts && npm ci --ignore-scripts && npm run
build && npm audit --json\`` — the trusted gate's own record of what judged
it. All five builds were green; none rolled back. The lockfile the build
produced is tracked in the first commit, computed by the envelope from the
agent's `package.json` intent, never authored by the agent (ADR 0009).

Deliberately left for after launch: editing/mutations, real backend
integration (the fixture seam is isolated in `src/data/`), and — at genesis
time — no tests yet (a test harness arrived later, as trusted setup for the
runtime-envelope demonstration).

## Cost and shape of the run

- **Turns:** 77
- **Cost:** $2.1295
- **Result:** stopped cleanly — `stopped: success`
- **Journal:** the project's `advisor-audit.jsonl`

## A correction made during this run

An earlier attempt at this same genesis run, before this one, spontaneously
named the app after a real company and invented fixture data describing that
company's real products — nothing in any observation named an organisation.
That attempt was discarded before anything was published. The brief given to
the advisor (both the genesis and maintenance system prompts) now explicitly
requires any invented organisation, brand, or product to be clearly
fictional, regardless of what produced the association in the first place.
This run's fixture data is for a fictional company, "Zynthar Inc.," invented
for the occasion.

## What this run enabled

Because the design system was advisor-authored under genesis rather than
seeded later as trusted setup, the subsequent demonstrations — autonomous
maintenance from a sensor, a self-healing production incident, and a
design-governed feature — required only **three operator (human) commits** in
the entire published history: seeding a vendor catalogue and the frozen
reproducer test that a runtime fix is judged against (structurally necessary
— that oracle must never be agent-authored), the one change that was
deliberately not agent-authored either (a bug an agent would have to be
deceived into writing, which is a worse dishonesty than a labeled operator
commit), and the README. Every other commit — nine of twelve non-baseline
commits — is the advisor's own work, envelope-committed.
