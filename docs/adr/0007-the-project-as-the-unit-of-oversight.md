# ADR 0007 — The project as the unit of oversight; operated from within, state external

- **Status:** Accepted
- **Date:** 2026-07-12
- **Scope:** where the state belonging to one overseen project lives, how a project
  comes into being, and how a run selects it. Extends ADR 0003's outcome-externality
  to all per-project state; the ledger forms of ADR 0006 are unchanged.

## Context

ADR 0003 established that the outcome is external to the system repo — instance
data the system is pointed at, never vendored in. ADR 0006 gave the system its
inputs: an observation ledger, and named read-only sources configured by the
operator.

But the state that binds these together for one overseen project had no home. The
workspace path, the sources, the charter in force, the observation ledger, and the
change journal were five independent environment settings, each with a default
that pointed at one hard-coded deployment — the ledger's default even lived inside
the system checkout. Three consequences:

1. **One checkout could oversee exactly one project.** A system meant to maintain
   several outcomes would need either several checkouts or a shell environment
   swapped wholesale per run.

2. **Incoherent combinations were expressible.** Nothing grouped the five
   settings, so one project's ledger could silently run against another project's
   workspace under the wrong charter. The operator's discipline was the only
   defence, on exactly the axis — which project am I acting on? — where a mistake
   means acting on the wrong system.

3. **Creation was undocumented labour, and selection was ambient.** There was no
   way to make a project except hand-writing config against a schema that lives in
   source code, and once made it was selected by an environment variable — set in
   one shell and forgotten, acting at a distance.

The same reasoning that made the outcome external applies to all of this state: it
describes a *deployment* of Charter, not Charter itself. This repository should
carry decisions that hold for any deployment (its ADRs) and nothing that holds
for only one.

The remedy is already embodied by the project-scoped tools we reach for. `git`,
`terraform`, `vercel` are operated *from within* the thing they act on: an `init`
makes a directory the unit, and thereafter the directory you stand in names the
target. Nobody exports `REPO=…` to commit. That grammar makes creation a command
and selection visible in the prompt, and it leaves no deployment-specific default
in the tool's own source.

## Decision

### 1. The project is the unit of oversight

A **project** is one outcome together with everything the system needs to oversee
it: the workspace to establish and build, the read-only sources it may observe,
the reach charter in force, its observation ledger, and its change journal. One
system oversees any number of projects; the system itself contains none of them.

### 2. A project is one external directory

All state belonging to a project lives in a single **project home** — a directory
external to the system repo, the same status ADR 0003 gives the outcome:

```
<project>/
  project.json             what to build, from what, under which charter
  observations/NNNN-*.md   the observation ledger (ADR 0006 — form unchanged)
  conductor-audit.jsonl    the brain's change journal
  workspace/               the outcome, by default — the one agent-writable area
```

Relative paths in `project.json` resolve against the project home, so a project is
relocatable as a unit. The charter is recorded here because it is lifecycle state
*of the project* — flipped from `genesis` to `maintenance` by the operator when
the project crosses that line, never by the brain.

### 3. The system is operated from within the project, like git

The conductor's entry point is a `charter` command with three verbs (filing and
running remain different acts — ADR 0006 §3):

- `charter init` makes the **current directory** a project: it scaffolds
  `project.json` and the empty ledger, and refuses if one already exists. Creating
  a project is now a command, not documentation against a schema in source.
- `charter observe` and `charter run` resolve their project as the **nearest
  ancestor directory holding `project.json`** — run from inside the project home,
  its ledger, or its workspace.

The working directory is the only selector. There is no `PROJECT` variable and no
wired-in default: with one selector visible in the prompt, there is nothing to
set, forget, or mix, and an incoherent mixture of two projects is not expressible.
Environment configuration remains only for what is genuinely not the project's:
the envelope binary and the turn cap describe the *installation* (`ENVELOPE_BIN`,
`MAX_TURNS`), and stay env.

### 4. The workspace defaults to inside the project home; the config never is

`init` scaffolds the workspace at `./workspace`, so a project is self-contained:
one directory holds the config, the ledgers, and the outcome. The nesting is
deliberate in one direction only. The workspace may live inside the project home,
but the config and ledgers must never live inside the workspace — the workspace is
the one region the brain may write, and the brain must not be able to edit its own
charter, sources, or ledger. The outcome remains external to the *system* repo
(ADR 0003); a project home is not the system.

### 5. The system repo holds no project state

The system repo records the system — its code and its own ADRs. A project home
records one deployment. Nothing under the system repo is read or written per
project; a fresh clone oversees whatever projects it is pointed at.

## Consequences

- The conductor gains a project loader (`conductor/src/project.ts`), a `cli.ts`,
  and a `charter` bin (tsx-run, no build step; `npm link` once to put it on PATH).
  `main.ts` becomes the `run` command; the per-setting environment variables
  (`WORKSPACE`, `OBSERVATIONS`, `OBSERVE_SOURCES`, `CHARTER`, `CONDUCTOR_AUDIT`)
  and the `start`/`observe` npm scripts are gone, and the ledger and journal
  directories leave the conductor.
- The last deployment-specific defaults leave the conductor's source; what each
  project builds is entirely data in its project home. No deployment-specific path
  remains in the conductor's source.
- Overseeing a second project is creating a second directory and standing in it —
  no second checkout, no environment juggling, one shared envelope build.
- A monitoring adapter files observations the same way anything else does: by
  writing the next record into the project home it is configured with — the open
  input door of ADR 0006 is unchanged.
- Whether a project home should itself be version-controlled is the operator's
  choice per project; the observation ledger and journal are append-only either
  way, and none of it enters the system repo.
- The trust thesis is untouched: where a command is invoked from selects *which*
  project steers the brain, and observations still only steer (ADR 0006 §2); every
  write is still adjudicated by the envelope under the project's charter.
