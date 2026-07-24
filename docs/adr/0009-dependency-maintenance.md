# ADR 0009 — Dependency maintenance as a first-class changeset

- **Status:** Accepted
- **Date:** 2026-07-24
- **Scope:** how the agent proposes dependency/transitive-security changes, and
  what the trusted core computes versus what the agent may author. Extends
  `invariants::reach` (ADR 0005's clearances) and `worktree::BuildVerifier`
  (ADR 0005's per-changeset verifier); does not change the trust thesis or the
  changeset lifecycle itself.

## Context

The envelope gitignored lockfiles and excluded root files from the Maintenance
allowlist, so the agent could not perform a dependency or transitive-security
upgrade at all: staging `package.json` or `package-lock.json` was rejected, and
after a genesis commit zero lockfiles were tracked. That is a real capability
gap — "keep dependencies patched" is ordinary maintenance work, not a structural
edge case, and a system that cannot do it cannot be handed real upkeep.

Closing the gap the naive way — let the agent write `package-lock.json` like any
other file — is not safe to do, for two independent reasons specific to what a
lockfile *is*:

1. **A lockfile's integrity does not bind to what the registry publishes.** Each
   entry pairs a `resolved` URL with an `integrity` hash. npm computes that hash
   from whatever bytes it downloads from the URL at resolve time, not from a
   signed, registry-attested artifact. An agent that authors the pair controls
   both ends of the check simultaneously — it can name any URL and compute the
   matching hash itself. The hash then verifies nothing about provenance; it only
   proves the file is internally self-consistent.
2. **Installing from a lockfile executes code the lockfile names, inside the
   verifier.** `npm ci`/`npm install` run dependency lifecycle scripts
   (`preinstall`/`postinstall`/etc.) for whatever packages the lockfile
   resolves — and that install happens inside the envelope's own verification
   step, on the host that also decides "fit to ship" (R8, `THREAT_MODEL.md`). An
   agent-authored lockfile is therefore a way for the untrusted agent to choose
   code that runs in the trusted verifier, not merely code that ships in the
   bundle.

Both problems are specific to the lockfile as an artifact — not to dependency
management in general, and not to `package.json`. A manifest change (a version
range, an `overrides` entry, an exact pin) is inert data: it says what the agent
*wants*, and carries no resolved URL, no hash, no install step. The gap is real,
but the fix is not "let the agent write more files" — it is "give the agent a way
to express dependency *intent* that the trusted core turns into the artifact
itself."

## Decision

### 1. The agent proposes intent; the trusted core computes the lockfile

`package.json` joins the Maintenance write surface, by **exact-match**, not by
extending the prefix allowlist (`ALLOWED_WRITE_FILES`, checked by equality) — a
prefix rule would wrongly admit `package.json.bak`. The agent may express
anything a manifest can say: semver ranges, `overrides`, exact pins. None of
that costs expressive power relative to writing the lockfile directly.

`package-lock.json`, `pnpm-lock.yaml`, and `yarn.lock` are added to an exact-match
**never-write** list, enforced under **every** clearance — Genesis included, not
only Maintenance. This is categorical, not merely narrow: there is no clearance
under which the agent authors a lockfile, because the two problems in Context
(unverifiable provenance; lifecycle scripts inside the verifier) do not become
safe just because establishment is broader than upkeep. The denial names the real
cause ("computed by the trusted core, not authored by the agent") rather than the
generic allowlist message, so an auditor reading a rejection sees why, not just
that.

### 2. Resolution is part of verification, not part of staging

`worktree::BuildVerifier` gains a resolve step ahead of install:

1. **Resolve** — if `package.json` is among the changeset's staged paths, or no
   lockfile exists yet, run `npm install --package-lock-only --ignore-scripts`.
   This touches only the lockfile: no `node_modules`, and `--ignore-scripts`
   means no lifecycle script runs here, before the change has even reached the
   build gate.
2. **Install** — `npm ci --ignore-scripts` when `node_modules` is missing or the
   lockfile changed relative to `HEAD`. `npm ci` installs strictly from the
   lockfile and never mutates it, so a green build can never itself be a source
   of drift the next changeset has to explain.
3. **Build** — unchanged: the outcome's own build command, still detected from
   which lockfile is present (npm/pnpm/yarn). Resolution and install are npm's
   job regardless of which manager runs the build script, because
   `package-lock.json` is the one lockfile the envelope computes.
4. **Audit** — a **non-regression** gate, not a zero-vulns gate: `npm audit
   --json` against the changeset's tree is compared to the same command run
   against `HEAD`'s manifest and lockfile in a scratch directory
   (`--package-lock-only`, no install — cheap). A changeset that introduces an
   advisory ID absent from that baseline fails closed
   (`Disposition::BuildFailed`, carrying the audit summary and the newly
   introduced IDs); a changeset that leaves pre-existing findings merely
   unchanged is not blocked by them. Findings that predate this gate, or that
   arrive via an adopted predecessor, are not retroactively grounds for refusal —
   only new ones are.

   **Establishment is not a regression.** When `HEAD` carries no manifest there is
   no baseline to regress *from*, so the gate does not apply: the establishing
   genesis changeset sets the baseline and its findings are recorded rather than
   refused. The alternative — treating an absent baseline as an empty one — reads
   every advisory as newly introduced and makes the envelope unable to bring any
   real application into existence, since every mainstream stack ships some
   transitive advisory on the day it is installed. That is an inability to start,
   not a security property. Genesis is bounded instead by a disposable workspace,
   atomic reversibility, and the human launch gate (ADR 0005), with the recorded
   findings in front of that review.

Resolution producing or updating `package-lock.json` records that path into the
open changeset's membership (the same `.git/envelope-changeset` list `stage`
writes to) — an explicitly **envelope-authored** member, not agent residue. The
commit's existing path-scoped `git add -- <staged>` therefore includes it
without weakening the "commit equals the adjudicated set" property `worktree.rs`
already relies on (never `git add -A`): every member of the set was either
written by the agent and checked against reach, or computed by the trusted core
from what the agent proposed — never anything else.

### 3. Lockfiles are tracked, not gitignored

The baseline `.gitignore` written at `establish` drops `package-lock.json` (and
the other lockfile names) from its ignore list. A tracked, envelope-computed
lockfile belongs in history like any other envelope-authored artifact — reviewers
can see exactly what resolution produced, and `git diff` shows dependency drift
the same way it shows any other change. `node_modules/`, build output, and
machine-local files remain ignored: they are reproducible from the lockfile and
would only ever be residue.

### 4. A trusted operation for the pure-transitive case

Not every dependency fix needs a `package.json` edit — `npm audit fix` can move
a resolved version within an existing range with no manifest change at all. For
that case, `envelope refresh-deps --repo <dir> [--audit-fix]` runs resolution
directly (a plain re-resolve, or `npm audit fix --package-lock-only
--ignore-scripts` with the flag) and records the lockfile into the open
changeset — the same trusted-compute path as resolution-during-commit, just
invocable without an accompanying manifest write. It stages; it does not itself
commit, so the changeset still passes through the ordinary
resolve→install→build→audit gate before anything lands. The advisor exposes this
to the brain as `refresh_dependencies`, alongside `propose_write` and
`commit_changeset` — the brain proposes the operation, the envelope computes the
result, exactly as for a manifest change.

## Consequences

- `invariants::reach.rs`: `ALLOWED_WRITE_FILES` (exact-match) admits
  `package.json` under Maintenance; `NEVER_WRITE_FILES` (exact-match) refuses
  `package-lock.json`/`pnpm-lock.yaml`/`yarn.lock` under every clearance, checked
  in both `check_genesis` and `check_maintenance`.
- `worktree.rs`: `BuildVerifier::run` takes the changeset's staged paths and
  gains resolve/audit around the existing install/build; `BASELINE_IGNORES`
  drops the lockfile entries; `commit` re-reads the changeset's staged paths
  after verification so an envelope-computed lockfile is included in `git add`;
  a new `refresh_dependencies` entry point serves the pure-transitive case.
- `main.rs` gains the `refresh-deps` subcommand (with `--audit-fix`); the
  advisor's `envelope.ts`/`tools.ts` gain `refreshDependencies`/
  `refresh_dependencies` alongside the existing establish/stage/commit surface.
- `THREAT_MODEL.md` gains the audit non-regression gate as a mitigation, and
  records the honest residuals it does *not* discharge: registry
  provenance/publish attestation is still unverified (the audit gate catches
  *known* advisories, not a supply-chain compromise with none filed yet), and
  `--ignore-scripts` reduces but does not eliminate R8 (a written source file the
  build imports can still execute at build time; only the *install* step's
  scripts are suppressed).
- `ROADMAP.md` records this as a completed milestone.
