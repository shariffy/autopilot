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

Genesis ran in two passes against the same project — an initial build, then a
short follow-up once the shape of the rest of the demonstration made a fourth
need clear. Four human-filed observations were in the ledger by the end:

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
4. **Observation 4** (filed after Observations 1–3 had already landed) — the
   project had no README; add one covering what the app is, how to run it, and
   the source layout.

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
trusted setup — meant every subsequent maintenance change composed from it
from day one, and the envelope's design-conformance stage (added to the
system after this run, see [ADR 0012](../adr/0012-the-design-system-invariant.md))
applies to the *whole* history, not a narratively-introduced slice of it.

## What landed: six changesets, each green on its own

| # | Commit | What it did |
|---|---|---|
| — | `baseline` | (trusted setup) `establish` initialised the workspace |
| 1 | `398500a` | Bootstrap: Vite + React 18 + TypeScript, global CSS design tokens, ADR 0001 |
| 2 | `3b276c6` | Internal UI component library — `Button`, `Input`, `SearchInput`, `Card`, `Badge`, `PageHeader`, `Table` + family, `Shell` layout — built on the design-system primitives |
| 3 | `31114a5` | Domain types and fixture data: 15 fictional users, 15 fictional products, for a fictional company |
| 4 | `fc23f7a` | Users feature: searchable list and detail view, routing wired into `Shell` |
| 5 | `388fdaa` | Products feature: searchable list and detail view, complete routing |
| 6 | `01cdebe` | README and a placeholder `test` script, from the follow-up pass |

Each commit's message carries `[envelope] verified by \`npm install
--package-lock-only --ignore-scripts && npm ci --ignore-scripts && npm run
build && npm audit --json\`` — the trusted gate's own record of what judged
it. All six builds were green; none rolled back. The lockfile the build
produced is tracked in the first commit, computed by the envelope from the
agent's `package.json` intent, never authored by the agent (ADR 0009). The
agent also reached for the dependency-maintenance tool from that ADR on its
own mid-run (`refresh_dependencies`), unprompted.

The README the agent wrote (Observation 4) is entirely its own — no meta
account of the later phases, since it had no way to know about them. That
context — the autonomous-maintenance run, the incident, the design-governance
run, all layered on afterward — lives here and in the outcome repo's GitHub
description instead, not in a hand-edited addition to the agent's own file.

Deliberately left for after launch: editing/mutations, real backend
integration (the fixture seam is isolated in `src/data/`), and — at genesis
time — real automated tests (a placeholder `test` script; a real vitest
harness arrived later, as trusted setup for the runtime-envelope
demonstration).

## Cost and shape of the run

- **Turns:** 77 (initial build) + 74 (README follow-up) = 151
- **Cost:** $2.1295 + $1.3417 ≈ **$3.47**
- **Result:** both passes stopped cleanly — `stopped: success`
- **Journal:** the project's `advisor-audit.jsonl`

## Corrections and experiments along the way

Two things were tried and not published, kept here for the honest record:

- **A discarded first attempt.** Before this run, an earlier genesis attempt
  spontaneously named the app after a real company and invented fixture data
  describing that company's real products — nothing in any observation named
  an organisation. It was discarded before anything was published. Both
  system prompts (genesis and maintenance) now explicitly require any
  invented organisation, brand, or product to be clearly fictional, regardless
  of what produced the association in the first place.
- **An experiment in agent-authored incident data.** While building the
  runtime-envelope demonstration (Phase C), a plain, non-leading maintenance
  observation — "show each product's vendor" — was given to the advisor
  against fixture data that already contained one product with a dangling
  vendor reference, to see whether the resulting code would reproduce a
  realistic crash without any prompting either way. The advisor wrote safe
  code: a guarded lookup with an explicit fallback, and it named the
  data-integrity edge case in its own ADR unprompted. That run ($0.20) was not
  published — the deployed-and-crashed scenario in the published history was
  constructed as trusted setup instead, since inducing the same bug would have
  meant either hiding the edge case from a later run or asking outright for
  broken code, and both are a worse dishonesty than a labeled operator commit.
  The finding itself is worth recording: given a plain ask and no signal
  either way, the model's default was defensive.

## What this run enabled

Because the design system and the README were both advisor-authored under
genesis rather than seeded later as trusted setup, the subsequent
demonstrations — autonomous maintenance from a sensor, a self-healing
production incident, and a design-governed feature — required only **two
operator (human) commits** in the entire published history: seeding a vendor
catalogue and the frozen reproducer test that a runtime fix is judged against
(structurally necessary — that oracle must never be agent-authored, under any
clearance), and the vendor-lookup change that was deployed and later reverted
(deliberately not agent-authored, for the reason above). Nine of the thirteen
non-baseline commits are the advisor's own work, envelope-committed; two more
are the trusted core acting alone — the baseline and the runtime-trip revert.
