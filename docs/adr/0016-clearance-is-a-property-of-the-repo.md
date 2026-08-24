# ADR 0016 — Clearance is a property of the repo, not of the request

- **Status:** Accepted
- **Date:** 2026-08-18
- **Scope:** where reach clearance comes from. Introduces a persistent per-repo
  stamp (`.git/envelope-clearance`), removes `stage`'s `--clearance` flag, and
  removes `clearance` from the advisor's `project.json`. Does not change what
  either clearance permits, the changeset lifecycle, or any verifier stage.

## Context

Reach clearance decides whether the agent may write `src/api/`, `src/data/`,
`src/design-system/` and `package.json`'s `scripts` — the frozen contracts that
most of this system's threat rows depend on. It was supplied as a command-line
flag to `envelope stage`.

The caller supplying that flag is `advisor/src/envelope.ts`. The threat model's
actor table designates the advisor **untrusted**, "assumed adversarial: may lie,
escalate, or try to bypass." So the process being bounded was telling the
boundary how wide to be.

This was not theoretical. Against a copy of the published demo outcome, whose
`project.json` declared `"clearance": "maintenance"`:

```
stage --path src/data/products.ts                              -> rejected [reach]
stage --path src/data/products.ts --clearance genesis          -> staged
stage --path src/design-system/Button.tsx --clearance genesis  -> staged
```

Same bytes, same path, same repo. One word on the command line unfroze the
data/fixture contract (T4) and the design system (T13).

Two details make it worse than a missing check.

**The real defence had already been designed — for a narrower case.** ADR 0013
added `.git/envelope-changeset`'s line-1 clearance precisely so a changeset
staged under Maintenance could not be committed claiming Genesis. That marker's
value came from the same argv flag, so it faithfully recorded whatever the
untrusted caller first said. The mechanism was right and its input was wrong.

**There were two sources of truth.** `project.json` carried a `clearance` field
that the advisor read and forwarded. The envelope never read `project.json` and
had no way to check the forwarding was honest — and a config the untrusted
process parses and relays is not a control, it is a suggestion.

## Decision

### 1. The stamp

Clearance is stamped on the repo at `.git/envelope-clearance`: one line,
`genesis` or `maintenance`. `.git/` is already unreachable for the agent under
every clearance (`NEVER_WRITE_PREFIXES`), and `.git/envelope-changeset` already
establishes that envelope state lives there.

`stage` and `begin` read the stamp. `stage` loses its `clearance` parameter
entirely, so there is no argument for a caller to supply.

### 2. Fail closed, to the narrow clearance

Missing, unreadable, or anything other than exactly `genesis` reads as
`maintenance`. Never the reverse. An advisor cannot widen its reach by deleting
the stamp, truncating it, or corrupting it — every failure mode lands on the
clearance that freezes the contracts.

This also makes the change safe for repositories that predate it: the published
demo outcome has no stamp, and reads as Maintenance, which is what its
`project.json` said.

### 3. Written by establish and by the operator, never by the agent

`establish` stamps `genesis` — a freshly established workspace is pre-launch. The
operator flips it with `envelope clearance --repo <dir> --set maintenance`.

The flag is `--set`, not `--clearance`, deliberately: it lets `--clearance` be
deleted from the shared flag parser, so `stage --clearance genesis` now fails as
an unknown flag with exit 2 rather than being silently accepted and ignored. An
ignored flag would leave the seam looking closed while every existing caller kept
passing it.

### 4. One source of truth, so `project.json` loses its copy

`clearance` is removed from `project.json` and from the advisor's `Project`
type. The advisor derives it by asking the envelope (`envelope clearance --repo`),
including for choosing which system prompt to run under. A workspace that does
not exist yet reads as Genesis, since nothing has been established.

Keeping a second copy in a file the untrusted process parses would recreate the
original defect in a slower form: two values that can disagree, with the
authoritative one not being the one the operator edits.

## Consequences

- **T17** records the closed hole; T16's row now names the stamp as the source of
  the value it freezes.
- `cmd_stage` now resolves the repository *before* evaluating reach, because the
  clearance is a property of the repository. The old ordering was deliberate and
  documented — it let the seam be probed without a workspace — and that property
  is given up knowingly. Reach remains pure and deny-by-default; only the
  question "under which clearance" now requires the repo.
- That ordering change broke `autopilot run --dry-run`, whose whole purpose is
  to prove the seam against a project where nothing is established yet. It now
  checks the workspace exists first and says so plainly instead of failing with
  an unexpected verdict. Worth naming: the fix's first casualty was the command
  that demonstrates the thing being fixed.
- **Bounded by R8, and this is not a footnote.** The stamp lives on disk. Reach
  denies the agent a `.git/` write when it *proposes* one; it does nothing about
  a build-time side effect writing `.git/envelope-clearance` directly, because
  that write never passes through reach at all (ADR 0014). This closes the
  channel where the agent *names* its own clearance. It does not, and must not be
  described as, protection against an agent that has achieved host execution —
  against that agent, nothing stored on the host is a control.
- **Residual — the stamp is not authenticated.** Anything running as the
  operator can write it. That is the same trust basis as the local process seam
  (R3) and the unsandboxed build (R8), and it does not improve on either.
