# ADR 0013 — Deterministic e2e verification

- **Status:** Accepted
- **Date:** 2026-07-26
- **Note on the name.** The roadmap called this milestone "agentic UI
  verification," after `verifier.rs`'s original stub — *"a browser-driving
  agent that checks the rendered UI."* What this ADR decides is **not** that.
  There is no model anywhere in this gate: it runs the outcome's own
  `test:e2e` script and reads an exit code. The only agent involvement is that
  the advisor authors the specs during Genesis, which is the *weak* part of
  this design, not the feature — it is why R7 records that presence is a
  floor, not a quality bar. The agentic judge the old name promised is
  deferred as R13, unbuilt. Named for what it is, so the roadmap's ✅ does not
  claim the half that was skipped.
- **Scope:** how the envelope closes the **crash class** of R7
  (`docs/THREAT_MODEL.md`) — a change that builds green and passes unit tests
  but throws or fails to render at runtime, with no seeded reproducer to catch
  it. It does **not** close R7's larger half (a page that renders the wrong
  thing); R7 stays open, narrowed. Adds an e2e
  verifier stage (`worktree.rs::BuildVerifier`) and makes the existing test
  stage (ADR 0011) mandatory rather than opt-in. Does not change the
  changeset lifecycle, the clearances, `reach.rs`, or the trust thesis of
  ADR 0005.

## Context

ADR 0011 closed the *specific* half of R7: a frozen reproducer, seeded from
telemetry evidence for one incident already captured, judges whether a fix
actually resolves the runtime crash it targets. What it left open — named
openly in R7's own prose — is the *general* case: nothing drives the
rendered page for a change with **no** seeded reproducer and confirms it
actually works. Today's gate proves "the code compiles" and "the tests that
already exist still pass"; neither proves "the page renders."

### What kind of stage this is

`design.rs` (ADR 0012) is a *static* check: it reads staged bytes and
pattern-matches structure, never executing anything. The `test` stage (ADR
0011) is *dynamic*: it executes code and asserts on behaviour. What this ADR
adds is unambiguously the second kind — it executes the built artifact in a
real browser and asserts on runtime behaviour. It is an **e2e stage**, the
browser-level sibling of `test`, not a new structural-invariant category
alongside `design.rs`. It gets its own block in `BuildVerifier::run`, its own
gate, its own position, its own line in `describe()`, its own threat-model
entry — the same way `design.rs` and `test` already live as two distinct
stages in the same function rather than one blended check. A unit-test
failure and a browser crash are different failure classes worth
distinguishing in the build log and in whatever detail the advisor sees;
e2e is materially more expensive (a real server, a real browser) and worth
sequencing after the cheaper gates have already had a chance to fail fast;
and folding it into `test`'s own code path would be the inconsistent choice
given nothing else here is merged that way.

### Where the content lives, and why this needed no new toolchain or reach rule

The *fuller*, functionally useful version of this check — "the table is
actually sorted," "the detail page shows the right record" — is inherently
**outcome-specific**: it has to know that outcome's pages and behaviour. That
knowledge must not accumulate inside the envelope's own repo — ADR 0003 keeps
the outcome external and swappable precisely so the system stays ignorant of
outcome specifics. So there is no envelope-owned Node/Playwright toolchain
and no envelope-authored crawl script. Playwright (or an equivalent) becomes
an ordinary **outcome devDependency**, installed by the `npm ci`
`BuildVerifier` already runs, audited by the existing non-regression gate
like anything else in `package.json` — no special-casing. The `.spec` files
live under the outcome's own `tests/e2e/`, run via a `"test:e2e"` script the
outcome's `package.json` declares. `playwright.config.ts`'s own `webServer`
option is the outcome's standard way to boot a preview server before tests
run and tear it down after — ordinary Playwright usage, not something the
envelope orchestrates.

This also means **no `reach.rs` change is needed**. `ALLOWED_WRITE_PREFIXES`
under Maintenance already has no `tests/` prefix at all —
`ordinary_tests_outside_contract_follow_the_normal_rules` already proves
`tests/unit/foo.test.ts` is unwritable under Maintenance today, purely by
omission, deny-by-default. `tests/e2e/` gets the identical treatment for
free: Genesis (broad reach, bringing the app into existence) writes it,
Maintenance's narrow allowlist doesn't mention `tests/`, so it is frozen
post-launch with zero new code — the same footing as `tests/unit/`, not the
stronger "frozen under every clearance including Genesis" rule
`tests/contract/` needs. That stronger rule exists specifically because the
frozen oracle judges a fix for an *incident*, and the agent must never grade
its own fix — a different situation from ordinary e2e coverage written as
part of building the thing in the first place, the same footing as the
design system.

### Scope decision: mandatory, not opt-in — and this reaches back to `test` too

ADR 0003 was checked directly before deciding this: it commits the system to
keeping only the outcome's *reversibility/storage substrate* type-pluggable
("git is the right implementation for *a codebase outcome*... an
append/truncate effector for a log, a transaction for a database" — that is
M5, still open, and scoped narrowly to the effector). It never claims
`reach.rs`/`BuildVerifier`/`design.rs` must be generic across outcome types.
Given that, and given the concrete implementation is already thoroughly
npm/TS/React/Vite-specific (`package.json` parsing, `npm audit`,
`src/pages/`, the design system) with zero cost paid today for that
specificity being "wrong," there is no architectural reason to keep
`test`/`test:e2e` soft-opt-in the way an outcome-agnostic system would have
to. Both scripts existing and passing is ordinary expected practice for a
web project — the same category of requirement as "the build must succeed,"
which is already unconditional.

So: **`has_test_script` and `has_e2e_script` become fail-conditions, not
skip-conditions**, for every changeset — including the first genesis
changeset. No exemption for Genesis, matching the precedent `design.rs`
already set (it lints Genesis's own first `.tsx` files with no clearance
exception). This is a **policy change to the existing `test` stage** (ADR
0011), not just a property of the new one — mandating `test:e2e` while
leaving `test` optional would be an inconsistent split of the same decision,
not two decisions.

## Decision

### The new stage

`BuildVerifier::run` gains a step ("3.6. E2E") immediately after the
existing test stage ("3.5") and before the audit stage ("4"):

- **`has_e2e_script(repo)`** mirrors `has_test_script`'s existing
  brace-scanning shape exactly: checks `package.json`'s `scripts` object
  (reusing `json_object`) for a `"test:e2e"` key.
- If present, it runs `pm run test:e2e` (`pm test:e2e` for yarn, mirroring
  `build_args`'s pm-specific invocation convention — `test:e2e` is not a
  reserved alias any package manager exempts from `run`, unlike `test`),
  capturing combined output and failing the changeset on nonzero exit.
- `describe()` gains an unconditional mention of the e2e stage: it is only
  ever reached from `commit`, after `run` has already returned green with a
  changeset open, by which point the e2e stage has always run and always
  passed — there is no absent case left to guard against there.

### The corrected mechanism: where the mandatory bar actually bites

An earlier draft of this design would have enforced the mandatory scripts
inside `establish_clone`'s own precondition check
(`verifier.run(workspace, &[])`) — which runs *before any changeset exists*,
judging a predecessor's raw state, not something the agent proposed. Doing
that would refuse adopting a legacy, test-less predecessor outright,
foreclosing the exact case where an advisor clones something specifically to
bring it under coverage.

The corrected mechanism: `establish_clone`'s precondition stays
**unchanged** — build-green only, nothing about test scripts. The mandatory
requirement is enforced only when a changeset is actually being adjudicated.
`run()` already has a free, reliable signal to distinguish the two call
sites: `changeset_is_open(repo)` is false during `establish_clone`'s call (no
changeset exists yet) and true by the time `commit` calls it (opened via
`begin`/`stage`). Both `has_test_script` and `has_e2e_script`'s
absence-handling is gated on that signal, not on a new parameter threaded
through every call site:

```rust
if Self::has_test_script(repo) {
    // run it, fail on nonzero exit — unchanged
} else if changeset_is_open(repo) {
    // absence now fails the changeset — new
}
// else: no changeset open (establish_clone's precondition call) —
// absence is tolerated so a legacy, test-less predecessor can still
// be adopted.
```

Net effect: adopting a green-but-test-less predecessor succeeds; the
advisor's *first* real changeset against it is then refused until
`test`/`test:e2e` exist and pass — so a Clone-based genesis can still add e2e
coverage as its first act, exactly like an Empty-based one can. This is
proven directly:
`establish_clone_adopts_a_test_less_predecessor_but_refuses_the_first_changeset_against_it`
adopts a predecessor whose `package.json` declares only `build`, confirms
`establish` succeeds, then confirms the very next `stage`+`commit` against
that adopted workspace is refused, naming the missing `test` script.

### Two-tier crash detection: cheap first, expensive only for what the cheap tier can't see

Playwright/Puppeteer/raw-CDP all sit at the same speed floor — a real
Chromium boot plus real page load dominates regardless of wrapper library, so
swapping tools buys nothing. The actual lever is architectural: don't make
the browser stage catch everything.

A large share of "does this crash on render" is catchable by a
jsdom/happy-dom-based smoke assertion inside the existing (fast, no-browser)
`test` stage — a fixture with `jsdom` and `@testing-library/react` as
devDependencies can assert "mount `<App />`, assert it doesn't throw" in
milliseconds, catching the exact class of bug (a component throwing during
render) a lot of the e2e crawl's value is aimed at. This is guidance for what
a genesis run should include as part of satisfying the mandatory `test`
script, not an envelope-enforced rule (the same "presence and passing, not
content" boundary as everywhere else here) — but it changes the cost story:
most crash bugs get caught near-instantly, before the expensive stage ever
runs, and the `test`-before-`test:e2e` ordering already in place means this
happens for free.

This does not replace `test:e2e` — jsdom never runs against the actual built
`dist/` bundle, so it is blind to real bundling/asset-path/base-URL bugs,
real router navigation, real CSS layout, anything that only manifests in the
actually-served, actually-built artifact. The real-browser stage is what
catches that class specifically; the two tiers are complementary, not
redundant, ordered cheap-first.

Concrete speedups for the `test:e2e` stage itself, if using Playwright, none
requiring a tool swap: install only Chromium (`npx playwright install
chromium`, not the default of all three engines), reuse one browser launch
and one context across every `page.goto()` in a crawl rather than a fresh
process per page, and — considered and rejected — reusing a persistent
preview server across changesets to skip boot cost entirely. Each changeset
needs a server built from *its own* freshly staged tree, not stale state left
over from a previous verification: correctness over speed here.

### What's mandated vs. what's left to genesis's judgment

The envelope enforces *presence and passing*, not *content or depth* — it has
no opinion on which routes get covered or how thoroughly, only that
`test`/`test:e2e` exist and exit 0, same as it has no opinion on what the
build actually builds. A minimal genesis-time `tests/e2e/` smoke spec (load
each top-level route, assert no thrown error and non-empty render — exactly
the shape `FIXTURE_E2E_SMOKE_SPEC` in `tests/worktree_lifecycle.rs` takes) is
a reasonable default a genesis run can satisfy the mandatory-presence
requirement with, the same way genesis already produces its own design
system and component library unprompted, without the envelope dictating
their shape. Depth beyond that (real functional assertions) stays a quality
question, not a structural one — the human launch gate (ADR 0005) is what
actually judges whether genesis's test coverage is meaningful or vacuous,
same as it already judges everything else genesis produces.

### Deferred, not forgotten: the agentic judge and code review

A deterministic e2e suite — however outcome-specific — can only ever catch
what someone thought to assert. It cannot catch a non-throwing wrong-answer
fault, nor judge code quality/security the way a human reviewer would.
`verifier.rs`'s own original stub comment already named the more ambitious
version — *"a browser-driving agent that checks the rendered UI"* — and a
parallel gap exists for code review: nothing today judges whether
agent-written code is well-formed or free of a vulnerability the agent
itself introduced. Both are the same underlying pattern (an agent grading
agent-authored work) and both are **deliberately out of scope here**: an
agentic judge would be this system's first *behavioural* (not structural)
trust component, newly exposed to indirect prompt injection (the advisor
writes the content — page DOM, code comments — the judge would read), and
deserves one deliberately-designed shared mitigation story rather than being
rushed in here. Tracked as residual **R13** below and a note in
`ROADMAP.md`'s "Not on this roadmap" section, not a numbered milestone yet.

## Decision (scripts-freeze — T16)

### The hole

`package.json` joined Maintenance's `ALLOWED_WRITE_FILES` in ADR 0009 as
*dependency intent*. `reach` sees a path and a byte count, never content — so
nothing stopped a Maintenance changeset replacing
`"test:e2e": "playwright test"` with `"test:e2e": "node -e 'process.exit(0)'"` in
the same changeset that introduced the bug. The frozen `tests/e2e/` suite never
runs; the gate passes. An added `pretest`/`posttest` hook exploits the same gap:
npm runs those automatically, so a hook injected alongside a bug can silence the
gate without touching the gate-key scripts at all.

### Fix: freeze the whole `scripts` object under Maintenance

Under Maintenance, `BuildVerifier::run` compares the `scripts` object
(`json_object(&manifest, "scripts")`) byte-for-byte against the one committed at
`HEAD` (read via the existing `git show HEAD:package.json` helper, reusing the
pattern `lockfile_changed_since_head` already establishes). The whole object,
not just the three gate keys — so any key, including auto-run npm lifecycle hooks,
is covered. Genesis writes scripts freely; if HEAD has no `package.json` yet there
is no baseline to compare against.

### Why `--clearance` on `commit` is the wrong fix

Adding `--clearance` to the `commit` CLI is fewer lines and is wrong: the agent
could stage under Maintenance, then invoke `commit --clearance genesis`, bypassing
the check. The correct fix persists the clearance where the agent cannot reach it
— line 1 of the changeset marker (`.git/envelope-changeset`), already a
never-writable zone enforced by `invariants::reach`. `open_changeset` writes it
there; `record_staged` preserves it; `staged_paths` skips it (so the clearance
line never leaks into the `git add -- <paths>` list); `commit` reads it back and
passes it to `BuildVerifier::run`. Any code path that opens a changeset without
an explicit clearance (`refresh_dependencies`' auto-open, `begin`) defaults to
Maintenance — deny-by-default.

## Consequences

- `worktree.rs::BuildVerifier`: `has_e2e_script`, `e2e_args`, the new e2e
  stage block, and `has_test_script`'s absence-handling flipped from a skip
  to a `changeset_is_open`-gated fail. `describe()` names the e2e stage
  unconditionally.
- No `invariants::reach.rs` change: `tests/e2e/` inherits the same
  Genesis-writable/Maintenance-frozen footing as `tests/unit/` by omission,
  deny-by-default.
- `envelope/tests/worktree_lifecycle.rs`: the shared fixtures
  (`FIXTURE_PACKAGE_JSON` and its siblings) now declare passing `test` and
  `test:e2e` scripts — a mechanical migration cost of making both mandatory,
  since every existing test that lands a changeset now depends on both being
  present. New tests: `missing_test_script_fails_the_changeset_naming_the_missing_script`
  and its `test:e2e` parallel (mandatory presence, not just passing); a real
  Vite+React+Playwright fixture proving `e2e_stage_allows_a_changeset_whose_e2e_spec_passes`
  and `e2e_stage_fails_a_changeset_whose_component_throws_during_render` (a
  component that typechecks and builds green but throws unconditionally
  during render — caught only by the real browser, `HEAD` unmoved on
  failure); and `establish_clone_adopts_a_test_less_predecessor_but_refuses_the_first_changeset_against_it`
  proving the corrected mechanism end to end.
- `docs/THREAT_MODEL.md`: **T15** (new) names the general-case discharge;
  **T14**'s wording is amended (the test stage is no longer "skipped when the
  outcome has no test script" — that stopped being true); **R7** (narrowed, not discharged) and **R13**
  join the residuals list.

## Residuals

- **R7 stays open, narrowed — it is not discharged by this ADR.** The crash
  class is closed (T15). What R7 also named is not: *"most residual frontend
  risk is visual/UX regressions that compile and pass existing tests but still
  look or behave wrong"* — a gate whose only question is "did it throw" cannot
  see any of that. A non-throwing wrong-answer fault still needs a human to
  say what correct is; presence of `test`/`test:e2e` is a floor rather than a
  quality bar (nothing stops either passing vacuously); and the specs are
  themselves agent-authored at Genesis. The agentic UI verification R7
  explicitly named remains unbuilt.
- **R13**: no qualitative/security code review. A change that clears every
  deterministic gate (build, design, test, e2e, audit) can still be poorly
  written or contain an agent-introduced vulnerability no existing gate
  checks for. Named as a sibling gap to R7, sharing a future mitigation (the
  deferred agentic-judge pattern noted above) — not addressed here, and not a
  numbered milestone yet.
