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
cargo test    # the invariants, the design-conformance stage, and the git-backed lifecycle
```

No dependencies. The entire trusted computing base is the code under `src/`.

### Operating for real

The trusted kernel adjudicates real writes to a real repository. The subcommands
are the whole interface — there is no other way to reach it:

```sh
# decide one proposed write: reach-check it, apply it, verify with the repo's own
# build, then commit it or revert it — file body on stdin, verdict (JSON) on stdout
echo "<file contents>" | cargo run -- adjudicate \
  --repo <path-to-outcome> --path src/components/Badge.tsx --intent "add Badge"
```

Reversibility is real git: the tree must be clean, the change is committed on a
green build or restored on a red one. Verification is the repository's own `npm
run build` — run *inside* the envelope, so the proposer still cannot supply what
it is judged by. This is the channel the [advisor](../advisor) (the untrusted
Claude-driven brain) drives; it is the only way the brain can change a file.

The changeset lifecycle (`begin` → `stage`* → `commit`, ADR 0005), workspace
establishment (`establish`), dependency maintenance (`refresh-deps`), abandoning
an open changeset (`reset`), and the runtime error-rate watchdog (`monitor`) are
the rest of the surface — run `cargo run` with no arguments for the full list.

## How it works

Every proposed write is decided by one pure function — `Policy::evaluate` — before
it ever touches disk:

1. **Policy check** (deterministic). The action-only reach invariant. If violated,
   the action is denied and never touches the working tree.
2. **Apply and verify.** An allowed write is applied to the working tree and
   verified by the repository's own build (`worktree::BuildVerifier`) — never the
   agent's self-report, which it cannot even express. Unverified ⇒ reverted. Fails
   closed.
3. **Commit or revert.** On green the changeset is committed with git itself as
   the reversibility substrate; on red the tree is reset back to `HEAD`.
4. **Runtime watchdog.** After a deploy, `envelope monitor` reads trusted
   telemetry (never from the agent) and reverts automatically on an error-rate
   breach — no human, no agent cooperation. Fails closed.

### The invariants

| Invariant | Rule | Module |
|---|---|---|
| `reach` | Deny-by-default write allowlist; forbidden zones (backend client, secrets, deploy config, the core); traversal-safe | `src/invariants/reach.rs` |
| design conformance | Staged UI files must compose the design system, not fork it | `src/design.rs` |

A further guarantee holds at **compile time**, not by runtime check: the agent
cannot supply or fake its own verification or health metrics — both are
unrepresentable in the `Action` type, which can express nothing but a write.

## Project layout

```
src/
  policy.rs         the action-only rule set (reach)
  invariants/       the individual rules (reach)
  worktree.rs       the REAL git-backed effector + build verifier (adjudicate path)
  design.rs         design-conformance stage, invoked from worktree.rs
  runtime.rs        the runtime error-rate watchdog (envelope monitor)
  types.rs          the closed Action / Verdict model
  main.rs           the CLI: wires the subcommands to the trusted core
```

The real untrusted agent lives in a separate project — [advisor](../advisor),
a Claude-driven loop — and reaches the world only through `envelope adjudicate`
and the changeset lifecycle commands.

## Documentation

- [ADR 0001 — Project name](../docs/adr/0001-project-name.md)
- [ADR 0002 — Language for the trusted core](../docs/adr/0002-language-for-the-trusted-core.md)
- [ADR 0003 — One system repository; the outcome is external](../docs/adr/0003-one-system-repository-outcome-external.md)
- [Threat model](../docs/THREAT_MODEL.md) — assets, trust boundary, threats T1–T9, and honest residuals

## Status

Runnable, not yet production-hardened: it runs, it is tested, and its boundary is
real. The known gaps are tracked honestly as residuals in the
[threat model](../docs/THREAT_MODEL.md).

The structural keystone — putting the agent across a process seam, with the real
gate on the trusted side — is now built: the [advisor](../advisor) is a real
Claude-driven agent that can only change the app through `envelope adjudicate`,
and verification is the app's real build. The remaining load-bearing residual for
a frontend is agentic **UI** verification (does the rendered page actually work),
on top of the typecheck/build gate that exists today.
