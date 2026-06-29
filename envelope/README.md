# envelope

A trusted reference monitor that bounds an autonomous AI agent **by
construction**.

The agent's job: develop and maintain a **frontend** against an external,
provided backend API. There is no database — the agent touches only the frontend
codebase and the frontend deploy, and the backend is out of its reach.

The premise: you cannot make an autonomous agent trustworthy by making it
*careful*. You make it safe by putting it inside a small, auditable boundary
where anything outside the boundary is structurally impossible and everything
inside it is reversible. Trust lives in the boundary — the *envelope* — not in
the agent's behaviour.

> Don't trust the brain. Trust the envelope.

*The trusted core of a larger system — see the [system overview](../README.md) for
how the core, the brain, and the outcome fit together.*

## Quick start

```sh
cargo run     # watch the boundary judge a batch of agent proposals
cargo test    # 15 tests: the invariants, the gates, reversibility, and the end-to-end batch
```

No dependencies. The entire trusted computing base is the code under `src/`.

### Operating for real

The batch above runs the boundary in-memory to show its *shape*. The same trusted
kernel also adjudicates real writes to a real repository:

```sh
# decide one proposed write: reach-check it, apply it, verify with the repo's own
# build, then commit it or revert it — file body on stdin, verdict (JSON) on stdout
echo "<file contents>" | cargo run -- adjudicate \
  --repo <path-to-outcome> --path src/components/Badge.tsx --intent "add Badge"
```

`reach` is decided by the *same* `Policy` the demo uses. Reversibility is real
git: the tree must be clean, the change is committed on a green build or restored
on a red one. Verification is the repository's own `npm run build` — run *inside*
the envelope, so the proposer still cannot supply what it is judged by. This is
the channel the [conductor](../conductor) (the untrusted Claude-driven brain)
drives; it is the only way the brain can change a file.

## What you'll see

An untrusted stub agent proposes eight changes — some good, some forbidden,
unverified, unhealthy, or self-escalating. Every one passes through the single
chokepoint and gets a deterministic verdict:

```
SUMMARY   committed=3  rejected=4  rolled_back=1
DENIALS   change_shape=1  immutable_policy=1  reach=2
REVERTED  task_completion
```

The point of the demo: **none of these outcomes depend on the agent being
well-behaved.** The boundary produces them regardless.

## How it works

Every proposed action flows through one function — `Harness::enact` — which is
the entire trust boundary:

1. **Policy check** (deterministic). The action-only invariants (`reach`,
   `immutable_policy`). If any is violated, the action is denied and never touches
   the world.
2. **Verification gate.** For a deploy, the verification result (typecheck, tests,
   UI) is read from a **trusted verifier** — never the agent's self-report, which
   it cannot even express. Unverified ⇒ rejected. Fails closed.
3. **Reversible effect.** An allowed action is turned into an effect that is
   applied *and* yields an undo handle — holding the undo is the precondition for
   committing anything.
4. **Outcome gate.** After a deploy, health is read from **trusted telemetry**
   (never from the agent) and judged against **trusted guardrails**. A breach
   reverts automatically — no human, no agent cooperation. Fails closed.

### The invariants

| Invariant | Rule | Module |
|---|---|---|
| `reach` | Deny-by-default write allowlist; forbidden zones (backend client, secrets, deploy config, the core); traversal-safe | `src/invariants/reach.rs` |
| `change_shape` | No deploy without typecheck + tests + UI verification | `src/invariants/change_shape.rs` |
| `immutable_policy` | The agent cannot modify the rules that bound it | `src/policy.rs` |

Two further guarantees hold at **compile time**, not by runtime check: the agent
cannot supply or fake its own verification or health metrics (both unrepresentable
in the `Action` type), and it cannot mutate the world outside `enact` (`&mut
World` is never exposed).

## Project layout

```
src/
  harness.rs        the trusted chokepoint: the only path from proposal to effect
  policy.rs         the action-only rule set (reach, immutable_policy)
  invariants/       the individual rules (reach, change_shape)
  guardrails.rs     trusted outcome-gate policy (SLOs)
  telemetry.rs      trusted source of production health (agent has no handle)
  verifier.rs       trusted source of verification (CI + agentic UI checks)
  reversible.rs     reversible effects + the encapsulated World (in-memory demo)
  worktree.rs       the REAL git-backed effector + build verifier (adjudicate path)
  decision_log.rs   append-only audit trail
  types.rs          the closed Action / Verdict / Outcome model
  agent.rs          a STUB untrusted agent (stands in for a real model)
  main.rs           wires it together; runs the demo, or `adjudicate` for real
```

The real untrusted agent lives in a separate project — [conductor](../conductor),
a Claude-driven loop — and reaches the world only through `envelope adjudicate`.

## Documentation

- [ADR 0001 — Project name](../docs/adr/0001-project-name.md)
- [ADR 0002 — Language for the trusted core](../docs/adr/0002-language-for-the-trusted-core.md)
- [ADR 0003 — One system repository; the outcome is external](../docs/adr/0003-one-system-repository-outcome-external.md)
- [Threat model](../docs/THREAT_MODEL.md) — assets, trust boundary, threats T1–T9, and honest residuals

## Status

This is a frontier **demo** of the structural-trust thesis: it runs, it is
tested, and its boundary is real. It is not a production system. The known gaps
are tracked honestly as residuals in the [threat model](../docs/THREAT_MODEL.md).

The structural keystone — putting the agent across a process seam, with the real
gate on the trusted side — is now built: the [conductor](../conductor) is a real
Claude-driven agent that can only change the app through `envelope adjudicate`,
and verification is the app's real build. The remaining load-bearing residual for
a frontend is agentic **UI** verification (does the rendered page actually work),
on top of the typecheck/build gate that exists today.
