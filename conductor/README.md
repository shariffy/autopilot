# conductor

The untrusted **brain** of the frontier demo: a Claude-driven loop that
autonomously maintains **roli-admin** (the external *outcome*), able to change it
only through the [`envelope`](../envelope) trust boundary.

> Don't trust the brain. Trust the envelope.

This is the half people usually mean by "an AI that maintains a web tool on its
own" — the model reads the codebase, decides what to change, and writes it. The
point of the demo is the *other* half: it does all of that as an **untrusted**
component. Nothing it can do depends on it behaving well, because the only way it
can touch a file is to ask the envelope, and the envelope decides — by
construction, not by good intentions.

## The shape of it

```
  conductor (this project)                 envelope (../envelope)
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
                                              roli-admin (external outcome)
```

The seam is a process boundary. The brain's *only* channel to the world is
`envelope adjudicate` — file body on stdin, a JSON verdict on stdout. It cannot
write files itself, cannot widen the allowlist, and cannot run or fake its own
verification: the build that decides "fit to ship" runs inside the envelope.

## What lands, and what can't

Each proposed write comes back as one of:

- **committed** — within the allowlist *and* the build passed. It is now a real
  git commit in `roli-admin`, attributed to the change, with the rationale as the
  message.
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
npm install

# prove the seam without calling Claude (mutates nothing):
npm start -- --dry-run

# file an observation into the ledger (does NOT run the agent):
npm run observe -- "support reports bulk user export is missing"

# the real loop — reads the observation ledger and acts on it:
npm start
```

Auth is the Claude Agent SDK's: your Claude Code login (`~/.claude`) — i.e. your
subscription — or `ANTHROPIC_API_KEY` if that is set instead. No key is passed in
code. Running only reads the ledger; filing an observation is a separate act
(`npm run observe`, or drop a record under `observations/`).

Prerequisite: build the envelope once (`cd ../envelope && cargo build`). The
workspace is established fresh by the envelope — nothing needs a pre-existing clean
tree.

Config is via env (all optional — see [`.env.example`](.env.example)):
`WORKSPACE`, `OBSERVATIONS`, `OBSERVE_SOURCES`, `CHARTER`, `ENVELOPE_BIN`,
`CONDUCTOR_AUDIT`, `MAX_TURNS`.

## The brain's tools

| Tool | Trusted? | What it does |
|---|---|---|
| `list_dir`, `read_file` | reads, repo-confined | explore the codebase before editing |
| `propose_write` | **gated** | submit a write to the envelope; returns the verdict |

Reads are unrestricted in intent but confined to the repo — reading is safe.
Writing is the only thing that crosses the trust boundary.

## Audit

Two durable surfaces record what happened and why:

- `roli-admin`'s **git history** — every landed change, with its rationale.
- `conductor-audit.jsonl` — one line per *proposal* (including rejected and
  rolled-back ones), with the full verdict. This is the "communicate what
  changed, and what was refused" trail.

## Layout

```
src/
  main.ts       entry point: config, preflight, --dry-run probe, run
  loop.ts       the manual agentic loop (Claude proposes, envelope disposes)
  tools.ts      the brain's tool surface (reads confined; propose_write gated)
  envelope.ts   the typed seam to the trusted core (the only path to a write)
```

## Status

A runnable frontier demo, not a production system. The build (`tsc` + `vite
build`) is a real verification gate; the honest next step — tracked as the
load-bearing residual in the [threat model](../docs/THREAT_MODEL.md)
— is agentic **UI** verification: actually driving the rendered page to confirm a
change works, not just that it compiles.
