//! Real-path integration tests for the changeset lifecycle (ADR 0005): `establish`
//! → `begin` → `stage`* → `commit` / `reset`, driven exactly as the advisor drives
//! them — through the compiled `envelope` binary, against throwaway git repos in
//! temp dirs.
//!
//! Each test gets its own fresh workspace and drives it end to end with real
//! `git` and (where a build is involved) a real `npm install && npm run build`.
//! The npm-dependent tests are slower (seconds, not milliseconds) but exercise the
//! actual trusted gate, not a stand-in for it — the same tradeoff the verifier
//! itself makes.
//!
//! These lock in six real fixes made while landing M1 (2026-07-23): commit-the-
//! adjudicated-set, stage-auto-opens-changeset, untracked-residue tolerance,
//! reset-on-red, and begin-refuses-a-dirty-tracked-tree. Losing any of them would
//! either corrupt a commit's attributable contents or wedge a run permanently.

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
    envelope(
        &[
            "stage",
            "--repo",
            ws.path_str(),
            "--path",
            rel_path,
            "--clearance",
            "genesis",
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

    // The commit's own contents must be exactly the three staged paths.
    let (ok, files) = git(
        ws.path(),
        &["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"],
    );
    assert!(ok, "diff-tree: {files}");
    let mut committed_paths: Vec<&str> = files.lines().collect();
    committed_paths.sort_unstable();
    assert_eq!(
        committed_paths,
        vec!["package.json", "src/index.ts", "tsconfig.json"],
        "the commit must contain exactly the staged set, nothing swept in"
    );

    // The repo's full tracked set is the baseline plus the staged set — NOTES.md
    // is not among them.
    let (ok, tracked) = git(ws.path(), &["ls-files"]);
    assert!(ok, "ls-files: {tracked}");
    let mut tracked_paths: Vec<&str> = tracked.lines().collect();
    tracked_paths.sort_unstable();
    assert_eq!(
        tracked_paths,
        vec![
            ".gitignore",
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

// ---- 4. untracked residue (the real npm-install byproduct) never wedges the next changeset ----

#[test]
fn untracked_residue_after_a_commit_does_not_block_the_next_changeset() {
    let ws = Workspace::new("residue");
    land_green_fixture(&ws, "add the fixture ts project");

    // `npm install`, run inside the trusted build gate during that commit, left a
    // real, gitignored, untracked lockfile in the tree — the exact residue the
    // liveness fix targets.
    assert!(
        ws.path().join("package-lock.json").is_file(),
        "npm install should have produced a lockfile as a byproduct of the build"
    );
    let (ok, tracked) = git(ws.path(), &["ls-files"]);
    assert!(ok, "ls-files: {tracked}");
    assert!(
        !tracked.lines().any(|f| f == "package-lock.json"),
        "the lockfile must stay untracked (it is gitignored, not staged): {tracked:?}"
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
