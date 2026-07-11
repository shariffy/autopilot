# Charter

**A system that builds an outcome** — and trusts the autonomous agent that builds
it *by construction*, not by hoping it behaves.

Charter autonomously maintains a software **outcome**: an untrusted frontier model
proposes changes; a small trusted core decides their fate. The name is the thesis —
the agent is *chartered*, free to act and produce an outcome within fixed limits it
cannot change, with every action verified and reversible. Nothing the system does
depends on the model behaving well.

It has two parts, and the outcome is deliberately **not** one of them:

| Part | Trust | What it is |
|---|---|---|
| [`envelope/`](envelope) | **trusted** | The reference monitor (Rust, zero-dependency). Decides every change: a deny-by-default reach allowlist, git-backed reversibility, and verification by the outcome's own build. Commits on green, reverts on red. |
| [`conductor/`](conductor) | **untrusted** | The brain (TypeScript). A Claude-driven loop that reads the outcome and proposes writes. Its only channel to the world is asking the envelope. |
| the **outcome** | — | The artifact being maintained (here, the `roli-admin` app). **External and swappable** — supplied to the system by configuration, never vendored here. |

```
   conductor  ──propose write──▶  envelope  ──commit / revert──▶  outcome
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

# run the untrusted brain against an external outcome
cd ../conductor && npm install
npm start -- --dry-run        # prove the seam without calling Claude (mutates nothing)

export ANTHROPIC_API_KEY=sk-ant-...
npm start -- "Add a reusable Badge component under src/components/ui"
```

By default the brain looks for the outcome at `../roli-admin` (a sibling of this
repo); point it elsewhere with `ROLI_ADMIN_REPO`. See [conductor/README](conductor/README.md).

## Documentation

- [conductor/README](conductor/README.md) — the brain, the loop, what lands and what can't
- [envelope/README](envelope/README.md) — the trusted core, the invariants, how the boundary holds
- [docs/adr/](docs/adr) — architecture decisions (core name, language for the core, one-system-repo, naming the system)
- [docs/THREAT_MODEL.md](docs/THREAT_MODEL.md) — assets, trust boundary, threats, and honest residuals
- [docs/ROADMAP.md](docs/ROADMAP.md) — the ordered backlog: the residuals and ADR follow-ons, sequenced

## Status

A runnable frontier **demo**, not a production system. The trust boundary is real
and the build is a genuine verification gate. The load-bearing remaining residual
is agentic **UI** verification — confirming a rendered page actually works, not just
that it compiles — tracked honestly in the [threat model](docs/THREAT_MODEL.md).
