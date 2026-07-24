# Autopilot

**Autopilot for your codebase** — it acts only within limits it can't change,
every change verified and reversible, and you can take the controls anytime.

Autopilot autonomously maintains a software **outcome**: an untrusted frontier
model proposes changes; a small trusted core decides their fate. The name is the
thesis, read the way you'd read it on a plane — automation that flies routine work
for you, inside hard limits it cannot exceed, with a human who can take the
controls at any moment. Nothing the system does depends on the model behaving
well. The mnemonic: **the advisor proposes; the envelope disposes.**

It has two parts, and the outcome is deliberately **not** one of them:

| Part | Trust | What it is |
|---|---|---|
| [`envelope/`](envelope) | **trusted** | The reference monitor (Rust, zero-dependency) — flight-envelope protection. Decides every change: a deny-by-default reach allowlist, git-backed reversibility, and verification by the outcome's own build. Commits on green, reverts on red. |
| [`advisor/`](advisor) | **untrusted** | The brain (TypeScript) — the flight director. A Claude-driven loop that reads the outcome and proposes writes. It has no authority to act; its only channel to the world is asking the envelope. |
| the **outcome** | — | The artifact being maintained (here, a small admin web app). **External and swappable** — supplied to the system by configuration, never vendored here. |

```
   advisor    ──propose write──▶  envelope  ──commit / revert──▶  outcome
  (untrusted)  ◀────verdict─────  (trusted)                      (external)
```

The seam between brain and core is a **runtime process boundary** (`envelope
adjudicate`): the brain cannot write files itself, widen the allowlist, or fake the
build that judges its work. That is why this is one repository rather than three —
repository layout does not affect the trust boundary, so the structure is chosen
for the one thing it *does* affect: an auditor clones **one** thing and reads it
top to bottom. See [ADR 0003](docs/adr/0003-one-system-repository-outcome-external.md).

## Why the outcome is external

The outcome here is a codebase, so it is version-controlled with git — which is why
the envelope can use git as its reversibility substrate. But that is a property of
*this* outcome, not of the system. An event-log outcome would be append-and-truncate;
a database, transactions. You would never `git clone` an event log to understand the
system that writes it. The outcome is something the system *points at and produces*,
not a part you check out to understand it.

## Run it

```sh
# build the trusted core
cd envelope && cargo build && cargo test

# install the brain; `npm link` puts the `autopilot` command on your PATH
cd ../advisor && npm install && npm link

# a project is a directory — operate from inside it, like git
mkdir ~/my-tool && cd ~/my-tool
autopilot init                                     # make this directory a project
autopilot run --dry-run                            # prove the seam (mutates nothing)
autopilot observe "…what is noticed or wanted…"    # file into the ledger
autopilot run                                      # act on the ledger
```

A project directory holds `project.json` (the workspace to build, read-only
sources, the clearance in force), the observation ledger, and the audit journal.
See [advisor/README](advisor/README.md).

## Documentation

- [advisor/README](advisor/README.md) — the brain, the loop, what lands and what can't
- [envelope/README](envelope/README.md) — the trusted core, the invariants, how the boundary holds
- [docs/adr/](docs/adr) — architecture decisions (core name, language for the core, one-system-repo, naming the system)
- [docs/THREAT_MODEL.md](docs/THREAT_MODEL.md) — assets, trust boundary, threats, and honest residuals
- [docs/ROADMAP.md](docs/ROADMAP.md) — the ordered backlog: the residuals and ADR follow-ons, sequenced
- [docs/runs/](docs/runs) — write-ups of real advisor runs against real outcomes

## Status

Runnable, not yet production-hardened — and it has run. On 2026-07-23 the advisor
took a small admin console from nothing to a working app: it read two seeded
observations, chose its own strategy (greenfield React+Vite+TS over local
fixtures), and landed **5 green, atomic genesis changesets** plus a self-authored
ADR, for $0.96 over 53 turns, none of it requiring the agent to be trustworthy —
every commit passed through the envelope's own build gate. See
[docs/runs/0001-first-light.md](docs/runs/0001-first-light.md).

The outcome it produces is a real, separate repository —
**[autopilot-demo-admin-console](https://github.com/shariffy/autopilot-demo-admin-console)**
— and its `git log` is the whole demonstration, told as one continuous history
rather than scattered across repos: the untrusted **advisor** as author, the
trusted **envelope** as committer, on every change the system landed, in order:

1. **Genesis** — the app built from nothing.
2. **Autonomous maintenance** — a sortable-columns feature landed from a real
   analytics signal, unprompted by a human ask.
3. **A self-healing incident** — a change built green but crashed in production;
   a model-free **runtime trip** (`git revert` to last-known-good) caught it, then
   the advisor landed a durable fix under a frozen reproducer it cannot author
   (ADR 0011).
4. **Design governance** — the envelope began rejecting ad-hoc UI, so the next
   feature composed only from a sanctioned design system (ADR 0012).

The trust boundary is real and the build is a genuine verification gate; the real
(git-backed) adjudication path is under automated test
(`envelope/tests/worktree_lifecycle.rs`). The load-bearing remaining residual is
general agentic **UI** verification — a seeded reproducer and the runtime trip
narrow it (ADR 0011), but confirming *arbitrary* rendered behaviour, not just that
it compiles and passes a seeded test, is still open — tracked honestly in the
[threat model](docs/THREAT_MODEL.md).
