# Run 0001 — First light

**Date:** 2026-07-23
**Milestone:** [M1](../ROADMAP.md#m1--first-light-one-real-genesis-changeset-done)
**Project:** `admin-console` (external project directory, per [ADR 0003](../adr/0003-one-system-repository-outcome-external.md))
**Outcome:** `admin-console/workspace` (external, not vendored into this repo)
**Audit journal:** `admin-console/advisor-audit.jsonl` — 1 `establish`, 33 `stage`, 5 `commit` records

This is the write-up M1's exit criterion asked for: what was observed, what
strategy the advisor chose, what landed, and what it cost. It is the first time
the brain → envelope → green-build → committed-outcome path ran end to end.

## What was observed

The project's ledger held two human-filed observations before the run:

1. **Observation 1** (`observations/0001-admin-console-for-users-and-products.md`
   in the project) — staff manage users and products by hand against the
   database; a small internal admin console is needed. The first thing that
   matters is *seeing* the data — a searchable list and detail view for each —
   with editing explicitly deferred.
2. **Observation 2** (`observations/0002-greenfield-and-self-contained.md`) —
   greenfield: no predecessor to adopt, no backend to integrate with yet. The tool
   must read from local fixture data and build/run standalone, on a small,
   conventional stack a new engineer would recognise, with `npm run build` kept as
   the build so verification stays meaningful.

`project.json` set `clearance: "genesis"` and pointed at an empty `./workspace`.

## The strategy the agent chose

Per [ADR 0005](../adr/0005-changesets-clearances-and-observation-driven-genesis.md)
§3, the strategy is the agent's choice — "do little" is a legitimate outcome. Here
it chose to build: a client-side single-page app with **React + TypeScript +
Vite**, routed with **react-router-dom**, reading from **JSON fixtures** checked
into `src/data/` behind a thin data-access module (`getUsers`/`getUserById`/
`getProducts`/`getProductById`) so the UI never imports fixtures directly —
swapping the fixture source for a real API later is a one-file change. The build
is `tsc && vite build` behind `npm run build`, so the envelope's verification
gate genuinely typechecks the whole tree and produces a real bundle, rather than
being a no-op.

It rejected Next.js (server-rendering weight not asked for, and a pull toward a
backend it was told not to build yet), plain HTML/no bundler (no component or
type model, and `npm run build` would be a no-op — weakening the gate that judges
it), and fetching fixtures over HTTP at runtime (the ask was explicitly
standalone). The agent recorded this reasoning itself, in the outcome, as
`docs/adr/0001-admin-console-foundation.md` — the self-authored decision record
[ADR 0006](../adr/0006-inputs-and-decisions-as-append-only-ledgers.md) §3 calls
for.

## What landed: five changesets, each green on its own

| # | Commit | Intent | What it did |
|---|---|---|---|
| — | `baseline` | (trusted setup) | `establish` initialised the workspace and committed `.gitignore` |
| 1 | `9b30c90` | Bootstrap admin console: React + Vite + TS app shell with routing | Manifest, TS/Vite config, entry point, app shell with routing and a home page, plus the ADR above |
| 2 | `071edbc` | Add domain types, JSON fixtures, and fixture-backed data-access layer | `User`/`Product` types, `users.json`/`products.json` fixtures, the data-access module with in-memory search |
| 3 | `5368860` | Add users feature: searchable list and detail view | Searchable users list (query held in the URL) and detail view, with shared `SearchBar`/`StatusBadge` components |
| 4 | `68ec0d5` | Add products feature: searchable list and detail view | The same list/detail pattern for products, with currency-formatted prices |
| 5 | `bdafa9c` | Add not-found route, home quick-links, and project README | Catch-all not-found route, home-page quick links into both datasets, a project README |

Each commit's message carries `[envelope] verified by \`npm run build\`` — the
trusted gate's own record of what judged it, not something the agent could
assert. All five builds were green; none rolled back.

Deliberately left for after launch, per the agent's own summary: editing/
mutations (explicitly deferred by the ask; the data layer is shaped so writes
slot in without touching pages), real backend integration (the seam is isolated
in `src/data/`), and automated tests (no test runner yet — verification today is
the type checker plus a green bundle).

## Cost and shape of the run

- **Turns:** 53
- **Cost:** $0.9606
- **Result:** stopped cleanly — `stopped: success`
- **Journal:** `admin-console/advisor-audit.jsonl` — 1 `establish`, 33 `stage`,
  5 `commit`, zero refused/rejected entries (every proposed write cleared reach
  and every changeset built green on the first attempt)

## What this run locked in

Getting here surfaced six real defects in the envelope's changeset lifecycle —
commit-the-adjudicated-set (a commit must equal exactly what was staged, not
whatever `git add -A` would sweep in), stage-auto-opens-a-changeset, untracked-
residue tolerance (the liveness fix: a real `npm install` byproduct left in the
tree must not wedge the next changeset), and baseline `.gitignore`/lockfile handling — now fixed on `main`
and locked in by real-path integration tests
(`envelope/tests/worktree_lifecycle.rs`, see [M3](../ROADMAP.md#m3--the-real-path-under-test-discharges-r9-done)).
