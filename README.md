# Autopilot

**Autopilot for your codebase** — it acts only within limits it can't change,
every change verified and reversible, and you can take the controls anytime.

## The problem

Your coding agent has a shell. It can `rm -rf`, `git push --force`, edit the
test suite that judges it, and widen whatever config you used to fence it in.
Every guardrail you give it is one the agent can reach.

Autopilot removes the shell. An untrusted model proposes changes; a small
trusted core in Rust decides their fate, and the model's only channel to the
world is asking. It cannot write a file, name its own permissions, or fake the
build that judges its work — not because it is well-behaved, but because none of
those things are expressible in what it is allowed to say.

The name is the thesis, read the way you would on a plane: automation that flies
routine work for you, inside hard limits it cannot exceed, with a human who can
take the controls at any moment. The mnemonic:
**the advisor proposes; the envelope disposes.**

## See it refuse

Real output from the binary, against the demo outcome below.

**It won't write outside its allowlist.** `src/data/` is the frozen data contract:

```
$ envelope stage --repo ./workspace --path src/data/products.ts
{"outcome":"rejected","violations":[{"invariant":"reach",
 "reason":"`src/data/products.ts` resolves into an explicitly forbidden zone"}]}
```

**It can't widen its own clearance.** Reach is stamped on the repository, so
there is no argument to pass:

```
$ envelope stage --repo ./workspace --path src/data/products.ts --clearance genesis
{"outcome":"error","reason":"unknown flag `--clearance`"}
```

**It can't land a change your own build rejects.** The verifier runs the
outcome's `build`, `test`, `test:e2e`, a design-conformance lint and an audit
non-regression check. Here a staged page throws on render — it typechecks and
bundles fine, and is caught by the frozen reproducer under `tests/contract/`,
which the agent can never author under any clearance:

```
$ envelope commit --repo ./workspace --intent "show each product's vendor on the Products page"
{"outcome":"build_failed","path":"","reason":"verification failed","detail":"…
 FAIL tests/contract/products-render.test.tsx > ProductsPage (frozen reproducer)
   > renders the full product list without throwing, for the fixture data as configured
 AssertionError: expected [Function] to not throw an error
   but 'Error: vendor lookup returned undefin…' was thrown"}
```

Nothing landed and the tree was reset. A green run instead returns
`{"outcome":"committed","path":"","commit":"63c3d0c"}`.

**And if something breaks after it shipped, no model is consulted.** The runtime
trip reads trusted telemetry and reverts to last-known-good on its own:

```
$ envelope monitor --repo ./workspace --telemetry ./telemetry.json
{"outcome":"tripped","reverted_to":"ca8849a",
 "detail":"error_rate 0.061 > threshold 0.02; reverted the deployed change"}
```

## How it fits together

| Part | Trust | What it is |
|---|---|---|
| [`envelope/`](envelope) | **trusted** | The reference monitor (Rust, zero-dependency) — flight-envelope protection. Decides every change: a deny-by-default reach allowlist, git-backed reversibility, and verification by the outcome's own build. Commits on green, reverts on red. |
| [`advisor/`](advisor) | **untrusted** | The brain (TypeScript) — the flight director. A Claude-driven loop that reads the outcome and proposes writes. It has no authority to act; its only channel to the world is asking the envelope. |
| the **outcome** | — | The artifact being maintained (here, a small admin web app). **External and swappable** — supplied by configuration, never vendored here. |

```
   advisor    ──propose write──▶  envelope  ──commit / revert──▶  outcome
  (untrusted)  ◀────verdict─────  (trusted)                      (external)
```

The seam is a **runtime process boundary**: the brain runs with every built-in
tool stripped, so it cannot write files, widen the allowlist, or fake the build.
One repository rather than three, because layout does not affect the trust
boundary — so it is chosen for the thing it *does* affect: an auditor clones
**one** thing and reads it top to bottom
([ADR 0003](docs/adr/0003-one-system-repository-outcome-external.md)).

## Words this repo uses

| Term | Means |
|---|---|
| **outcome** | the software being maintained — a separate repo, not part of this one |
| **workspace** | where the outcome is checked out and built |
| **reach** | *where* the agent may write; a deny-by-default path allowlist |
| **clearance** | how wide reach is — `genesis` (pre-launch) or `maintenance` (narrow, fitted). Stamped on the repo by the envelope; flipped by the operator, never the agent |
| **changeset** | one `begin → stage… → commit`; verified as a unit, lands atomically or not at all |
| **runtime trip** | the model-free revert-to-last-known-good when telemetry breaches |
| **frozen oracle** | `tests/contract/` — the reproducer judging a fix, never agent-writable |

## Run it

```sh
# build the trusted core
cd envelope && cargo build && cargo test

# install the brain; `npm link` puts the `autopilot` command on your PATH
cd ../advisor && npm install && npm link

# a project is a directory — operate from inside it, like git
mkdir ~/my-tool && cd ~/my-tool
autopilot init                                     # make this directory a project
autopilot observe "…what is noticed or wanted…"    # file into the ledger
autopilot run                                      # act on the ledger

# once the outcome exists and has launched, narrow the agent's reach
envelope clearance --repo ./workspace --set maintenance
```

A project directory holds `project.json` (the workspace to build and the
read-only sources it may observe), the observation ledger, and the audit journal.
Reach clearance is *not* in that file — the envelope stamps it on the repo
itself, so the untrusted brain has nothing to relay and nothing to disagree with
([ADR 0016](docs/adr/0016-clearance-is-a-property-of-the-repo.md)).
See [advisor/README](advisor/README.md).

## It has run

On 2026-07-25 the advisor took a small admin console from nothing to a working
app: it read three seeded observations, chose its own strategy (greenfield
React+Vite+TS over local fixtures, including building its own internal UI
component library), and landed **6 green, atomic genesis changesets** plus a
self-authored ADR, for $2.13 over 77 turns — none of it requiring the agent to be
trustworthy, every commit through the envelope's own build gate.

The outcome is a real, separate repository —
**[autopilot-demo-admin-console](https://github.com/shariffy/autopilot-demo-admin-console)**
— and its `git log` is the whole demonstration, because the author/committer
split records who proposed and who enacted, on every change:

```
author    committer  subject
--------  ---------  ---------------------------------------------------
envelope  envelope   baseline
advisor   envelope   Bootstrap project: Vite + React 18 + TypeScript…
advisor   envelope   Add internal UI component library: Button, Input…
advisor   envelope   Add domain types and fixture data: 15 fictional…
advisor   envelope   Add Users feature: searchable list page and detail
advisor   envelope   Add Products feature: searchable list page and…
advisor   envelope   Add README and npm test script
advisor   envelope   Add clickable column sort to Products table
operator  operator   operator: add vendor catalogue, vitest infra, and…
operator  operator   Display each product's vendor via the catalogue…
envelope  envelope   Revert "Display each product's vendor via the…"
advisor   envelope   Show vendor label safely; degrade gracefully…
advisor   envelope   Add "Clear filters" button to Products toolbar
envelope  envelope   operator: add Playwright e2e smoke suite
```

(Identities shortened and subjects truncated for width; 14 rows, the whole
history.)

Of 14 commits, **9 are the agent's**, each landed through the build gate. **2
carry the operator's identity**, doing only what the agent structurally cannot:
seeding the frozen test oracle a runtime fix is judged against, and constructing
the "shipped and crashed" scenario as trusted setup — an agent given that same
feature as a plain maintenance ask wrote it safely on its own, so reproducing the
incident honestly meant seeding it rather than coaxing a bug out of a later run.
**2 are the envelope acting alone**: the establish baseline and the runtime-trip
revert. The fourteenth — a Playwright suite — is operator setup carrying the
*envelope's* identity rather than the operator's, an attribution bug against
[ADR 0010](docs/adr/0010-envelope-attributes-the-trust-roles.md) that is listed
here rather than quietly counted as agent work.

Four phases, in one continuous history:

1. **Genesis** — the app built from nothing, including its own component library.
2. **Autonomous maintenance** — sortable columns, landed from a real analytics
   signal, unprompted by any human ask.
3. **A self-healing incident** — a change built green but crashed in production;
   the model-free runtime trip caught it, then the advisor landed a durable fix
   under a frozen reproducer it cannot author ([ADR 0011](docs/adr/0011-the-runtime-envelope-and-the-frozen-oracle.md)).
4. **Design governance** — a feature composed entirely from the design system
   genesis had established; the conformance stage has applied to every change
   since phase 2 ([ADR 0012](docs/adr/0012-the-design-system-invariant.md)).

## Status, honestly

Runnable, not production-hardened. The trust boundary is real, the build is a
genuine verification gate, and the git-backed adjudication path is under
automated test (`envelope/tests/worktree_lifecycle.rs`).

Two residuals are load-bearing, and neither is closed:

- **The verifier runs the outcome's build on the host** (R8). Under genesis the
  agent writes source, dependencies and build config, and the envelope executes
  them. Every later gate is a process spawned *after* the agent's code has run
  once. This is the next milestone, and until it lands, run genesis somewhere you
  don't mind losing.
- **UI verification is real for crashes, not for behaviour** (R7). A change that
  throws or blanks the page is caught pre-commit by a real headless-browser run.
  A change that renders successfully but *wrongly* is not — and that is most
  frontend risk.

Both are tracked, with everything else, in the
[threat model](docs/THREAT_MODEL.md).

## Documentation

- [advisor/README](advisor/README.md) — the brain, the loop, what lands and what can't
- [envelope/README](envelope/README.md) — the trusted core, the invariants, how the boundary holds
- [docs/THREAT_MODEL.md](docs/THREAT_MODEL.md) — assets, trust boundary, threats, and honest residuals
- [docs/ROADMAP.md](docs/ROADMAP.md) — the ordered backlog: residuals and ADR follow-ons, sequenced
- [docs/adr/](docs/adr) — architecture decisions
- [docs/runs/](docs/runs) — write-ups of real advisor runs against real outcomes

### Why the outcome is external

The outcome here is a codebase, so it is version-controlled with git — which is
why the envelope can use git as its reversibility substrate. But that is a
property of *this* outcome, not of the system. An event-log outcome would be
append-and-truncate; a database, transactions. You would never `git clone` an
event log to understand the system that writes it. The outcome is something the
system *points at and produces*, not a part you check out to understand it.
