# ADR 0003 — One system repository; the outcome is external

- **Status:** Accepted
- **Date:** 2026-06-28
- **Scope:** the repository structure of the project as a whole — how the trusted
  core, the agent, and the thing being maintained are arranged. Not the internal
  design of any one component.

## Context

The project is **a system that builds an outcome**. The *system* is the trusted
core (the reference monitor) plus the untrusted brain (the agent that proposes
changes). The *outcome* is the artifact the system maintains — here, a frontend
codebase, but it could equally be an event log, a database, or a config bundle.

Two facts about this arrangement drive the structure:

1. **The outcome's storage is a property of the outcome type, not of the system.**
   This outcome is a codebase, so it is version-controlled with git — and that
   happens to make git a convenient reversibility substrate. But that is a
   coincidence of *this* outcome. An event-log outcome would be an append-only
   file; a database outcome would be transactions. Nobody would expect an auditor
   to `git clone` an event log to understand the system that writes it. The outcome
   is something the system **produces and points at**, not a part of the system.

2. **The trust boundary is a runtime process seam, not a repository boundary.**
   The brain reaches the world only by invoking the trusted core as a separate
   process over a validated channel (`envelope adjudicate`), and the core decides
   every write. That isolation is enforced at runtime — by the process boundary and
   the reach allowlist — and is entirely independent of how the source is arranged
   into repositories. Splitting the core and the brain into separate repos does not
   strengthen the seam; co-locating them in one repo does not weaken it.

The consumer that this structure must serve is the **auditor**: someone who needs
to answer "what does this build, and how is it bounded?" That question is about the
*system*, and it should be answerable from one place, read top to bottom, without
chasing cross-links between repositories or cloning the outcome at all.

## Options considered

| Option | Audit in one clone | Outcome swappable without touching the system | Trust seam preserved | TCB independently vendorable | Compose ceremony |
|---|---|---|---|---|---|
| **A. One repo per component; outcome a peer repo** | no — follow cross-links across repos | no — the outcome is wired in as a sibling | yes | yes | high (needs a root/manifest to declare the whole) |
| **B. One system repo (core + brain as subdirectories); outcome external** | **yes** | **yes** | **yes** | no (the core is a subdirectory) | none |
| **C. TCB standalone repo + brain repo; outcome external** | partial — two clones | yes | yes | yes | low |

The decisive column is **trust seam preserved**: it is `yes` for every option.
Because the seam is a runtime process boundary, repository layout cannot affect it
— which removes the only reason to keep the system fragmented and lets the choice
turn purely on auditability.

## Decision

**One repository is the system.** It contains the trusted core and the brain as
two clearly-bounded subdirectories. **The outcome is external** — supplied to the
system by configuration (a path the system is pointed at), never vendored into the
system repo and never a peer to be cross-linked.

- The system repo is the single unit an auditor clones and reads. Its entry
  document states the model — *a system that builds an outcome* — before any tour
  of the parts, so the structure is understood by reading, not by reconstructing
  it from links.
- The trusted core remains a clearly-bounded subdirectory with its own build,
  tests, and zero-dependency discipline intact (per ADR 0002). It is a *bounded*
  part of one repo, not a fused one.
- The outcome is whatever the system is pointed at for a given run. Swapping it
  requires no change to the system.

### Why not keep the trusted core standalone (Option C)

The one real argument against Option B is that a trusted computing base, like a
security library, has value as an independently-versioned, separately-auditable
artifact that several brains could depend on. That benefit is **hypothetical here**
and the cost is **concrete**: Option C forces an auditor to clone and correlate two
repositories to understand one system. Auditability of the actual system outweighs
the option value of vendoring the core elsewhere. If a second consumer of the core
ever materialises, extracting a bounded subdirectory into its own repository is a
mechanical change — the subdirectory boundary is kept clean precisely so that door
stays open.

### Why not a peer-repo arrangement (Option A)

Treating the outcome as a peer repository mistakes the *output* for a *component*.
It is the arrangement that creates the "no declared root" problem in the first
place — three peers with nothing that says they are one system — and then demands a
manifest or a web of cross-links to paper over it. Removing the outcome from the
system's repository removes the problem rather than composing around it.

## Consequences

- The trusted core, currently a standalone repository, becomes a bounded
  subdirectory of the system repository; the brain moves in alongside it. The
  core's internal structure, build, and tests are unchanged.
- The system repo's documentation leads with the system/outcome model and stops
  describing the maintained artifact as a sibling project; the artifact's own
  documentation may note that it is *an* outcome this system can maintain, but the
  system does not depend on it.
- The ADR log (this directory) is the **system's** decision record. ADRs 0001 and
  0002, written when the core was the whole of the repository, remain valid as
  records scoped to the core.
- Because the outcome is now explicitly external and swappable, the reversibility
  substrate inside the core must be an **abstraction** (git is the right
  implementation for a codebase outcome, an append/truncate effector for a log, a
  transaction for a database) rather than a hardcoded assumption that the outcome
  is a git repository. That effector-pluggability is a follow-on decision, made
  necessary by this one.
- Operating the system still spans two languages across the trust seam (ADR 0002);
  one repository does not change that, and the seam remains a runtime process
  boundary regardless of co-location.
