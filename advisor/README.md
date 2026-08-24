# advisor

The untrusted **brain** of Autopilot: a Claude-driven loop that
autonomously maintains the external **outcome**, able to change it
only through the [`envelope`](../envelope) trust boundary.

> Don't trust the brain. Trust the envelope.

In the aviation frame Autopilot takes its name from, this is the **flight
director**: it reads the state of things and proposes a correction, but it has
no authority to actuate anything itself. This is the half people usually mean by
"an AI that maintains a web tool on its own" — the model reads the codebase,
decides what to change, and writes it. The point of Autopilot is the *other*
half: it does all of that as an **untrusted** component. Nothing it can do
depends on it behaving well, because the only way it can touch a file is to ask
the envelope, and the envelope decides — by construction, not by good
intentions. The advisor proposes; the envelope disposes.

## The shape of it

```
  advisor (this project)                    envelope (../envelope)
  ┌───────────────────────────┐            ┌──────────────────────────────┐
  │ Claude (Opus 4.8, untrusted)│  propose  │ trusted core (Rust, zero-dep) │
  │   reads the repo            │  write    │   reach allowlist (deny-by-   │
  │   proposes a file write ────┼──────────▶│     default, traversal-safe)  │
  │                             │           │   git-backed reversibility    │
  │   ◀──────────── verdict ────┼───────────│   verify = the app's own build │
  │   reacts, iterates          │           │   commit (green) / revert (red)│
  └───────────────────────────┘            └──────────────────────────────┘
                                                    │ commits
                                                    ▼
                                              the outcome (external repo)
```

The seam is a process boundary. The brain's *only* channel to the world is
`envelope adjudicate` — file body on stdin, a JSON verdict on stdout. It cannot
write files itself, cannot widen the allowlist, and cannot run or fake its own
verification: the build that decides "fit to ship" runs inside the envelope.

## What lands, and what can't

Each proposed write comes back as one of:

- **committed** — within the allowlist *and* the build passed. It is now a real
  git commit in the outcome repo, attributed to the change, with the rationale as
  the message.
- **rejected** — outside the write allowlist (e.g. the API client under
  `src/api/`, `secrets/`, `infra/`, the envelope itself). Nothing touched disk.
- **rolled_back** — allowed, but it broke the build. Reverted; the tree is clean
  again. The build output comes back so the brain can fix and re-propose.
- **refused** — a precondition failed (e.g. the tree wasn't clean). Fails closed.

So a misbehaving or mistaken model is bounded the same way a well-behaved one is:
bad paths never apply, broken changes never persist, and every landed change is
reviewable in git history.

## Run it

```sh
npm install && npm link   # once: puts `autopilot` on your PATH (tsx-run, no build step)

# a project is a directory — make one and operate from inside it, like git:
mkdir my-tool && cd my-tool
autopilot init

# file an observation into the ledger (does NOT run the agent):
autopilot observe "support reports bulk user export is missing"

# the real loop — reads the observation ledger and acts on it:
autopilot run

# prove the seam without calling Claude (mutates nothing).
# needs the workspace directory to exist, so run it after the first
# `autopilot run` has established one: reach clearance is a property of
# the repo now, so `stage` must resolve the repo before it can judge
# reach at all (ADR 0016).
autopilot run --dry-run
```

Auth is the Claude Agent SDK's: your Claude Code login (`~/.claude`) — i.e. your
subscription — or `ANTHROPIC_API_KEY` if that is set instead. No key is passed in
code. Running only reads the ledger; filing an observation is a separate act
(`autopilot observe`, or drop a record in the project's `observations/`).

Prerequisite: build the envelope once (`cd ../envelope && cargo build`). The
workspace is established fresh by the envelope — nothing needs a pre-existing clean
tree.

Everything project-scoped comes from the **project you are standing in** — the
nearest ancestor directory holding `project.json` (workspace and sources; the
reach clearance is stamped on the workspace by the envelope, not recorded here —
see [ADR 0016](../docs/adr/0016-clearance-is-a-property-of-the-repo.md)),
alongside the observation ledger and the audit journal
([ADR 0007](../docs/adr/0007-the-project-as-the-unit-of-oversight.md)). Env is system-level only
(see [`.env.example`](.env.example)): `ENVELOPE_BIN`, `MAX_TURNS`.

## The brain's tools

| Tool | Trusted? | What it does |
|---|---|---|
| `list_dir`, `read_file` | reads, repo-confined | explore the codebase before editing |
| `propose_write` | **gated** | submit a write to the envelope; returns the verdict |

Reads are unrestricted in intent but confined to the repo — reading is safe.
Writing is the only thing that crosses the trust boundary.

## Audit

Two durable surfaces record what happened and why:

- the outcome's **git history** — every landed change, with its rationale.
- the project's `advisor-audit.jsonl` — one line per *proposal* (including
  rejected and rolled-back ones), with the full verdict. This is the "communicate
  what changed, and what was refused" trail.

## Layout

```
bin/autopilot.js   the `autopilot` command (npm link once)
src/
  cli.ts           command dispatch: init | observe | run
  project.ts       the project home: init, and resolving the project you stand in
  run.ts           the run command: preflight, --dry-run probe, the loop
  loop.ts          the manual agentic loop (Claude proposes, envelope disposes)
  tools.ts         the brain's tool surface (reads confined; propose_write gated)
  observations.ts  the observation ledger (read, file, render as the brief)
  observe.ts       the observe command: file one observation into the ledger
  envelope.ts      the typed seam to the trusted core (the only path to a write)
```

## Status

Runnable, not yet production-hardened. The build (`tsc` + `vite build`) is a real
verification gate; the honest next step — tracked as the load-bearing residual in
the [threat model](../docs/THREAT_MODEL.md) — is agentic **UI** verification:
actually driving the rendered page to confirm a change works, not just that it
compiles.
