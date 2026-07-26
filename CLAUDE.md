## Working on this repo

The threat model is the product. A false claim in `docs/THREAT_MODEL.md` is a
worse defect than a failing test, and it will not fail CI — so:

- Before claiming a residual is discharged, read `docs/THREAT_MODEL.md`'s
  "Two rules for changing this document" and follow both. If the residual was
  already deleted, recover its text from git history rather than paraphrasing
  it from memory.
- Verification means the **live outcome**, not just fixtures. A gate can pass
  every test in `envelope/tests/` and still strand the maintained app — run
  the demo outcome's own build/test/e2e (`autopilot-demo-v2-final/workspace`)
  before calling a gate change done.
- `cargo test --all-targets` from `envelope/` is the bar; it makes real
  `git`/`npm`/Playwright calls, so ~40s for the integration suite is normal.

**Reviewing your own plan does not count as verification.** This is the same
self-grading hole `tests/contract/` exists to close, applied to the work of
building the thing. If you wrote the plan, you will check the code against the
plan and never notice the plan was wrong — that is exactly how M3 shipped
claiming to discharge R7. Get a reviewer with **fresh context**, given the repo
and the list of claims but *not* the plan's reasoning.

## graphify

This project has a knowledge graph at graphify-out/ with god nodes, community structure, and cross-file relationships.

Rules:
- For codebase questions, first run `graphify query "<question>"` when graphify-out/graph.json exists. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts. These return a scoped subgraph, usually much smaller than GRAPH_REPORT.md or raw grep output.
- If graphify-out/wiki/index.md exists, use it for broad navigation instead of raw source browsing.
- Read graphify-out/GRAPH_REPORT.md only for broad architecture review or when query/path/explain do not surface enough context.
- After modifying code, run `graphify update .` to keep the graph current (AST-only, no API cost).
