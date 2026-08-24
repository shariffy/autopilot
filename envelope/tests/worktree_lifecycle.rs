//! Real-path integration tests for the changeset lifecycle (ADR 0005): `establish`
//! → `begin` → `stage`* → `commit` / `reset`, driven exactly as the advisor drives
//! them — through the compiled `envelope` binary, against throwaway git repos in
//! temp dirs.
//!
//! Each test gets its own fresh workspace and drives it end to end with real
//! `git` and (where a build is involved) the real trusted-gate pipeline —
//! resolve, install, build, and audit (ADR 0009) — against real `npm`/`tsc`.
//! The npm-dependent tests are slower (seconds, not milliseconds) but exercise the
//! actual trusted gate, not a stand-in for it — the same tradeoff the verifier
//! itself makes.
//!
//! These lock in six real fixes made while landing M1 (2026-07-23): commit-the-
//! adjudicated-set, stage-auto-opens-changeset, untracked-residue tolerance,
//! reset-on-red, and begin-refuses-a-dirty-tracked-tree. Losing any of them would
//! either corrupt a commit's attributable contents or wedge a run permanently.
//! Extended for ADR 0009 (dependency maintenance): the lockfile is now a tracked,
//! envelope-computed changeset member, and the audit gate is a non-regression
//! check on top of the build.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

// ---- fixtures: a minimal TS project the real `npm`/`tsc` build can judge ----

// `test`/`test:e2e` are mandatory (ADR 0013) for every changeset this
// fixture lands, so the baseline itself must declare both, passing, or the
// mandatory-script gate would fail every single test in this file before it
// ever reaches whatever behaviour that test actually means to exercise. Both
// are a plain `node -e`, not a real test runner, for the same reason the
// pre-ADR-0013 `test` fixture below was: what the gate cares about is
// presence and exit code, not content.
const FIXTURE_PACKAGE_JSON: &str = r#"{
  "name": "fixture",
  "private": true,
  "version": "0.0.0",
  "scripts": { "build": "tsc", "test": "node -e \"process.exit(0)\"", "test:e2e": "node -e \"process.exit(0)\"" },
  "devDependencies": { "typescript": "^5.5.4" }
}
"#;

const FIXTURE_TSCONFIG: &str = r#"{
  "compilerOptions": {
    "target": "ES2020",
    "module": "commonjs",
    "outDir": "dist",
    "strict": true
  },
  "include": ["src"]
}
"#;

const FIXTURE_INDEX_TS_GREEN: &str =
    "export const greet = (name: string): string => `hello ${name}`;\n";

// A type error `tsc` cannot miss: assigning a string literal to a `number`.
const FIXTURE_INDEX_TS_RED: &str = "export const greet: number = \"not a number\";\n";

const FIXTURE_EXTRA_TS_GREEN: &str =
    "export const shout = (s: string): string => s.toUpperCase();\n";

// A `package.json` edit that changes nothing but the version — enough to force
// resolution to recompute the lockfile (ADR 0009 §1) without needing a new
// dependency or any network fetch beyond the one `land_green_fixture` already
// paid for.
const FIXTURE_PACKAGE_JSON_BUMPED: &str = r#"{
  "name": "fixture",
  "private": true,
  "version": "0.0.1",
  "scripts": { "build": "tsc", "test": "node -e \"process.exit(0)\"", "test:e2e": "node -e \"process.exit(0)\"" },
  "devDependencies": { "typescript": "^5.5.4" }
}
"#;

// A dependency with real, stable, well-known advisories (several high-severity
// ReDoS findings against `minimatch@3.0.0`) — used to exercise the audit
// non-regression gate against genuine `npm audit` output, not a stand-in.
const FIXTURE_PACKAGE_JSON_WITH_VULNERABLE_DEP: &str = r#"{
  "name": "fixture",
  "private": true,
  "version": "0.0.0",
  "scripts": { "build": "tsc", "test": "node -e \"process.exit(0)\"", "test:e2e": "node -e \"process.exit(0)\"" },
  "devDependencies": { "typescript": "^5.5.4" },
  "dependencies": { "minimatch": "3.0.0" }
}
"#;

// Same vulnerable dependency, same version — only an unrelated field differs —
// so resolution reruns but reproduces the identical advisory set: the "leaves
// pre-existing findings unchanged" case.
const FIXTURE_PACKAGE_JSON_WITH_VULNERABLE_DEP_DESCRIBED: &str = r#"{
  "name": "fixture",
  "description": "same vulnerable dependency, unrelated edit",
  "private": true,
  "version": "0.0.0",
  "scripts": { "build": "tsc", "test": "node -e \"process.exit(0)\"", "test:e2e": "node -e \"process.exit(0)\"" },
  "devDependencies": { "typescript": "^5.5.4" },
  "dependencies": { "minimatch": "3.0.0" }
}
"#;

// `test` and `test:e2e` scripts that both always succeed — plain `node -e`
// calls, not a real test runner, so the test-stage tests below stay fast and
// need no extra dependency beyond what the fixture already resolves. What
// matters to the gate is only that the outcome DECLARES both scripts and
// their exit codes, not what they actually do. `test:e2e` joined this
// fixture under ADR 0013 — without it, this changeset would now fail the new
// mandatory e2e-presence check before ever reaching whatever the `test`
// stage itself is meant to prove.
const FIXTURE_PACKAGE_JSON_WITH_PASSING_TEST: &str = r#"{
  "name": "fixture",
  "private": true,
  "version": "0.0.0",
  "scripts": { "build": "tsc", "test": "node -e \"process.exit(0)\"", "test:e2e": "node -e \"process.exit(0)\"" },
  "devDependencies": { "typescript": "^5.5.4" }
}
"#;

// `test` script exits 1 — the build-green/test-red case the test stage exists to catch.
const FIXTURE_PACKAGE_JSON_WITH_FAILING_TEST: &str = r#"{
  "name": "fixture",
  "private": true,
  "version": "0.0.0",
  "scripts": { "build": "tsc", "test": "node -e \"process.exit(1)\"" },
  "devDependencies": { "typescript": "^5.5.4" }
}
"#;

// ---- mandatory-script-presence fixtures (ADR 0013) ----
//
// `test`/`test:e2e` are mandatory, not opt-in, as of ADR 0013: absence now
// fails the changeset (once one is open) instead of being silently skipped.
// These fixtures each omit exactly one of the two, so a test can prove the
// absence itself is what's being judged, independent of whether either
// script would have passed.

// Has `test:e2e` but no `test` — proves the amended (now-mandatory) behaviour
// of the pre-existing test stage.
const FIXTURE_PACKAGE_JSON_NO_TEST_SCRIPT: &str = r#"{
  "name": "fixture",
  "private": true,
  "version": "0.0.0",
  "scripts": { "build": "tsc", "test:e2e": "node -e \"process.exit(0)\"" },
  "devDependencies": { "typescript": "^5.5.4" }
}
"#;

// Has `test` but no `test:e2e` — proves the same policy on the new stage.
const FIXTURE_PACKAGE_JSON_NO_E2E_SCRIPT: &str = r#"{
  "name": "fixture",
  "private": true,
  "version": "0.0.0",
  "scripts": { "build": "tsc", "test": "node -e \"process.exit(0)\"" },
  "devDependencies": { "typescript": "^5.5.4" }
}
"#;

// Neither `test` nor `test:e2e` — the exact pre-ADR-0013 fixture shape,
// kept as its own named constant (rather than reusing `FIXTURE_PACKAGE_JSON`,
// which now always carries both) to stand for a "legacy, green-but-test-less
// predecessor": a repo `establish_clone`'s UNCHANGED precondition (build-green
// only) must still be able to adopt, so the mandatory bar can then be proven
// to bite at the advisor's first real changeset against it instead
// (`changeset_is_open` — see the corrected mechanism note in ADR 0013).
const FIXTURE_PACKAGE_JSON_NO_TEST_OR_E2E: &str = r#"{
  "name": "fixture",
  "private": true,
  "version": "0.0.0",
  "scripts": { "build": "tsc" },
  "devDependencies": { "typescript": "^5.5.4" }
}
"#;

// ---- scripts-freeze fixtures (ADR 0013, T16) ----
//
// Under Maintenance the scripts object must be byte-identical to HEAD's — the
// whole object, not just the gate keys, so an added `pretest`/`posttest` hook
// (npm runs those automatically) can't sneak past either. The fixture below
// adds a `pretest` hook to the baseline scripts object, leaving all mandatory
// scripts present; used to prove the scripts-freeze gate fires before any npm
// invocation, and that Genesis is unrestricted.

// Like FIXTURE_PACKAGE_JSON but with a `pretest` hook injected into `scripts`.
// All mandatory scripts are still present (so the mandatory-presence gate does
// not fire); only the scripts object differs from the baseline — the exact shape
// of the "replace a gate script with a hook" bypass T16 closes.
const FIXTURE_PACKAGE_JSON_SCRIPTS_WITH_HOOK: &str = r#"{
  "name": "fixture",
  "private": true,
  "version": "0.0.0",
  "scripts": { "build": "tsc", "test": "node -e \"process.exit(0)\"", "test:e2e": "node -e \"process.exit(0)\"", "pretest": "echo injected" },
  "devDependencies": { "typescript": "^5.5.4" }
}
"#;

// ---- e2e-stage fixtures (ADR 0013): a real, minimal Vite+React+Playwright
// project ----
//
// Unlike the `test` stage's fixtures above, exercising `has_e2e_script`'s
// stage for real needs a real bundler (`vite build`) and a real headless
// browser driving the actually-built `dist/` — a `node -e` stand-in would
// prove nothing about the class of bug this stage exists to catch (a
// component that throws only once React actually renders it). Kept as its
// own separate genesis project rather than layered onto the plain `tsc`
// fixture above: it needs `index.html`/`vite.config.ts`/`playwright.config.ts`/
// `tests/e2e/`, none of which the bare-`tsc` fixture has any use for.
//
// `PORT_COUNTER` gives each fixture instance its own preview-server port, so
// two of these can run concurrently within the same `cargo test` invocation
// (which runs tests in parallel by default) without colliding on a bound
// port — the one piece of shared, host-global state a real webServer needs
// that a temp-dir workspace does not already isolate for free.
static PORT_COUNTER: AtomicU64 = AtomicU64::new(4300);

fn next_port() -> u16 {
    PORT_COUNTER.fetch_add(1, Ordering::SeqCst) as u16
}

const FIXTURE_E2E_TSCONFIG: &str = r#"{
  "compilerOptions": {
    "target": "ES2020",
    "useDefineForClassFields": true,
    "lib": ["ES2020", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "skipLibCheck": true,
    "moduleResolution": "bundler",
    "jsx": "react-jsx",
    "strict": true
  },
  "include": ["src"]
}
"#;

const FIXTURE_E2E_VITE_CONFIG: &str = r#"import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
});
"#;

const FIXTURE_E2E_INDEX_HTML: &str = r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <title>fixture</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
"#;

const FIXTURE_E2E_MAIN_TSX: &str = r#"import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
"#;

// The conformant component: renders without throwing.
const FIXTURE_E2E_APP_GREEN_TSX: &str = r#"export default function App() {
  return <div>hello fixture</div>;
}
"#;

// Typechecks fine and `vite build` succeeds — nothing here is a compile-time
// error, so neither `tsc`-shaped checking nor the bundler can see it coming —
// but throws unconditionally the moment React actually renders it. Exactly
// the "compiles but breaks at runtime" class of fault THREAT_MODEL.md's
// R7/T15 name, and the reason this stage drives a real browser instead of
// trusting the build alone.
const FIXTURE_E2E_APP_THROWS_TSX: &str = r#"export default function App() {
  throw new Error("render crash fixture");
}
"#;

// A minimal smoke spec: load the page, and fail if either the mount left
// `#root` empty (React never got to render anything) or the page raised an
// uncaught error (React's own path for an unhandled render throw with no
// error boundary in place). Deliberately not a framework-provided assertion
// helper beyond what `@playwright/test` ships — this is the shape ADR 0013
// suggests as a reasonable genesis default, not something the envelope
// mandates the content of.
const FIXTURE_E2E_SMOKE_SPEC: &str = r##"import { test, expect } from "@playwright/test";

test("home page renders without throwing", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (err) => errors.push(err.message));

  await page.goto("/");
  await expect(page.locator("#root")).not.toBeEmpty();
  expect(errors).toEqual([]);
});
"##;

fn fixture_e2e_package_json(port: u16) -> String {
    format!(
        r#"{{
  "name": "e2e-fixture",
  "private": true,
  "version": "0.0.0",
  "scripts": {{
    "build": "vite build",
    "preview": "vite preview --port {port} --strictPort",
    "test": "node -e \"process.exit(0)\"",
    "test:e2e": "playwright install chromium && playwright test"
  }},
  "devDependencies": {{
    "vite": "^5.4.0",
    "@vitejs/plugin-react": "^4.3.0",
    "react": "^18.3.0",
    "react-dom": "^18.3.0",
    "typescript": "^5.5.4",
    "@playwright/test": "^1.62.0"
  }}
}}
"#
    )
}

fn fixture_e2e_playwright_config(port: u16) -> String {
    format!(
        r#"import {{ defineConfig }} from "@playwright/test";

export default defineConfig({{
  testDir: "tests/e2e",
  webServer: {{
    command: "npm run preview",
    port: {port},
    reuseExistingServer: false,
    timeout: 30_000,
  }},
  use: {{
    baseURL: "http://localhost:{port}",
  }},
}});
"#
    )
}

/// Stage a full Vite+React+Playwright genesis project's files (not yet
/// committed) into `ws` — `package.json`, `tsconfig.json`, `vite.config.ts`,
/// `index.html`, `src/main.tsx`, the caller-chosen `src/App.tsx` (green or
/// throws-on-render), `playwright.config.ts`, and the e2e smoke spec — each
/// its own `stage` call under Genesis clearance (the default), mirroring how
/// `stage_green_fixture` builds up the plain `tsc` fixture above. Each call
/// gets its own preview-server port (`next_port`), so this can be called from
/// more than one test in the same run without two fixtures' preview servers
/// colliding.
fn stage_playwright_fixture(ws: &Workspace, app_tsx: &str) {
    let port = next_port();
    for (path, content) in [
        ("package.json", fixture_e2e_package_json(port)),
        ("tsconfig.json", FIXTURE_E2E_TSCONFIG.to_string()),
        ("vite.config.ts", FIXTURE_E2E_VITE_CONFIG.to_string()),
        ("index.html", FIXTURE_E2E_INDEX_HTML.to_string()),
        ("src/main.tsx", FIXTURE_E2E_MAIN_TSX.to_string()),
        ("src/App.tsx", app_tsx.to_string()),
        ("playwright.config.ts", fixture_e2e_playwright_config(port)),
        (
            "tests/e2e/smoke.spec.ts",
            FIXTURE_E2E_SMOKE_SPEC.to_string(),
        ),
    ] {
        let out = stage(ws, path, &content);
        assert!(
            out.contains("\"outcome\":\"staged\""),
            "staging {path}: {out}"
        );
    }
}

// ---- design-conformance fixtures (ADR 0012, Phase D) ----
//
// `design.rs` lints staged `.tsx` files by SUBSTRING, never by parsing JSX
// (the crate stays zero-dependency), so these fixtures need not be real JSX
// to exercise it — plain `.tsx` source containing (or not containing) the
// banned substrings is enough, which keeps this bare `tsc`-only fixture
// project (no `--jsx`, no React) sufficient for the design stage tests below.
// A stub `src/design-system/index.ts` gives the fixtures' `../design-system`
// import a real module to resolve, so the *build* stays green — the design
// stage runs strictly after a green build (`worktree.rs::BuildVerifier::run`).

const FIXTURE_DESIGN_SYSTEM_INDEX_TS: &str = "export const Button = 0;\n";

// Imports the design system and contains none of the banned substrings —
// the conformant case.
const FIXTURE_CONFORMANT_PAGE_TSX: &str =
    "import { Button } from '../design-system'\n\nexport const marker = 'conformant page fixture'\n";

// Imports the design system (so it does NOT trip the missing-import rule) but
// contains a raw `<button` and an inline `style={{` — the two other rules,
// both violated by the same file, deliberately, so the rejection's detail can
// be checked for both rule names at once.
const FIXTURE_VIOLATING_PAGE_TSX: &str = "import { Button } from '../design-system'\n\nexport const marker = '<button style={{}}>click</button>'\n";

// ---- process plumbing ----

fn envelope_bin() -> &'static str {
    env!("CARGO_BIN_EXE_envelope")
}

/// A committer identity supplied via env vars rather than global/local git config,
/// so tests are hermetic and commits succeed unattended in CI regardless of the
/// runner's own git configuration.
fn git_identity_envs() -> [(&'static str, &'static str); 4] {
    [
        ("GIT_AUTHOR_NAME", "Envelope Test"),
        ("GIT_AUTHOR_EMAIL", "envelope-test@example.invalid"),
        ("GIT_COMMITTER_NAME", "Envelope Test"),
        ("GIT_COMMITTER_EMAIL", "envelope-test@example.invalid"),
    ]
}

/// Run the compiled `envelope` binary with the given args, feeding `stdin_body` on
/// stdin (as `stage`/`adjudicate` expect), and return combined stdout+stderr.
fn envelope(args: &[&str], stdin_body: &[u8]) -> String {
    let mut cmd = Command::new(envelope_bin());
    cmd.args(args)
        .envs(git_identity_envs())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn envelope binary");
    child
        .stdin
        .take()
        .expect("envelope stdin")
        .write_all(stdin_body)
        .expect("write envelope stdin");
    let out = child.wait_with_output().expect("wait on envelope");
    let mut combined = String::from_utf8_lossy(&out.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&out.stderr));
    combined
}

/// Run `git` directly against a workspace, for assertions the envelope binary
/// itself has no reason to expose (log shape, tracked-file listing, status).
fn git(dir: &Path, args: &[&str]) -> (bool, String) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .envs(git_identity_envs())
        .output()
        .expect("run git");
    let mut combined = String::from_utf8_lossy(&out.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.success(), combined)
}

// ---- throwaway workspace ----

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A fresh, empty temp directory (the shape `establish` requires), removed on
/// drop so a slow/parallel test run doesn't leave junk behind.
struct Workspace {
    dir: PathBuf,
}

impl Workspace {
    fn new(label: &str) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "envelope-worktree-it-{label}-{}-{n}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create throwaway workspace");
        Workspace { dir }
    }

    fn path(&self) -> &Path {
        &self.dir
    }

    fn path_str(&self) -> &str {
        self.dir.to_str().expect("workspace path is valid utf-8")
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

// ---- lifecycle helpers, built from the public CLI surface ----

fn establish_empty(ws: &Workspace) -> String {
    envelope(
        &["establish", "--repo", ws.path_str(), "--mode", "empty"],
        &[],
    )
}

fn stage(ws: &Workspace, rel_path: &str, content: &str) -> String {
    stage_with_clearance(ws, rel_path, content, "genesis")
}

/// Stage under an explicit clearance ("genesis" or "maintenance") — for tests
/// that exercise the Maintenance-clearance reach rules directly (ADR 0009).
///
/// Clearance is now a repo-level stamp, not a `stage` flag (this milestone): set
/// the stamp first via `envelope clearance --set`, then stage plainly. Only
/// matters when no changeset is currently open — the marker freezes clearance at
/// open time, exactly as before.
fn stage_with_clearance(ws: &Workspace, rel_path: &str, content: &str, clearance: &str) -> String {
    let set_out = envelope(
        &["clearance", "--repo", ws.path_str(), "--set", clearance],
        &[],
    );
    assert!(
        set_out.contains("\"outcome\":\"clearance_set\""),
        "setting clearance to {clearance}: {set_out}"
    );
    envelope(
        &["stage", "--repo", ws.path_str(), "--path", rel_path],
        content.as_bytes(),
    )
}

fn commit(ws: &Workspace, intent: &str) -> String {
    envelope(
        &["commit", "--repo", ws.path_str(), "--intent", intent],
        &[],
    )
}

fn begin(ws: &Workspace) -> String {
    envelope(&["begin", "--repo", ws.path_str()], &[])
}

fn reset(ws: &Workspace) -> String {
    envelope(&["reset", "--repo", ws.path_str()], &[])
}

/// Stage the green fixture's three files (package.json, tsconfig.json,
/// src/index.ts) under Genesis clearance, without committing.
fn stage_green_fixture(ws: &Workspace) {
    let out = stage(ws, "package.json", FIXTURE_PACKAGE_JSON);
    assert!(
        out.contains("\"outcome\":\"staged\""),
        "staging package.json: {out}"
    );
    let out = stage(ws, "tsconfig.json", FIXTURE_TSCONFIG);
    assert!(
        out.contains("\"outcome\":\"staged\""),
        "staging tsconfig.json: {out}"
    );
    let out = stage(ws, "src/index.ts", FIXTURE_INDEX_TS_GREEN);
    assert!(
        out.contains("\"outcome\":\"staged\""),
        "staging src/index.ts: {out}"
    );
}

/// Establish, stage the green fixture, and land it as one committed changeset.
/// Used by tests whose interesting behaviour happens *after* a green baseline.
fn land_green_fixture(ws: &Workspace, intent: &str) -> String {
    let out = establish_empty(ws);
    assert!(
        out.contains("\"outcome\":\"established\""),
        "establish: {out}"
    );
    stage_green_fixture(ws);
    let out = commit(ws, intent);
    assert!(out.contains("\"outcome\":\"committed\""), "commit: {out}");
    out
}

/// Establish and land a genesis changeset with a caller-chosen `package.json`
/// (tsconfig and the green source file are otherwise fixed) — used by the audit
/// gate tests to control exactly what the baseline's dependency findings are.
fn land_fixture_with_package_json(ws: &Workspace, package_json: &str, intent: &str) {
    let out = establish_empty(ws);
    assert!(
        out.contains("\"outcome\":\"established\""),
        "establish: {out}"
    );
    let out = stage(ws, "package.json", package_json);
    assert!(
        out.contains("\"outcome\":\"staged\""),
        "staging package.json: {out}"
    );
    let out = stage(ws, "tsconfig.json", FIXTURE_TSCONFIG);
    assert!(
        out.contains("\"outcome\":\"staged\""),
        "staging tsconfig.json: {out}"
    );
    let out = stage(ws, "src/index.ts", FIXTURE_INDEX_TS_GREEN);
    assert!(
        out.contains("\"outcome\":\"staged\""),
        "staging src/index.ts: {out}"
    );
    let out = commit(ws, intent);
    assert!(out.contains("\"outcome\":\"committed\""), "commit: {out}");
}

// ---- 1. establish empty writes a committed baseline; tree is clean ----

#[test]
fn establish_empty_writes_committed_gitignore_and_leaves_a_clean_tree() {
    let ws = Workspace::new("establish");

    let out = establish_empty(&ws);
    assert!(
        out.contains("\"outcome\":\"established\""),
        "establish: {out}"
    );

    assert!(
        ws.path().join(".gitignore").is_file(),
        ".gitignore should be written to the new workspace"
    );

    let (ok, log) = git(ws.path(), &["log", "--oneline"]);
    assert!(ok, "git log: {log}");
    let commits: Vec<&str> = log.lines().collect();
    assert_eq!(
        commits.len(),
        1,
        "establish should create exactly one baseline commit, got: {log:?}"
    );
    assert!(
        log.contains("baseline"),
        "baseline commit should say so: {log}"
    );

    // Clean by both measures: no tracked-vs-HEAD drift, and nothing untracked
    // either (nothing has touched the tree besides the baseline commit itself).
    let (ok, status) = git(ws.path(), &["status", "--porcelain"]);
    assert!(ok, "git status: {status}");
    assert!(
        status.trim().is_empty(),
        "workspace should be clean right after establish, got: {status:?}"
    );
}

// ---- 2. commit == the adjudicated set, never `add -A` (F1) ----

#[test]
fn commit_tracks_only_the_staged_paths_not_untracked_residue() {
    let ws = Workspace::new("commit-scope");
    let out = establish_empty(&ws);
    assert!(out.contains("\"outcome\":\"established\""), "{out}");

    stage_green_fixture(&ws);

    // An unrelated file that lands in the tree out-of-band (never staged through
    // the envelope) — the adversarial case: a commit that swept in `git add -A`
    // would attribute it to this changeset's intent and verification.
    fs::write(ws.path().join("NOTES.md"), b"scratch notes, never staged")
        .expect("write unrelated untracked file");

    let out = commit(&ws, "add the fixture ts project");
    assert!(out.contains("\"outcome\":\"committed\""), "{out}");

    // The commit's own contents must be exactly the staged paths PLUS the
    // envelope-computed lockfile (ADR 0009): `package.json` was staged with no
    // lockfile yet on disk, so resolution ran and `package-lock.json` joined
    // this changeset's membership — an envelope-authored member, not agent
    // residue, so it belongs here and NOTES.md still must not.
    let (ok, files) = git(
        ws.path(),
        &["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"],
    );
    assert!(ok, "diff-tree: {files}");
    let mut committed_paths: Vec<&str> = files.lines().collect();
    committed_paths.sort_unstable();
    assert_eq!(
        committed_paths,
        vec![
            "package-lock.json",
            "package.json",
            "src/index.ts",
            "tsconfig.json"
        ],
        "the commit must contain exactly the staged set plus the envelope-computed lockfile, nothing swept in"
    );

    // The repo's full tracked set is the baseline plus the staged set (plus the
    // lockfile) — NOTES.md is not among them.
    let (ok, tracked) = git(ws.path(), &["ls-files"]);
    assert!(ok, "ls-files: {tracked}");
    let mut tracked_paths: Vec<&str> = tracked.lines().collect();
    tracked_paths.sort_unstable();
    assert_eq!(
        tracked_paths,
        vec![
            ".gitignore",
            "package-lock.json",
            "package.json",
            "src/index.ts",
            "tsconfig.json"
        ],
        "NOTES.md must never become tracked"
    );

    // And it is still sitting there, untracked, exactly as it was written.
    let (ok, status) = git(ws.path(), &["status", "--porcelain"]);
    assert!(ok, "status: {status}");
    assert!(
        status.contains("?? NOTES.md"),
        "NOTES.md should remain untracked after the commit: {status:?}"
    );
}

// ---- 3. stage with no prior `begin` opens a changeset implicitly ----

#[test]
fn stage_with_no_prior_begin_opens_a_changeset_on_a_clean_tree() {
    let ws = Workspace::new("auto-open");
    let out = establish_empty(&ws);
    assert!(out.contains("\"outcome\":\"established\""), "{out}");

    // No `begin` call here — staging directly onto the clean post-establish tree
    // must succeed, asserting the clean baseline itself rather than refusing for
    // want of an explicit `begin`.
    let out = stage(&ws, "package.json", FIXTURE_PACKAGE_JSON);
    assert!(
        out.contains("\"outcome\":\"staged\""),
        "stage should succeed and implicitly open a changeset: {out}"
    );

    // The changeset marker is real, not just a happy return code.
    assert!(
        ws.path().join(".git").join("envelope-changeset").is_file(),
        "a changeset should now be open"
    );

    // Clean up the open changeset so the workspace doesn't leak; not the subject
    // of this test, but keeps the fixture tidy.
    reset(&ws);
}

// ---- 4. untracked residue (the real npm-ci byproduct) never wedges the next changeset ----

#[test]
fn untracked_residue_after_a_commit_does_not_block_the_next_changeset() {
    let ws = Workspace::new("residue");
    land_green_fixture(&ws, "add the fixture ts project");

    // Resolution ran inside that commit's build gate (ADR 0009): the lockfile
    // it computed is now a genuine, TRACKED member of the commit, not residue.
    assert!(
        ws.path().join("package-lock.json").is_file(),
        "resolution should have produced a lockfile"
    );
    let (ok, tracked) = git(ws.path(), &["ls-files"]);
    assert!(ok, "ls-files: {tracked}");
    assert!(
        tracked.lines().any(|f| f == "package-lock.json"),
        "the envelope-computed lockfile must be tracked, not residue: {tracked:?}"
    );

    // `npm ci`, also run inside that same gate, left a real, gitignored,
    // untracked `node_modules/` in the tree — now the residue the liveness fix
    // targets, in place of the (now-tracked) lockfile.
    assert!(
        ws.path().join("node_modules").is_dir(),
        "npm ci should have produced node_modules as a byproduct of install"
    );
    assert!(
        !tracked.lines().any(|f| f.starts_with("node_modules/")),
        "node_modules must stay untracked (it is gitignored): {tracked:?}"
    );

    // A second changeset, staged and committed on top of that residue, must
    // still proceed — this is the behaviour that used to wedge permanently.
    let out = stage(&ws, "src/extra.ts", FIXTURE_EXTRA_TS_GREEN);
    assert!(
        out.contains("\"outcome\":\"staged\""),
        "stage after residue should succeed, not refuse for an unclean tree: {out}"
    );
    let out = commit(&ws, "add a second, unrelated export");
    assert!(
        out.contains("\"outcome\":\"committed\""),
        "commit after residue should succeed, not refuse for an unclean tree: {out}"
    );

    let (ok, files) = git(
        ws.path(),
        &["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"],
    );
    assert!(ok, "diff-tree: {files}");
    assert_eq!(files.trim(), "src/extra.ts");
}

// ---- 5. reset-on-red: a build failure lands nothing, and reset clears the stage ----

#[test]
fn build_failure_lands_nothing_and_reset_clears_the_staged_files() {
    let ws = Workspace::new("build-failed");
    let out = establish_empty(&ws);
    assert!(out.contains("\"outcome\":\"established\""), "{out}");

    let (_, head_before) = git(ws.path(), &["rev-parse", "HEAD"]);

    let out = stage(&ws, "package.json", FIXTURE_PACKAGE_JSON);
    assert!(out.contains("\"outcome\":\"staged\""), "{out}");
    let out = stage(&ws, "tsconfig.json", FIXTURE_TSCONFIG);
    assert!(out.contains("\"outcome\":\"staged\""), "{out}");
    // The red file: a genuine type error `tsc` will catch.
    let out = stage(&ws, "src/index.ts", FIXTURE_INDEX_TS_RED);
    assert!(out.contains("\"outcome\":\"staged\""), "{out}");

    let out = commit(&ws, "a change that should never land");
    assert!(
        out.contains("\"outcome\":\"build_failed\""),
        "a real type error must fail the build: {out}"
    );

    // Nothing committed: HEAD is exactly where `establish` left it.
    let (_, head_after) = git(ws.path(), &["rev-parse", "HEAD"]);
    assert_eq!(
        head_before, head_after,
        "HEAD must not move on a failed build"
    );

    // The staged (uncommitted) files are still on disk right after the failure —
    // `commit` leaves them so the agent can fix and retry cheaply.
    assert!(ws.path().join("src/index.ts").is_file());

    // `reset` is the explicit abandon step: it must clear the staged files back
    // to the clean baseline.
    let out = reset(&ws);
    assert!(out.contains("\"outcome\":\"reset\""), "{out}");
    assert!(
        !ws.path().join("src/index.ts").exists(),
        "reset should remove the staged (never-committed) source file"
    );
    assert!(
        !ws.path().join("package.json").exists(),
        "reset should remove the staged (never-committed) package.json"
    );
    let (_, head_reset) = git(ws.path(), &["rev-parse", "HEAD"]);
    assert_eq!(head_before, head_reset, "reset must not move HEAD either");
}

// ---- 6. begin refuses a dirty *tracked* tree ----

#[test]
fn begin_refuses_when_a_tracked_file_has_drifted_from_head() {
    let ws = Workspace::new("dirty-tracked");
    land_green_fixture(&ws, "add the fixture ts project");

    // Modify a *tracked* file out-of-band (never through the envelope) — the case
    // that must make the baseline unsound for a future revert.
    fs::write(
        ws.path().join("package.json"),
        b"{ \"this\": \"was never staged\" }",
    )
    .expect("dirty a tracked file directly");

    let out = begin(&ws);
    assert!(
        out.contains("\"outcome\":\"refused\""),
        "begin over a dirty tracked tree must refuse: {out}"
    );
    assert!(
        out.to_lowercase()
            .contains("tracked files differ from head"),
        "the refusal should name the reason: {out}"
    );
}

// ---- 7. dependency maintenance as a first-class changeset (ADR 0009) ----

/// A genesis commit tracks its lockfile — it is no longer gitignored.
#[test]
fn genesis_commit_tracks_the_lockfile_no_longer_ignored() {
    let ws = Workspace::new("lockfile-tracked");
    land_green_fixture(&ws, "add the fixture ts project");

    assert!(
        ws.path().join("package-lock.json").is_file(),
        "resolution should have produced a lockfile"
    );
    let (ok, tracked) = git(ws.path(), &["ls-files"]);
    assert!(ok, "ls-files: {tracked}");
    assert!(
        tracked.lines().any(|f| f == "package-lock.json"),
        "package-lock.json should be tracked, not gitignored: {tracked:?}"
    );

    // Not merely present on disk untracked — it is IN the genesis commit itself.
    let (ok, files) = git(
        ws.path(),
        &["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"],
    );
    assert!(ok, "diff-tree: {files}");
    assert!(
        files.lines().any(|f| f == "package-lock.json"),
        "the lockfile should be part of the genesis commit itself: {files:?}"
    );
}

/// The agent can never author a lockfile — under genesis OR maintenance.
#[test]
fn staging_a_lockfile_is_rejected_under_every_clearance() {
    let ws = Workspace::new("lockfile-reject");
    let out = establish_empty(&ws);
    assert!(out.contains("\"outcome\":\"established\""), "{out}");

    for clearance in ["genesis", "maintenance"] {
        let out = stage_with_clearance(&ws, "package-lock.json", "{}", clearance);
        assert!(
            out.contains("\"outcome\":\"rejected\""),
            "staging a lockfile under {clearance} should be rejected: {out}"
        );
        assert!(
            out.to_lowercase().contains("computed by the trusted core"),
            "the reason should name the real cause under {clearance}: {out}"
        );
    }

    // A rejected proposal never touches the tree.
    assert!(
        !ws.path().join("package-lock.json").exists(),
        "a rejected write must never reach the tree"
    );
}

/// `package.json` is writable under Maintenance (ADR 0009's dependency-intent
/// surface); `package.json.bak` proves the allowlist is exact-match, not prefix.
#[test]
fn package_json_is_allowed_under_maintenance_but_the_bak_extension_is_not() {
    let ws = Workspace::new("package-json-maintenance");
    let out = establish_empty(&ws);
    assert!(out.contains("\"outcome\":\"established\""), "{out}");

    let out = stage_with_clearance(&ws, "package.json", FIXTURE_PACKAGE_JSON, "maintenance");
    assert!(
        out.contains("\"outcome\":\"staged\""),
        "package.json should be writable under maintenance: {out}"
    );

    let out = stage_with_clearance(&ws, "package.json.bak", FIXTURE_PACKAGE_JSON, "maintenance");
    assert!(
        out.contains("\"outcome\":\"rejected\""),
        "package.json.bak must stay rejected — the allowlist is exact-match, not a prefix: {out}"
    );
    assert!(
        !out.to_lowercase().contains("lockfile"),
        "package.json.bak is not a lockfile; it should fail the ordinary allowlist check, not the lockfile-specific one: {out}"
    );

    reset(&ws);
}

/// A `package.json` change lands together with the envelope-computed lockfile,
/// in the SAME commit.
#[test]
fn package_json_change_lands_with_the_envelope_computed_lockfile_in_the_same_commit() {
    let ws = Workspace::new("manifest-plus-lockfile");
    land_green_fixture(&ws, "add the fixture ts project");

    let (_, head_before) = git(ws.path(), &["rev-parse", "HEAD"]);

    let out = stage_with_clearance(
        &ws,
        "package.json",
        FIXTURE_PACKAGE_JSON_BUMPED,
        "maintenance",
    );
    assert!(out.contains("\"outcome\":\"staged\""), "{out}");

    let out = commit(&ws, "bump the version");
    assert!(out.contains("\"outcome\":\"committed\""), "{out}");

    let (_, head_after) = git(ws.path(), &["rev-parse", "HEAD"]);
    assert_ne!(head_before, head_after, "a new commit should have landed");

    let (ok, files) = git(
        ws.path(),
        &["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"],
    );
    assert!(ok, "diff-tree: {files}");
    let mut committed: Vec<&str> = files.lines().collect();
    committed.sort_unstable();
    assert_eq!(
        committed,
        vec!["package-lock.json", "package.json"],
        "the manifest change and the envelope-computed lockfile should land together: {files:?}"
    );

    let lockfile = fs::read_to_string(ws.path().join("package-lock.json")).expect("read lockfile");
    assert!(
        lockfile.contains("0.0.1"),
        "the lockfile should reflect the resolved manifest, not a stale one"
    );
}

/// Verification (resolve → install → build → audit) leaves the tree clean, so a
/// second changeset can still `begin` right afterward.
#[test]
fn verification_leaves_the_tree_clean_so_a_second_changeset_can_begin() {
    let ws = Workspace::new("clean-after-verify");
    land_green_fixture(&ws, "add the fixture ts project");

    let out = begin(&ws);
    assert!(
        out.contains("\"outcome\":\"begun\""),
        "a fresh changeset should open cleanly right after verification: {out}"
    );
    reset(&ws);
}

/// The audit gate is a non-regression gate: a changeset that introduces an
/// advisory absent at `HEAD` is refused.
#[test]
fn audit_gate_fails_a_changeset_that_introduces_a_new_advisory() {
    let ws = Workspace::new("audit-new-advisory");
    land_fixture_with_package_json(&ws, FIXTURE_PACKAGE_JSON, "add the fixture ts project");

    let (_, head_before) = git(ws.path(), &["rev-parse", "HEAD"]);

    // A dependency with real, well-known high-severity advisories, absent from
    // the baseline just committed above.
    let out = stage_with_clearance(
        &ws,
        "package.json",
        FIXTURE_PACKAGE_JSON_WITH_VULNERABLE_DEP,
        "maintenance",
    );
    assert!(out.contains("\"outcome\":\"staged\""), "{out}");

    let out = commit(&ws, "add a dependency that introduces a new advisory");
    assert!(
        out.contains("\"outcome\":\"build_failed\""),
        "a newly introduced advisory must fail the changeset: {out}"
    );
    assert!(
        out.to_lowercase().contains("newly introduced"),
        "the failure detail should name the audit gate: {out}"
    );

    let (_, head_after) = git(ws.path(), &["rev-parse", "HEAD"]);
    assert_eq!(
        head_before, head_after,
        "HEAD must not move when the audit gate fails"
    );
}

/// Land a baseline that already carries a dependency finding, committed
/// DIRECTLY with `git` — bypassing the envelope entirely. This is the only way
/// such a baseline can exist: once a baseline is established the gate never lets
/// a changeset introduce a new advisory (see the test above), so a "pre-existing"
/// finding is either inherited — an adopted predecessor, or history predating the
/// audit gate — or else arrived with the establishing genesis changeset, which by
/// definition has no baseline to regress from.
fn land_inherited_baseline_with_vulnerable_dep(ws: &Workspace) {
    let out = establish_empty(ws);
    assert!(out.contains("\"outcome\":\"established\""), "{out}");

    fs::write(
        ws.path().join("package.json"),
        FIXTURE_PACKAGE_JSON_WITH_VULNERABLE_DEP,
    )
    .expect("write package.json");
    fs::write(ws.path().join("tsconfig.json"), FIXTURE_TSCONFIG).expect("write tsconfig.json");
    fs::create_dir_all(ws.path().join("src")).expect("create src/");
    fs::write(ws.path().join("src/index.ts"), FIXTURE_INDEX_TS_GREEN).expect("write src/index.ts");

    // Real resolution, run directly rather than through the envelope — standing
    // in for however this predecessor's lockfile actually came to be before this
    // outcome was ever under the envelope's audit gate.
    let status = Command::new("npm")
        .args(["install", "--package-lock-only", "--ignore-scripts"])
        .current_dir(ws.path())
        .status()
        .expect("run npm install directly");
    assert!(status.success(), "npm install --package-lock-only failed");

    let (ok, out) = git(
        ws.path(),
        &[
            "add",
            "--",
            "package.json",
            "tsconfig.json",
            "src/index.ts",
            "package-lock.json",
        ],
    );
    assert!(ok, "git add for inherited baseline: {out}");
    let (ok, out) = git(
        ws.path(),
        &[
            "commit",
            "--quiet",
            "-m",
            "inherited baseline (predates the audit gate)",
        ],
    );
    assert!(ok, "git commit for inherited baseline: {out}");
}

/// The audit gate is non-regression, not zero-vulns: a changeset that leaves
/// PRE-EXISTING findings unchanged is allowed to land.
#[test]
fn audit_gate_allows_a_changeset_that_leaves_pre_existing_findings_unchanged() {
    let ws = Workspace::new("audit-pre-existing");
    land_inherited_baseline_with_vulnerable_dep(&ws);

    let (_, head_before) = git(ws.path(), &["rev-parse", "HEAD"]);

    // An unrelated manifest edit that does not touch the vulnerable dependency's
    // version: resolution reruns and reproduces the identical advisory set.
    let out = stage_with_clearance(
        &ws,
        "package.json",
        FIXTURE_PACKAGE_JSON_WITH_VULNERABLE_DEP_DESCRIBED,
        "maintenance",
    );
    assert!(out.contains("\"outcome\":\"staged\""), "{out}");

    let out = commit(&ws, "unrelated manifest edit");
    assert!(
        out.contains("\"outcome\":\"committed\""),
        "pre-existing findings that are merely unchanged must not block: {out}"
    );

    let (_, head_after) = git(ws.path(), &["rev-parse", "HEAD"]);
    assert_ne!(head_before, head_after, "a new commit should have landed");
}

/// Establishment is not a regression. A first genesis changeset whose dependencies
/// carry known advisories must LAND: it is establishing the baseline, not
/// regressing from one. Blocking it would leave the envelope unable to bring any
/// real application into existence — every mainstream stack ships some transitive
/// advisory on the day it is installed — which is an inability to start, not a
/// security property. Genesis is bounded instead by a disposable workspace, atomic
/// reversibility, and the human launch gate (ADR 0005).
#[test]
fn genesis_establishes_the_audit_baseline_rather_than_being_blocked_by_it() {
    let ws = Workspace::new("audit-genesis-baseline");
    let out = establish_empty(&ws);
    assert!(out.contains("\"outcome\":\"established\""), "{out}");

    for (path, content) in [
        ("package.json", FIXTURE_PACKAGE_JSON_WITH_VULNERABLE_DEP),
        ("tsconfig.json", FIXTURE_TSCONFIG),
        ("src/index.ts", FIXTURE_INDEX_TS_GREEN),
    ] {
        let out = stage_with_clearance(&ws, path, content, "genesis");
        assert!(
            out.contains("\"outcome\":\"staged\""),
            "stage {path}: {out}"
        );
    }

    let out = commit(&ws, "genesis: bootstrap on a stack carrying advisories");
    assert!(
        out.contains("\"outcome\":\"committed\""),
        "the establishing changeset must set the audit baseline, not be refused by it: {out}"
    );

    // ...and the envelope-computed lockfile landed in that same commit.
    let (_, tracked) = git(ws.path(), &["ls-files"]);
    assert!(
        tracked.contains("package-lock.json"),
        "the envelope-computed lockfile should be tracked: {tracked}"
    );
}

/// `envelope refresh-deps`: the pure-transitive trusted operation stages the
/// recomputed lockfile without any `package.json` change.
#[test]
fn refresh_deps_stages_the_lockfile_without_touching_package_json() {
    let ws = Workspace::new("refresh-deps");
    land_green_fixture(&ws, "add the fixture ts project");

    // Simulate a lockfile that has drifted stale relative to what resolution
    // would produce today, bypassing the envelope directly (standing in for
    // however such drift actually arises), so `refresh-deps` has real
    // resolution work to do rather than reproducing a byte-identical lockfile.
    fs::write(
        ws.path().join("package-lock.json"),
        br#"{"name":"fixture","version":"0.0.0","lockfileVersion":3,"stale":true}"#,
    )
    .expect("write a deliberately stale lockfile");
    let (ok, out) = git(ws.path(), &["add", "--", "package-lock.json"]);
    assert!(ok, "git add: {out}");
    let (ok, out) = git(
        ws.path(),
        &["commit", "--quiet", "-m", "simulate a stale lockfile"],
    );
    assert!(ok, "git commit: {out}");

    let out = envelope(&["refresh-deps", "--repo", ws.path_str()], &[]);
    assert!(
        out.contains("\"outcome\":\"staged\""),
        "refresh-deps should stage the recomputed lockfile: {out}"
    );
    assert!(
        out.contains("package-lock.json"),
        "refresh-deps should name the lockfile it staged: {out}"
    );

    let out = commit(&ws, "refresh dependencies");
    assert!(out.contains("\"outcome\":\"committed\""), "{out}");

    // `npm install --package-lock-only` updates an existing lockfile in place
    // rather than replacing it wholesale (an unrelated top-level key like our
    // injected `"stale"` marker survives), so the real signal that resolution
    // actually ran is the resolved dependency data it must have freshly
    // computed — our stale stub had none.
    let lockfile = fs::read_to_string(ws.path().join("package-lock.json")).expect("read lockfile");
    assert!(
        lockfile.contains("\"integrity\""),
        "the lockfile should have been recomputed by refresh-deps with real resolution data: {lockfile}"
    );
}

// ---- 8. the envelope stamps the trust roles on every commit (ADR 0010) ----

/// A changeset `commit` is authored by the advisor and committed by the
/// envelope — regardless of the test harness's own ambient `GIT_*` identity
/// envs (`git_identity_envs`), proving the envelope's explicit identity always
/// wins over whatever git identity happens to be configured.
#[test]
fn changeset_commit_is_attributed_to_advisor_author_and_envelope_committer() {
    let ws = Workspace::new("commit-identity");
    land_green_fixture(&ws, "add the fixture ts project");

    let (ok, out) = git(ws.path(), &["log", "-1", "--format=%an <%ae>|%cn <%ce>"]);
    assert!(ok, "git log: {out}");
    assert_eq!(
        out.trim(),
        "Autopilot advisor <advisor@autopilot.invalid>|Autopilot envelope <envelope@autopilot.invalid>",
        "a changeset commit should be authored by the advisor and committed by the envelope, not the test harness's ambient git identity: {out}"
    );
}

/// The `establish` baseline commit is trusted setup, not the advisor's work: both
/// author and committer are the envelope, again regardless of the harness's
/// ambient `GIT_*` envs.
#[test]
fn establish_baseline_commit_is_attributed_to_the_envelope_as_both_author_and_committer() {
    let ws = Workspace::new("establish-identity");
    let out = establish_empty(&ws);
    assert!(out.contains("\"outcome\":\"established\""), "{out}");

    let (ok, out) = git(ws.path(), &["log", "-1", "--format=%an <%ae>|%cn <%ce>"]);
    assert!(ok, "git log: {out}");
    assert_eq!(
        out.trim(),
        "Autopilot envelope <envelope@autopilot.invalid>|Autopilot envelope <envelope@autopilot.invalid>",
        "the establish baseline should be attributed to the envelope alone, as both author and committer: {out}"
    );
}

/// A multi-line `commit` intent (what `commit_changeset`'s `summary` becomes) is
/// preserved verbatim — subject on the first line, a blank line, then the body —
/// never flattened into a single enormous subject line.
#[test]
fn multiline_commit_summary_is_preserved_not_flattened() {
    let ws = Workspace::new("multiline-message");
    let out = establish_empty(&ws);
    assert!(out.contains("\"outcome\":\"established\""), "{out}");
    stage_green_fixture(&ws);

    let intent = "Add the fixture TS project\n\nBrings in package.json, tsconfig.json, and a\nminimal src/index.ts so the build has something to check.";
    let out = commit(&ws, intent);
    assert!(out.contains("\"outcome\":\"committed\""), "{out}");

    let (ok, body) = git(ws.path(), &["log", "-1", "--format=%B"]);
    assert!(ok, "git log: {body}");
    let lines: Vec<&str> = body.lines().collect();
    assert_eq!(
        lines[0], "Add the fixture TS project",
        "the subject should be the message's first line, unflattened: {lines:?}"
    );
    assert_eq!(
        lines[1], "",
        "a blank line should separate subject from body: {lines:?}"
    );
    assert!(
        body.contains(
            "Brings in package.json, tsconfig.json, and a\nminimal src/index.ts so the build has something to check."
        ),
        "the body should keep its internal newline rather than being collapsed to spaces: {body:?}"
    );
}

// ---- 9. the verifier's test stage: build-green is not enough (partially discharges R7) ----

/// A changeset whose `test` script fails is rejected by the build gate, even
/// though `tsc`/`vite build` themselves are green — the exact shape a runtime
/// crash a type checker cannot see takes (THREAT_MODEL.md R7).
#[test]
fn test_stage_fails_a_changeset_whose_test_script_fails() {
    let ws = Workspace::new("test-stage-red");
    land_green_fixture(&ws, "add the fixture ts project");

    let (_, head_before) = git(ws.path(), &["rev-parse", "HEAD"]);

    let out = stage_with_clearance(
        &ws,
        "package.json",
        FIXTURE_PACKAGE_JSON_WITH_FAILING_TEST,
        // Genesis, not Maintenance: this fixture alters `scripts`, so under
        // Maintenance the scripts-freeze gate (T16) would refuse the changeset
        // before `npm test` ever ran — the changeset would still fail, but for
        // the wrong reason, and this test would silently stop covering the test
        // stage at all. The negative assertion below pins that down.
        "genesis",
    );
    assert!(out.contains("\"outcome\":\"staged\""), "{out}");

    let out = commit(&ws, "add a test script that fails");
    assert!(
        out.contains("\"outcome\":\"build_failed\""),
        "a failing test script must fail the changeset, exactly like a failing build: {out}"
    );
    assert!(
        !out.contains("`scripts`"),
        "the failure must come from the test stage, not the scripts-freeze gate: {out}"
    );

    let (_, head_after) = git(ws.path(), &["rev-parse", "HEAD"]);
    assert_eq!(
        head_before, head_after,
        "HEAD must not move when the test stage fails"
    );
}

/// A changeset whose `test` script passes commits green, same as any other
/// changeset — the test stage is additive, not a new obstacle for outcomes
/// that already pass it.
#[test]
fn test_stage_allows_a_changeset_whose_test_script_passes() {
    let ws = Workspace::new("test-stage-green");
    land_fixture_with_package_json(
        &ws,
        FIXTURE_PACKAGE_JSON_WITH_PASSING_TEST,
        "add the fixture ts project with a passing test",
    );
    // `land_fixture_with_package_json` already asserts `"outcome":"committed"` —
    // reaching this point means build AND test both passed.
}

// ---- 10. the runtime envelope's fast loop: `envelope monitor` (ADR 0011) ----

/// Write a small hand-authored telemetry fixture inside the workspace (an
/// untracked file — `ensure_clean` tolerates those, same as any other build
/// byproduct) and return its path.
fn write_telemetry(ws: &Workspace, contents: &str) -> PathBuf {
    let path = ws.path().join("telemetry.json");
    fs::write(&path, contents).expect("write telemetry fixture");
    path
}

fn monitor(ws: &Workspace, telemetry: &Path, threshold: Option<&str>) -> String {
    let telemetry_str = telemetry.to_str().expect("telemetry path is valid utf-8");
    let mut args = vec![
        "monitor",
        "--repo",
        ws.path_str(),
        "--telemetry",
        telemetry_str,
    ];
    if let Some(t) = threshold {
        args.push("--threshold");
        args.push(t);
    }
    envelope(&args, &[])
}

/// An error rate above threshold trips the runtime envelope: it reverts the
/// currently-deployed change with a real `git revert`, stamped as the
/// envelope's own action — both author AND committer, no advisor identity
/// anywhere in it, because no advisor supplied anything this path used.
#[test]
fn monitor_trips_and_reverts_on_an_error_rate_breach_stamped_as_the_envelope() {
    let ws = Workspace::new("monitor-tripped");
    land_green_fixture(&ws, "add the fixture ts project");

    // A second, "deployed" changeset — what the trip below will revert.
    let out = stage(&ws, "src/extra.ts", FIXTURE_EXTRA_TS_GREEN);
    assert!(out.contains("\"outcome\":\"staged\""), "{out}");
    let out = commit(&ws, "the deployed change the trip should revert");
    assert!(out.contains("\"outcome\":\"committed\""), "{out}");
    assert!(ws.path().join("src/extra.ts").is_file());

    let (_, head_before) = git(ws.path(), &["rev-parse", "HEAD"]);

    let telemetry = write_telemetry(&ws, r#"{"error_rate": 0.5, "threshold": 0.02}"#);
    let out = monitor(&ws, &telemetry, None);
    assert!(
        out.contains("\"outcome\":\"tripped\""),
        "an error-rate breach should trip the runtime envelope: {out}"
    );

    let (_, head_after) = git(ws.path(), &["rev-parse", "HEAD"]);
    assert_ne!(
        head_before, head_after,
        "a trip should land a new revert commit"
    );

    // The revert is attributed to the envelope alone — no advisor identity,
    // because the advisor supplied nothing on this path (ADR 0010, ADR 0011).
    let (ok, identity) = git(ws.path(), &["log", "-1", "--format=%an <%ae>|%cn <%ce>"]);
    assert!(ok, "git log: {identity}");
    assert_eq!(
        identity.trim(),
        "Autopilot envelope <envelope@autopilot.invalid>|Autopilot envelope <envelope@autopilot.invalid>",
        "the trip's revert must be attributed to the envelope alone, as both author and committer: {identity}"
    );

    // The revert actually undid the deployed change.
    assert!(
        !ws.path().join("src/extra.ts").exists(),
        "the revert should remove what the reverted commit added"
    );
}

/// An error rate at or below threshold leaves the deployed change exactly as
/// it was — no revert, no commit, `HEAD` unmoved.
#[test]
fn monitor_reports_nominal_and_changes_nothing_below_threshold() {
    let ws = Workspace::new("monitor-nominal");
    land_green_fixture(&ws, "add the fixture ts project");

    let (_, head_before) = git(ws.path(), &["rev-parse", "HEAD"]);

    let telemetry = write_telemetry(&ws, r#"{"error_rate": 0.001, "threshold": 0.02}"#);
    let out = monitor(&ws, &telemetry, None);
    assert!(
        out.contains("\"outcome\":\"nominal\""),
        "a healthy error rate must not trip: {out}"
    );

    let (_, head_after) = git(ws.path(), &["rev-parse", "HEAD"]);
    assert_eq!(
        head_before, head_after,
        "nominal telemetry must not move HEAD"
    );
}

/// `--threshold` overrides whatever the telemetry file itself records — an
/// operator narrowing the SLO for one call, independent of the sensor's own
/// recorded ceiling.
#[test]
fn monitor_threshold_flag_overrides_the_telemetry_files_own_threshold() {
    let ws = Workspace::new("monitor-threshold-override");
    land_green_fixture(&ws, "add the fixture ts project");

    let (_, head_before) = git(ws.path(), &["rev-parse", "HEAD"]);

    // The file's own threshold (0.9) alone would call 0.5 nominal.
    let telemetry = write_telemetry(&ws, r#"{"error_rate": 0.5, "threshold": 0.9}"#);
    let out = monitor(&ws, &telemetry, Some("0.1"));
    assert!(
        out.contains("\"outcome\":\"tripped\""),
        "the --threshold flag should win over the telemetry file's own threshold: {out}"
    );

    let (_, head_after) = git(ws.path(), &["rev-parse", "HEAD"]);
    assert_ne!(head_before, head_after, "the override should have tripped");
}

// ---- 11. design conformance: the design-system invariant (ADR 0012, Phase D) ----

/// Stage the design-system stub as trusted baseline infrastructure — genesis
/// clearance, mirroring how the real Phase D scenario seeds
/// `src/design-system/` before any maintenance changeset touches a page (see
/// the freeze test below: Maintenance itself cannot write this zone).
fn land_design_system_stub(ws: &Workspace) {
    let out = stage_with_clearance(
        ws,
        "src/design-system/index.ts",
        FIXTURE_DESIGN_SYSTEM_INDEX_TS,
        "genesis",
    );
    assert!(out.contains("\"outcome\":\"staged\""), "{out}");
    let out = commit(
        ws,
        "seed the design system as trusted baseline infrastructure",
    );
    assert!(out.contains("\"outcome\":\"committed\""), "{out}");
}

/// A maintenance changeset that composes a page from the design system —
/// imports it, uses none of the banned raw tags, no inline style — passes the
/// design stage and commits green, same as any other conformant change.
#[test]
fn design_conformant_page_commits_green() {
    let ws = Workspace::new("design-conformant");
    land_green_fixture(&ws, "add the fixture ts project");
    land_design_system_stub(&ws);

    let out = stage_with_clearance(
        &ws,
        "src/pages/Toolbar.tsx",
        FIXTURE_CONFORMANT_PAGE_TSX,
        "maintenance",
    );
    assert!(out.contains("\"outcome\":\"staged\""), "{out}");

    let out = commit(&ws, "add a page composed from the design system");
    assert!(
        out.contains("\"outcome\":\"committed\""),
        "a conformant page should pass the design stage and commit: {out}"
    );
}

/// A page that reaches for a raw `<button>` and an inline `style={{...}}`
/// instead of the design system is rejected by the design stage — the build
/// itself is green (the fixture is valid TypeScript), so this is specifically
/// the design-conformance gate firing, not `tsc`.
#[test]
fn design_violating_page_is_rejected_by_the_design_stage() {
    let ws = Workspace::new("design-violation");
    land_green_fixture(&ws, "add the fixture ts project");
    land_design_system_stub(&ws);

    let (_, head_before) = git(ws.path(), &["rev-parse", "HEAD"]);

    let out = stage_with_clearance(
        &ws,
        "src/pages/Toolbar.tsx",
        FIXTURE_VIOLATING_PAGE_TSX,
        "maintenance",
    );
    assert!(out.contains("\"outcome\":\"staged\""), "{out}");

    let out = commit(&ws, "add a page that reaches for raw markup");
    assert!(
        out.contains("\"outcome\":\"build_failed\""),
        "a raw <button> with an inline style must fail the design stage: {out}"
    );
    assert!(
        out.contains("src/pages/Toolbar.tsx"),
        "the rejection should name the offending file: {out}"
    );
    assert!(
        out.contains("raw `<button`"),
        "the rejection should name the specific rule: {out}"
    );
    assert!(
        out.contains("inline `style={{"),
        "the rejection should name the inline-style rule too: {out}"
    );

    let (_, head_after) = git(ws.path(), &["rev-parse", "HEAD"]);
    assert_eq!(
        head_before, head_after,
        "HEAD must not move when the design stage rejects the changeset"
    );
}

/// `src/design-system/` is frozen under Maintenance (ADR 0012): the agent
/// composes UI from the primitives but cannot fork or edit them, the same
/// discipline as `src/api/`/`src/data/`. Genesis, which brings the primitives
/// into existence in the first place, is unaffected.
#[test]
fn design_system_is_frozen_under_maintenance_but_writable_under_genesis() {
    let ws = Workspace::new("design-system-frozen");
    let out = establish_empty(&ws);
    assert!(out.contains("\"outcome\":\"established\""), "{out}");

    let out = stage_with_clearance(
        &ws,
        "src/design-system/Button.tsx",
        "export const Button = 0;\n",
        "maintenance",
    );
    assert!(
        out.contains("\"outcome\":\"rejected\""),
        "maintenance must not be able to write the design system: {out}"
    );
    assert!(
        out.to_lowercase().contains("forbidden"),
        "the reason should name a forbidden zone: {out}"
    );

    let out = stage_with_clearance(
        &ws,
        "src/design-system/Button.tsx",
        "export const Button = 0;\n",
        "genesis",
    );
    assert!(
        out.contains("\"outcome\":\"staged\""),
        "genesis must be able to establish the design system in the first place: {out}"
    );

    reset(&ws);
}

// ---- 12. mandatory script presence: absence now fails, not skips (ADR 0013) ----

/// A `package.json` with no `test` script fails the changeset, naming the
/// missing script — the amended behaviour of the pre-existing test stage
/// (ADR 0011 made it additive; ADR 0013 makes it mandatory). Uses Genesis
/// clearance so the mandatory-presence check is what fires (under Maintenance,
/// P1's scripts-freeze gate would fire first for the same fixture — that case
/// is covered by `maintenance_changeset_editing_scripts_is_refused_naming_the_rule`).
#[test]
fn missing_test_script_fails_the_changeset_naming_the_missing_script() {
    let ws = Workspace::new("mandatory-test-missing");
    land_green_fixture(&ws, "add the fixture ts project");

    let (_, head_before) = git(ws.path(), &["rev-parse", "HEAD"]);

    let out = stage_with_clearance(
        &ws,
        "package.json",
        FIXTURE_PACKAGE_JSON_NO_TEST_SCRIPT,
        "genesis",
    );
    assert!(out.contains("\"outcome\":\"staged\""), "{out}");

    let out = commit(&ws, "drop the test script");
    assert!(
        out.contains("\"outcome\":\"build_failed\""),
        "a missing `test` script must now fail the changeset, not silently skip it: {out}"
    );
    assert!(
        out.contains("no `test` script declared"),
        "the failure should name the missing script: {out}"
    );

    let (_, head_after) = git(ws.path(), &["rev-parse", "HEAD"]);
    assert_eq!(
        head_before, head_after,
        "HEAD must not move when the mandatory-script gate fails"
    );
}

/// The parallel case for `test:e2e` (T15, new as of this milestone): its
/// absence fails the changeset the same way, naming the missing script. Uses
/// Genesis clearance for the same reason as the `test` parallel above.
#[test]
fn missing_e2e_script_fails_the_changeset_naming_the_missing_script() {
    let ws = Workspace::new("mandatory-e2e-missing");
    land_green_fixture(&ws, "add the fixture ts project");

    let (_, head_before) = git(ws.path(), &["rev-parse", "HEAD"]);

    let out = stage_with_clearance(
        &ws,
        "package.json",
        FIXTURE_PACKAGE_JSON_NO_E2E_SCRIPT,
        "genesis",
    );
    assert!(out.contains("\"outcome\":\"staged\""), "{out}");

    let out = commit(&ws, "drop the test:e2e script");
    assert!(
        out.contains("\"outcome\":\"build_failed\""),
        "a missing `test:e2e` script must fail the changeset: {out}"
    );
    assert!(
        out.contains("no `test:e2e` script declared"),
        "the failure should name the missing script: {out}"
    );

    let (_, head_after) = git(ws.path(), &["rev-parse", "HEAD"]);
    assert_eq!(
        head_before, head_after,
        "HEAD must not move when the mandatory-script gate fails"
    );
}

// ---- 13. scripts-freeze under Maintenance: the agent cannot alter gate scripts (ADR 0013, T16) ----

/// A Maintenance changeset that adds a `pretest` hook to the `scripts` object —
/// all mandatory scripts still present, only the object itself differs from HEAD —
/// is refused by the scripts-freeze gate before any npm invocation, naming the
/// rule. The bypass this closes: `pretest` runs automatically under npm, so a
/// hook injected alongside a bug would fire during the test run and could silence
/// it without touching the gate-key scripts directly.
#[test]
fn maintenance_changeset_editing_scripts_is_refused_naming_the_rule() {
    let ws = Workspace::new("scripts-freeze-maintenance");
    land_green_fixture(&ws, "add the fixture ts project");

    let (_, head_before) = git(ws.path(), &["rev-parse", "HEAD"]);

    let out = stage_with_clearance(
        &ws,
        "package.json",
        FIXTURE_PACKAGE_JSON_SCRIPTS_WITH_HOOK,
        "maintenance",
    );
    assert!(out.contains("\"outcome\":\"staged\""), "{out}");

    let out = commit(&ws, "inject a pretest hook under maintenance");
    assert!(
        out.contains("\"outcome\":\"build_failed\""),
        "a scripts-object change under Maintenance must fail the scripts-freeze gate: {out}"
    );
    assert!(
        out.contains("may not alter the `scripts` object"),
        "the failure should name the scripts-freeze rule: {out}"
    );

    let (_, head_after) = git(ws.path(), &["rev-parse", "HEAD"]);
    assert_eq!(
        head_before, head_after,
        "HEAD must not move when the scripts-freeze gate fires"
    );
}

/// The same scripts-object edit under Genesis clearance commits green — Genesis
/// writes scripts freely; the scripts-freeze gate is Maintenance-only (T16).
/// Proves the restriction is clearance-scoped, not a blanket lock on scripts.
#[test]
fn genesis_changeset_with_altered_scripts_commits_green() {
    let ws = Workspace::new("scripts-freeze-genesis");
    land_green_fixture(&ws, "add the fixture ts project");

    let out = stage_with_clearance(
        &ws,
        "package.json",
        FIXTURE_PACKAGE_JSON_SCRIPTS_WITH_HOOK,
        "genesis",
    );
    assert!(out.contains("\"outcome\":\"staged\""), "{out}");

    let out = commit(&ws, "genesis: alter scripts freely");
    assert!(
        out.contains("\"outcome\":\"committed\""),
        "a scripts-object change under Genesis must not be blocked: {out}"
    );
}

/// A Maintenance changeset that changes only the `version` field — scripts and
/// `devDependencies` byte-identical to the baseline — commits green. This proves
/// ADR 0009's dependency-intent surface (non-scripts package.json changes) is
/// unaffected by the scripts-freeze gate: the gate compares the scripts object
/// only, not the whole manifest.
#[test]
fn maintenance_changeset_editing_non_scripts_fields_still_commits() {
    let ws = Workspace::new("scripts-freeze-non-scripts");
    land_green_fixture(&ws, "add the fixture ts project");

    // FIXTURE_PACKAGE_JSON_BUMPED changes only `version`; scripts are
    // byte-identical to the baseline that land_green_fixture committed.
    let out = stage_with_clearance(
        &ws,
        "package.json",
        FIXTURE_PACKAGE_JSON_BUMPED,
        "maintenance",
    );
    assert!(out.contains("\"outcome\":\"staged\""), "{out}");

    let out = commit(&ws, "bump the version under maintenance");
    assert!(
        out.contains("\"outcome\":\"committed\""),
        "a non-scripts Maintenance edit to package.json must still commit: {out}"
    );
}

// ---- 14. the e2e stage: build-and-test-green still is not enough (ADR 0013, T15) ----

/// A changeset whose Playwright `test:e2e` spec passes — a real Vite build, a
/// real headless-browser page load against the actually-built `dist/` —
/// commits green, the e2e analogue of
/// `test_stage_allows_a_changeset_whose_test_script_passes`.
#[test]
fn e2e_stage_allows_a_changeset_whose_e2e_spec_passes() {
    let ws = Workspace::new("e2e-stage-green");
    let out = establish_empty(&ws);
    assert!(out.contains("\"outcome\":\"established\""), "{out}");

    stage_playwright_fixture(&ws, FIXTURE_E2E_APP_GREEN_TSX);

    let out = commit(
        &ws,
        "genesis: minimal Vite+React app with a passing e2e smoke spec",
    );
    assert!(
        out.contains("\"outcome\":\"committed\""),
        "a conformant app should pass the e2e stage and commit: {out}"
    );
}

/// A top-level component that throws unconditionally during render typechecks
/// fine and builds green (`vite build` never executes the component — it only
/// bundles it), so neither the build nor the (jsdom-free) `tsc` check can see
/// this coming. The Playwright spec drives a real browser against the real
/// built `dist/` and observes the throw; the changeset fails closed and
/// `HEAD` never moves — the e2e analogue of
/// `test_stage_fails_a_changeset_whose_test_script_fails`, and a direct
/// demonstration of R7/T15's "general case": no seeded reproducer, no prior
/// incident, just a change that compiles but breaks the rendered page.
#[test]
fn e2e_stage_fails_a_changeset_whose_component_throws_during_render() {
    let ws = Workspace::new("e2e-stage-red");
    let out = establish_empty(&ws);
    assert!(out.contains("\"outcome\":\"established\""), "{out}");

    let (_, head_before) = git(ws.path(), &["rev-parse", "HEAD"]);

    stage_playwright_fixture(&ws, FIXTURE_E2E_APP_THROWS_TSX);

    let out = commit(
        &ws,
        "genesis: a component that throws unconditionally during render",
    );
    assert!(
        out.contains("\"outcome\":\"build_failed\""),
        "a component that throws during render must fail the e2e stage, even though tsc/vite build are green: {out}"
    );

    let (_, head_after) = git(ws.path(), &["rev-parse", "HEAD"]);
    assert_eq!(
        head_before, head_after,
        "HEAD must not move when the e2e stage fails"
    );
}

// ---- 15. establish_clone's precondition stays permissive; the first real
// changeset against the adopted workspace does not (ADR 0013, corrected
// mechanism) ----

/// `Establish::Clone`'s precondition is UNCHANGED by this milestone
/// (predecessor's own build must be green — nothing about test scripts), so a
/// legacy, green-but-test-less predecessor still adopts cleanly. The
/// mandatory `test`/`test:e2e` bar bites instead at the advisor's FIRST real
/// changeset against the adopted workspace, via `changeset_is_open` — proving
/// the corrected mechanism from ADR 0013: adoption stays permissive, the
/// first real changeset does not, so a Clone-based genesis can still add e2e
/// coverage as its own first act, exactly like an Empty-based one can.
#[test]
fn establish_clone_adopts_a_test_less_predecessor_but_refuses_the_first_changeset_against_it() {
    let predecessor = Workspace::new("clone-predecessor");
    let out = establish_empty(&predecessor);
    assert!(
        out.contains("\"outcome\":\"established\""),
        "establish predecessor: {out}"
    );

    // Write a green-but-test-less app DIRECTLY (bypassing the envelope's own
    // commit gate entirely) — standing in for however such a predecessor
    // actually came to exist (history predating ADR 0013, or adopted from
    // outside this system altogether). This is the only way such a baseline
    // CAN exist: once a repo is under the envelope's own commit gate, a real
    // changeset can never land without both scripts (see the tests above) —
    // the same reasoning `land_inherited_baseline_with_vulnerable_dep` above
    // already uses for the audit gate's pre-existing-baseline case.
    fs::write(
        predecessor.path().join("package.json"),
        FIXTURE_PACKAGE_JSON_NO_TEST_OR_E2E,
    )
    .expect("write predecessor package.json");
    fs::write(predecessor.path().join("tsconfig.json"), FIXTURE_TSCONFIG)
        .expect("write predecessor tsconfig.json");
    fs::create_dir_all(predecessor.path().join("src")).expect("create predecessor src/");
    fs::write(
        predecessor.path().join("src/index.ts"),
        FIXTURE_INDEX_TS_GREEN,
    )
    .expect("write predecessor src/index.ts");
    let status = Command::new("npm")
        .args(["install", "--package-lock-only", "--ignore-scripts"])
        .current_dir(predecessor.path())
        .status()
        .expect("run npm install directly");
    assert!(status.success(), "npm install --package-lock-only failed");
    let (ok, out) = git(
        predecessor.path(),
        &[
            "add",
            "--",
            "package.json",
            "tsconfig.json",
            "src/index.ts",
            "package-lock.json",
        ],
    );
    assert!(ok, "git add for predecessor: {out}");
    let (ok, out) = git(
        predecessor.path(),
        &[
            "commit",
            "--quiet",
            "-m",
            "legacy predecessor: green, no test scripts",
        ],
    );
    assert!(ok, "git commit for predecessor: {out}");

    // Adopt it. The precondition is build-green only — no opinion about test
    // scripts at all — so this must succeed even though the predecessor has
    // neither `test` nor `test:e2e`.
    let clone_target = Workspace::new("clone-adopted");
    let out = envelope(
        &[
            "establish",
            "--repo",
            clone_target.path_str(),
            "--mode",
            "clone",
            "--source",
            predecessor.path_str(),
        ],
        &[],
    );
    assert!(
        out.contains("\"outcome\":\"established\""),
        "cloning a green-but-test-less predecessor must succeed — the precondition is build-green only: {out}"
    );

    // The advisor's first real changeset against the adopted workspace is
    // refused until BOTH mandatory scripts exist and pass.
    let out = stage_with_clearance(
        &clone_target,
        "src/components/extra.ts",
        FIXTURE_EXTRA_TS_GREEN,
        "maintenance",
    );
    assert!(out.contains("\"outcome\":\"staged\""), "{out}");

    let out = commit(
        &clone_target,
        "the advisor's first change against the adopted predecessor",
    );
    assert!(
        out.contains("\"outcome\":\"build_failed\""),
        "the FIRST real changeset against an adopted test-less predecessor must be refused: {out}"
    );
    assert!(
        out.contains("no `test` script declared"),
        "the refusal should name the missing test script: {out}"
    );
}

// ---- 16. the clearance stamp: the repo names its own reach clearance, never
// the command line ----

/// The hole this milestone closes. Pre-fix, `stage` took reach clearance from
/// an argv `--clearance` flag supplied by the SAME untrusted caller whose write
/// was being judged: a workspace committed as a green baseline had no
/// persistent memory of "this is now Maintenance" once the changeset that
/// landed it closed, so the very next `stage` call could simply claim
/// `--clearance genesis` and the envelope believed it — unfreezing `src/data/`
/// (and, in the real advisor, the design system) for a caller that should never
/// have had that reach. Run against the pre-fix code (`d0954cb`) with a literal
/// `--clearance genesis` on this exact scenario, this test FAILED: the write
/// came back `{"outcome":"staged","path":"src/data/anything.ts"}` instead of
/// rejected — that failure was the proof the hole was real.
///
/// Post-fix, `--clearance` is not merely ignored, it does not exist as a
/// `stage` flag at all (`the_clearance_flag_is_no_longer_accepted_by_stage`
/// below pins that down) — reach clearance is read from the repo's own
/// persistent stamp (`.git/envelope-clearance`), flipped only by the operator
/// via `envelope clearance --set`. There is no longer any argv path that could
/// widen it, `--clearance` or otherwise, so this test now proves the positive:
/// a Maintenance-stamped workspace refuses a frozen-zone write via the
/// ORDINARY stage invocation, unconditionally.
#[test]
fn a_maintenance_workspace_refuses_a_frozen_zone_write_regardless_of_the_command_line() {
    let ws = Workspace::new("clearance-stamp-hole");
    land_green_fixture(&ws, "add the fixture ts project");

    // The workspace has launched (a green baseline is committed); stamp it
    // Maintenance, the way the operator would once the outcome is live.
    let out = stage_with_clearance(
        &ws,
        "src/data/anything.ts",
        "export const x = 1;\n",
        "maintenance",
    );
    assert!(
        out.contains("\"outcome\":\"rejected\""),
        "a Maintenance-stamped workspace must refuse a src/data/ write: {out}"
    );
    assert!(
        out.contains("\"invariant\":\"reach\""),
        "the rejection should name reach: {out}"
    );
    assert!(
        !ws.path().join("src/data/anything.ts").exists(),
        "a rejected write must never reach the tree"
    );
}

/// A repo with no clearance stamp at all (e.g. one predating this milestone, or
/// one whose stamp was somehow removed) adjudicates as Maintenance — fail
/// closed to the narrow clearance, never the broad one.
#[test]
fn an_unstamped_repo_adjudicates_as_maintenance() {
    let ws = Workspace::new("unstamped-repo");
    land_green_fixture(&ws, "add the fixture ts project");

    fs::remove_file(ws.path().join(".git").join("envelope-clearance"))
        .expect("remove the clearance stamp");

    let out = envelope(
        &[
            "stage",
            "--repo",
            ws.path_str(),
            "--path",
            "src/data/anything.ts",
        ],
        b"export const x = 1;\n",
    );
    assert!(
        out.contains("\"outcome\":\"rejected\""),
        "an unstamped repo must adjudicate as Maintenance (fail closed): {out}"
    );
    assert!(
        out.contains("\"invariant\":\"reach\""),
        "the rejection should name reach: {out}"
    );
}

/// A malformed stamp (anything other than exactly `genesis` or `maintenance`)
/// falls back to Maintenance — same fail-closed behaviour as a missing stamp.
#[test]
fn a_malformed_stamp_falls_back_to_maintenance() {
    let ws = Workspace::new("malformed-stamp");
    land_green_fixture(&ws, "add the fixture ts project");

    fs::write(
        ws.path().join(".git").join("envelope-clearance"),
        b"nonsense\n",
    )
    .expect("write a garbage clearance stamp");

    let out = envelope(
        &[
            "stage",
            "--repo",
            ws.path_str(),
            "--path",
            "src/data/anything.ts",
        ],
        b"export const x = 1;\n",
    );
    assert!(
        out.contains("\"outcome\":\"rejected\""),
        "a malformed stamp must fall back to Maintenance (fail closed): {out}"
    );
    assert!(
        out.contains("\"invariant\":\"reach\""),
        "the rejection should name reach: {out}"
    );
}

/// `establish` stamps Genesis, and the operator-only `clearance --set` command
/// flips it: a path only Genesis reaches (the design system) stages right
/// after establish, then is rejected once the operator sets Maintenance.
#[test]
fn establish_stamps_genesis_and_the_clearance_command_flips_it() {
    let ws = Workspace::new("establish-stamps-genesis");
    let out = establish_empty(&ws);
    assert!(out.contains("\"outcome\":\"established\""), "{out}");

    let read_out = envelope(&["clearance", "--repo", ws.path_str()], &[]);
    assert!(
        read_out.contains("\"outcome\":\"clearance\"")
            && read_out.contains("\"clearance\":\"genesis\""),
        "establish should stamp genesis: {read_out}"
    );

    let out = stage(
        &ws,
        "src/design-system/Button.tsx",
        "export const Button = 0;\n",
    );
    assert!(
        out.contains("\"outcome\":\"staged\""),
        "the genesis stamp should let a fresh workspace write the design system: {out}"
    );
    reset(&ws);

    let set_out = envelope(
        &["clearance", "--repo", ws.path_str(), "--set", "maintenance"],
        &[],
    );
    assert!(
        set_out.contains("\"outcome\":\"clearance_set\""),
        "{set_out}"
    );

    let out = envelope(
        &[
            "stage",
            "--repo",
            ws.path_str(),
            "--path",
            "src/design-system/Button.tsx",
        ],
        b"export const Button = 0;\n",
    );
    assert!(
        out.contains("\"outcome\":\"rejected\""),
        "once flipped to maintenance, the design system must be frozen: {out}"
    );
}

/// `--clearance` is not merely ignored by `stage` — it is not a recognised flag
/// at all, so `stage --clearance genesis` fails as an unknown flag (exit 2),
/// never as a silently-accepted no-op.
#[test]
fn the_clearance_flag_is_no_longer_accepted_by_stage() {
    let ws = Workspace::new("clearance-flag-removed");
    let out = establish_empty(&ws);
    assert!(out.contains("\"outcome\":\"established\""), "{out}");

    let mut cmd = Command::new(envelope_bin());
    cmd.args([
        "stage",
        "--repo",
        ws.path_str(),
        "--path",
        "package.json",
        "--clearance",
        "genesis",
    ])
    .envs(git_identity_envs())
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn envelope binary");
    child
        .stdin
        .take()
        .expect("envelope stdin")
        .write_all(FIXTURE_PACKAGE_JSON.as_bytes())
        .expect("write envelope stdin");
    let out = child.wait_with_output().expect("wait on envelope");
    assert_eq!(
        out.status.code(),
        Some(2),
        "an unknown `--clearance` flag should exit 2: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        combined.to_lowercase().contains("unknown flag"),
        "the failure should name the unrecognised flag: {combined}"
    );
}
