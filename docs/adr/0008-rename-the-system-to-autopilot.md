# ADR 0008 — Rename the system to Autopilot

- **Status:** Accepted
- **Date:** 2026-07-23
- **Scope:** the name of the **system as a whole** (supersedes ADR 0004), and the
  two names that cascade from it: the brain (`conductor` → `advisor`) and the
  domain enum for the reach authority in force (`Charter` → `Clearance`). The core
  crate/binary name (`envelope`) and the `Genesis`/`Maintenance` stage values are
  unaffected.

## Context

ADR 0004 named the system `Charter`: a grant of bounded, unamendable authority to
an autonomous agent. That framing is accurate but abstract — a general developer
audience does not carry "charter" straight to "bounded autonomous agent," and nothing
in the word itself suggests a human retains the controls.

The architecture is, and has always been, an **aviation** shape: an automation that
flies routine work autonomously, inside hard limits it cannot exceed, with a human
who can take over at any moment. **Autopilot** names that directly, and it pulls a
whole coherent vocabulary with it rather than one isolated term:

- the brain that *reads sensed state and proposes a correction but never
  actuates* is, in aviation, the **flight director** — an advisory organ, not a
  pilot;
- the trusted core's job is exactly **flight-envelope protection**: the hard
  physical/policy limits the automation cannot fly outside of, regardless of what
  the automation asks for;
- the reach authority in force at a given lifecycle stage is, in aviation, a
  **clearance** — an authorization to operate within specified limits, granted by
  someone other than the one operating.

Renaming the system surfaces two cascading renames and one that is conspicuous by
its absence:

1. `conductor` (a music metaphor: one who directs performers) is now the odd
   member of the family, and its ordinary reading — "the one in charge, driving
   the performance" — is close to backwards for a component whose entire point is
   that it has **no authority to act**. It only proposes.
2. The `Charter` enum (`Genesis` | `Maintenance`, selecting the reach allowlist in
   force) inherited its name from the old system brand. Under Autopilot, the
   aviation-native term for "an authorization to operate within specified limits"
   is **clearance**, and it is a better fit for what the enum actually is: not the
   grant of authority itself (that's the reach allowlist), but *which* grant is
   currently in force.
3. `envelope` needs no new name. Under the old frame it was already "the bounded
   region of permitted action" (ADR 0001); under Autopilot it is *literally* what
   the aviation term means — envelope protection, the trusted core enforcing the
   limits an autonomous system cannot be allowed to exceed on its own judgment.
   The rename makes this component's name *more* apt, not less.

## Options considered

| Name | Metaphor | Fit to the system's headline | Risk |
|---|---|---|---|
| **Autopilot** | automation that flies routine work within hard limits, human can take over anytime | **strong** — names the whole relationship (bounded autonomy + handoff), and pulls a coherent component vocabulary (advisor / envelope / clearance) with it | audience may read "autopilot" as "the autonomous doer" and credit the brain, missing that the envelope is what's trusted — mitigated by positioning (below), not by the name alone |
| keep **Charter** | a grant of authority within fixed, unamendable limits | accurate, but abstract; does not suggest a human retains the controls; no natural component family follows from it | none new, but the audience-legibility gap that motivated this ADR persists |
| **Copilot** | an assistant that helps but does not decide | weak — undersells the trust boundary; "copilot" already means "AI pair programmer" industry-wide (GitHub Copilot et al.), so it would be read as *the brain*, inverting the thesis the same way `conductor`-as-system-name would have | **high** — direct collision with the dominant existing usage |
| **Governor** | a mechanical limiter (steam-engine governor) | good bounded-authority fit, but purely mechanical/defensive register, no natural "human can take over" component, and no distinct name left for the brain | medium |

## Decision

**Rename the system `Charter` → `Autopilot`.**

Two component names cascade from the aviation frame; one component keeps its name
because the frame makes it fit *better*:

- **`conductor` → `advisor`.** In aviation, the flight director reads instruments
  and proposes a correction but has no control surfaces of its own — exactly the
  brain's role. `advisor` says this plainly and drops the orchestration
  connotation that read as authority `conductor` never actually had.
- **`envelope` — unchanged.** Flight-envelope protection *is* what the trusted
  core enforces: the hard limits an autonomous system operates inside of,
  regardless of what it proposes. The rename strengthens this name rather than
  orphaning it.
- **`Charter` (enum) → `Clearance`.** The enum selects which reach authority
  (`Genesis` or `Maintenance`) is in force for a project. "Clearance" is the
  aviation-native word for an authorization to operate within specified limits,
  granted by someone other than the one operating — a closer fit than the old
  system-brand-derived name. The `Genesis`/`Maintenance` values themselves are
  untouched; only the enum's name and the wire flag/field that carry it change:
  `--charter` → `--clearance` on the `envelope adjudicate`/`stage` CLI, and the
  `charter` field in `project.json` → `clearance`.

The mnemonic for the whole system: **the advisor proposes; the envelope
disposes.**

### On the audience-perception risk

"Autopilot" carries real risk of being read as "the autonomous thing that decides
on its own" — crediting the brain, which is exactly the inversion the system's
thesis exists to prevent (ADR 0004's criterion 3, carried forward here). This is
not disqualifying, because the aviation frame itself carries the correction:
autopilot is *definitionally* bounded and supervised — flight envelope
protection and a human who can take the controls are part of what the word
already means, not an addition to it. The mitigation is **positioning, not
renaming**: every surface that introduces the system leads with limits and
handoff before autonomy — *"Autopilot for your codebase: it acts only within
limits it can't change, every change verified and reversible, and you can take
the controls anytime."* This is a documentation obligation the READMEs carry, not
a property the name guarantees on its own.

### On the breaking `project.json` change

Renaming the `charter` field to `clearance` in `project.json` is a breaking
on-disk format change: any project directory created before this ADR has a
`project.json` with the old field name and will not parse until the field is
renamed by hand. This is accepted without a migration path — the project format
has no external installations to preserve compatibility for at this stage — and
is noted here so it is not mistaken for an oversight.

## Consequences

- The CLI command renames `charter` → `autopilot` (bin file, `npm link` target,
  all help text and examples).
- The directory and package `conductor/` → `advisor/` (git history preserved via
  `git mv`); the audit journal filename `conductor-audit.jsonl` →
  `advisor-audit.jsonl`.
- The Rust `Charter` enum → `Clearance` (`envelope/src/invariants/reach.rs`,
  `policy.rs`, `main.rs`); the CLI flag `--charter` → `--clearance`. The TS
  `Charter` type → `Clearance` (`advisor/src/envelope.ts`, propagated through
  `project.ts`, `cli.ts`, `run.ts`, `observe.ts`, `tools.ts`, `loop.ts`). This
  crosses the advisor↔envelope process seam, so both sides change together — a
  half-renamed seam is not a valid intermediate state.
- `project.json`'s `charter` field → `clearance` — a breaking, unmigrated on-disk
  change (see above).
- ADR 0004 is marked **Superseded by this ADR**. Its reasoning is left
  unrewritten as a historical record of why `Charter` was chosen at the time; it
  legitimately still says "Charter" throughout. ADR 0001 (naming `envelope`) is
  unaffected — `envelope` keeps both its name and, under this rename, a tighter
  fit to what it does.
- Every surface that introduces the system (root README, `advisor/README.md`)
  leads with the limits-and-handoff framing, not an autonomy claim, as the
  standing mitigation for the audience-perception risk noted above.
