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

const FIXTURE_PACKAGE_JSON: &str = r#"{
  "name": "fixture",
  "private": true,
  "version": "0.0.0",
  "scripts": { "build": "tsc" },
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
  "scripts": { "build": "tsc" },
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
  "scripts": { "build": "tsc" },
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
  "scripts": { "build": "tsc" },
  "devDependencies": { "typescript": "^5.5.4" },
  "dependencies": { "minimatch": "3.0.0" }
}
"#;

// A `test` script that always succeeds — a plain `node -e`, not a real test
// runner, so the test-stage tests below stay fast and need no extra
// dependency beyond what the fixture already resolves. What matters to the
// gate is only that the outcome DECLARES a `test` script and that script's
// exit code, not what it actually does.
const FIXTURE_PACKAGE_JSON_WITH_PASSING_TEST: &str = r#"{
  "name": "fixture",
  "private": true,
  "version": "0.0.0",
  "scripts": { "build": "tsc", "test": "node -e \"process.exit(0)\"" },
  "devDependencies": { "typescript": "^5.5.4" }
}
"#;

// Same shape, but the `test` script fails — the build-green/test-red case the
// test stage exists to catch (THREAT_MODEL.md R7).
const FIXTURE_PACKAGE_JSON_WITH_FAILING_TEST: &str = r#"{
  "name": "fixture",
  "private": true,
  "version": "0.0.0",
  "scripts": { "build": "tsc", "test": "node -e \"process.exit(1)\"" },
  "devDependencies": { "typescript": "^5.5.4" }
}
"#;

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
fn stage_with_clearance(ws: &Workspace, rel_path: &str, content: &str, clearance: &str) -> String {
    envelope(
        &[
            "stage",
            "--repo",
            ws.path_str(),
            "--path",
            rel_path,
            "--clearance",
            clearance,
        ],
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
        "maintenance",
    );
    assert!(out.contains("\"outcome\":\"staged\""), "{out}");

    let out = commit(&ws, "add a test script that fails");
    assert!(
        out.contains("\"outcome\":\"build_failed\""),
        "a failing test script must fail the changeset, exactly like a failing build: {out}"
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
