//! The real, git-backed effector: how the trusted core actually touches the
//! governed repository.
//!
//! The in-memory harness (`main`, `reversible::World`) proves the *shape* of the
//! guarantees in-memory. This module is where they become real: proposed writes
//! are applied to a working tree on disk, verified by the repository's own build,
//! and then committed or reverted — using git itself as the reversibility
//! substrate. Nothing here is trusted to the agent; it is the trusted core doing
//! I/O on the agent's behalf.
//!
//! Adjudication is over a **changeset**, not a single write (ADR 0005): reach is
//! enforced on every write (by the caller, via the shared [`crate::policy`]
//! kernel), but verification and reversibility are applied once, at the changeset
//! boundary — because "fit to ship" is a property of a coherent set of changes,
//! not of each keystroke. The lifecycle is begin → stage* → commit. A single
//! maintenance edit is the one-stage special case ([`adjudicate_write`]).
//!
//! State lives in git, not here: nothing is committed until the changeset closes,
//! so `HEAD` is always the baseline. On green the whole staged tree is committed;
//! on red it is reset back to `HEAD`. The trusted core keeps no cross-call state.

use std::path::{Path, PathBuf};
use std::process::Command;

/// What actually happened, mirroring [`crate::types::Outcome`] but carrying the
/// real-world evidence (a commit hash, a build-failure tail) the caller needs.
pub enum Disposition {
    /// A workspace baseline was established (an empty init, or a clone whose own
    /// build was confirmed green). `HEAD` is now a clean baseline to build on.
    Established { detail: String },
    /// A changeset was opened: the work tree is clean, so `HEAD` is a sound
    /// baseline to revert to.
    Begun,
    /// A proposed write was applied to the tree (reach already cleared). Not yet
    /// verified or committed.
    Staged { path: String },
    /// The changeset's writes passed the repo build and were committed atomically.
    Committed { commit: String },
    /// The changeset failed the repo build. The staged tree is LEFT in place so the
    /// agent can fix and re-commit cheaply; nothing committed, so `HEAD` is still
    /// the clean baseline. (The one-shot path resets instead — see `RolledBack`.)
    BuildFailed { detail: String },
    /// A single maintenance write failed the build and was reset away, leaving a
    /// clean tree. The one-stage analogue of `BuildFailed`.
    RolledBack { detail: String },
    /// A precondition was not met (e.g. dirty tree at begin, nothing to commit, an
    /// I/O failure). Fails closed: nothing landed.
    Refused { reason: String },
}

/// A trusted verifier backed by the governed repo's *own* build — the honest "is
/// this fit to ship" gate. Derived from the outcome (ADR 0005): it detects the
/// package manager from the lockfile and runs that build, installing dependencies
/// first only when they are absent (as in a fresh Genesis workspace). The agent
/// cannot influence which command runs.
pub struct BuildVerifier;

impl BuildVerifier {
    /// The verifier for an outcome: its own build, decided from its files.
    pub fn repo_build() -> Self {
        BuildVerifier
    }

    /// The package manager this outcome uses, inferred from its lockfile.
    fn package_manager(repo: &Path) -> &'static str {
        if repo.join("yarn.lock").exists() {
            "yarn"
        } else if repo.join("pnpm-lock.yaml").exists() {
            "pnpm"
        } else {
            "npm"
        }
    }

    /// Dependencies are installed only when missing — present in a maintained app,
    /// absent in a freshly created one.
    fn needs_install(repo: &Path) -> bool {
        !repo.join("node_modules").exists()
    }

    /// `npm`/`pnpm` need `run` before a script; `yarn build` is direct.
    fn build_args(pm: &str) -> &'static [&'static str] {
        if pm == "yarn" {
            &["build"]
        } else {
            &["run", "build"]
        }
    }

    fn run(&self, repo: &Path) -> (bool, String) {
        let pm = Self::package_manager(repo);
        let mut log = String::new();

        if Self::needs_install(repo) {
            let (ok, out) = run_in(repo, pm, &["install"]);
            log.push_str(&out);
            if !ok {
                return (false, log);
            }
        }

        let (ok, out) = run_in(repo, pm, Self::build_args(pm));
        log.push_str(&out);
        (ok, log)
    }

    fn describe(&self, repo: &Path) -> String {
        let pm = Self::package_manager(repo);
        let build = format!("{pm} {}", Self::build_args(pm).join(" "));
        if Self::needs_install(repo) {
            format!("{pm} install && {build}")
        } else {
            build
        }
    }
}

/// Open a changeset: require a clean work tree so the baseline (`HEAD`) is
/// well-defined and a later revert is exact, then record the open changeset so
/// `stage` and `commit` can refuse to act outside one. Fails closed.
pub fn begin(repo: &Path) -> Disposition {
    if let Err(reason) = ensure_clean(repo) {
        return Disposition::Refused { reason };
    }
    match open_changeset(repo) {
        Ok(()) => Disposition::Begun,
        Err(reason) => Disposition::Refused { reason },
    }
}

/// Stage one proposed write into the open changeset. Reach is the caller's
/// responsibility (decided by the shared policy kernel before we are called); here
/// we only apply the bytes and record the path as part of this changeset. The tree
/// is expected to be mid-changeset (dirty), so no clean check — that was asserted
/// at `begin`.
///
/// Staging with no changeset open opens one, which asserts the clean baseline
/// exactly as `begin` does — so a caller that stages straight into a fresh tree
/// still gets a well-defined baseline, and one that stages onto residue is still
/// refused. `begin` stays available to open a changeset explicitly. What is never
/// optional is the assertion itself: no changeset exists without a clean baseline
/// behind it, so a revert is always well-defined.
pub fn stage(repo: &Path, rel_path: &str, content: &[u8]) -> Disposition {
    if !changeset_is_open(repo) {
        if let Err(reason) = ensure_clean(repo) {
            return Disposition::Refused { reason };
        }
        if let Err(reason) = open_changeset(repo) {
            return Disposition::Refused { reason };
        }
    }
    if let Err(reason) = write_file(repo, rel_path, content) {
        return Disposition::Refused { reason };
    }
    if let Err(reason) = record_staged(repo, rel_path) {
        return Disposition::Refused { reason };
    }
    Disposition::Staged {
        path: rel_path.to_string(),
    }
}

/// Close the changeset: verify the accumulated tree with the repo's own build,
/// then commit everything atomically on green or reset to the baseline on red.
pub fn commit(repo: &Path, intent: &str, verifier: &BuildVerifier) -> Disposition {
    // Nothing staged ⇒ nothing to adjudicate. Refuse rather than make an empty
    // commit, so a no-op is visible rather than silently "successful".
    if !changeset_is_open(repo) {
        return Disposition::Refused {
            reason: "no staged changes to commit".to_string(),
        };
    }
    let staged = match staged_paths(repo) {
        Ok(paths) => paths,
        Err(reason) => return Disposition::Refused { reason },
    };
    if staged.is_empty() {
        return Disposition::Refused {
            reason: "no staged changes to commit".to_string(),
        };
    }

    // Verify with the repository's own build — the trusted gate.
    let (passed, output) = verifier.run(repo);
    if !passed {
        // Leave the staged tree as-is: nothing is committed, so `HEAD` is still
        // the clean baseline, and the agent can fix the offending file and retry
        // without re-staging the whole changeset. Cleanup is `reset`, on abort.
        return Disposition::BuildFailed {
            detail: tail(&output, 1600),
        };
    }

    // Commit exactly what this changeset staged — never `add -A`. The commit's
    // contents must equal the adjudicated set, or the attributable-changeset claim
    // is empty: anything else in the tree (a leftover from an earlier changeset
    // whose build went red) would ride along under an intent that never covered it
    // and a verification that never judged it.
    let message = format!("{intent}\n\n[envelope] verified by `{}`", verifier.describe(repo));
    let mut add = vec!["add", "--"];
    add.extend(staged.iter().map(String::as_str));
    if !git(repo, &add).0 {
        reset(repo);
        return Disposition::Refused {
            reason: "could not stage the changeset for commit".to_string(),
        };
    }
    if !git(repo, &["commit", "--quiet", "-m", &message]).0 {
        reset(repo);
        return Disposition::Refused {
            reason: "could not commit the verified changeset".to_string(),
        };
    }
    let commit = git(repo, &["rev-parse", "--short", "HEAD"])
        .1
        .trim()
        .to_string();
    close_changeset(repo);
    Disposition::Committed { commit }
}

/// Adjudicate a single proposed write: the one-stage changeset. Kept as a
/// convenience for maintenance edits and the dry-run seam — equivalent to
/// begin → stage one file → commit, with reach decided by the caller.
pub fn adjudicate_write(
    repo: &Path,
    rel_path: &str,
    content: &[u8],
    intent: &str,
    verifier: &BuildVerifier,
) -> Disposition {
    if let Disposition::Refused { reason } = begin(repo) {
        return Disposition::Refused { reason };
    }
    if let Disposition::Refused { reason } = stage(repo, rel_path, content) {
        return Disposition::Refused { reason };
    }
    // A one-shot edit owns its whole changeset, so a red build resets to a clean
    // tree (RolledBack) rather than leaving work for a follow-up the caller of a
    // single write will never make.
    match commit(repo, intent, verifier) {
        Disposition::BuildFailed { detail } => {
            reset(repo);
            Disposition::RolledBack { detail }
        }
        other => other,
    }
}

/// Establish a fresh workspace baseline from the agent's chosen starting-point
/// (ADR 0005). The agent never does this itself — it is trusted setup, so the
/// constraints (an empty target; a clone whose own build is green before adoption)
/// are enforced here, not left to the brain.
pub enum Establish {
    /// Greenfield: an empty repo with one clean commit.
    Empty,
    /// Adopt a predecessor: clone it, then confirm its own build is green before it
    /// becomes the baseline. A read-only `source` is never mutated.
    Clone { source: PathBuf },
}

pub fn establish(workspace: &Path, mode: Establish, verifier: &BuildVerifier) -> Disposition {
    if is_nonempty_dir(workspace) {
        return Disposition::Refused {
            reason: format!(
                "`{}` already exists and is not empty; establish needs a fresh target",
                workspace.display()
            ),
        };
    }
    match mode {
        Establish::Empty => establish_empty(workspace),
        Establish::Clone { source } => establish_clone(workspace, &source, verifier),
    }
}

/// Build products and machine-local noise: never part of a changeset, so the
/// commit stays the adjudicated set and nothing else.
///
/// Lockfiles are here because the *verifier* writes them, not the agent: `install`
/// runs inside the trusted gate, after staging has closed. An unignored lockfile is
/// therefore residue the agent never proposed and cannot stage — and it would leave
/// the tree dirty, so the next `begin` would refuse and the run would wedge after a
/// single changeset. Ignored, they still sit on disk for package-manager detection.
const BASELINE_IGNORES: &str = concat!(
    "node_modules/\n",
    "dist/\n",
    "build/\n",
    "package-lock.json\n",
    "pnpm-lock.yaml\n",
    "yarn.lock\n",
    ".DS_Store\n",
    "*.log\n",
);

fn establish_empty(workspace: &Path) -> Disposition {
    if let Err(e) = std::fs::create_dir_all(workspace) {
        return Disposition::Refused {
            reason: format!("could not create workspace: {e}"),
        };
    }
    if !git(workspace, &["init", "--quiet"]).0 {
        return Disposition::Refused {
            reason: "could not initialise git in the workspace".to_string(),
        };
    }
    // Baseline ignores, committed so the baseline tree is clean. Without these the
    // verifier's own `install` leaves build products in the tree, where they would
    // be indistinguishable from the agent's work — and `clean -fd` on a red build
    // would delete a dependency tree the agent never wrote and cannot restore.
    if let Err(reason) = write_file(workspace, ".gitignore", BASELINE_IGNORES.as_bytes()) {
        return Disposition::Refused { reason };
    }
    if !git(workspace, &["add", "--", ".gitignore"]).0 {
        return Disposition::Refused {
            reason: "could not stage the baseline ignores".to_string(),
        };
    }
    if !git(workspace, &["commit", "--quiet", "-m", "baseline"]).0 {
        return Disposition::Refused {
            reason: "could not create the baseline commit (is git user.name/email set?)".to_string(),
        };
    }
    Disposition::Established {
        detail: "empty workspace initialised".to_string(),
    }
}

fn establish_clone(workspace: &Path, source: &Path, verifier: &BuildVerifier) -> Disposition {
    let src = source.to_string_lossy();
    let dst = workspace.to_string_lossy();
    // `git clone` reads the source and writes only the new workspace; the source
    // working tree is never touched.
    let (ok, out) = run_in(Path::new("."), "git", &["clone", "--quiet", &src, &dst]);
    if !ok {
        return Disposition::Refused {
            reason: format!("could not clone `{src}`: {}", tail(&out, 400)),
        };
    }
    // Adoption precondition: the predecessor must build green as-is, or reverting a
    // future change has no sound baseline to return to. Verify before adopting.
    let (passed, output) = verifier.run(workspace);
    if !passed {
        let _ = std::fs::remove_dir_all(workspace);
        return Disposition::Refused {
            reason: format!(
                "predecessor does not build green at baseline; not adopted:\n{}",
                tail(&output, 1200)
            ),
        };
    }
    Disposition::Established {
        detail: format!("adopted clone of `{src}`; baseline build green"),
    }
}

/// True if `path` exists and contains entries.
fn is_nonempty_dir(path: &Path) -> bool {
    std::fs::read_dir(path)
        .map(|mut d| d.next().is_some())
        .unwrap_or(false)
}

/// Require a git work tree with no uncommitted changes.
fn ensure_clean(repo: &Path) -> Result<(), String> {
    if !git(repo, &["rev-parse", "--is-inside-work-tree"]).0 {
        return Err(format!("`{}` is not a git work tree", repo.display()));
    }
    let (ok, status) = git(repo, &["status", "--porcelain"]);
    if !ok {
        return Err("could not read git status".to_string());
    }
    if !status.trim().is_empty() {
        return Err("working tree is not clean; refusing so a revert stays well-defined".to_string());
    }
    Ok(())
}

/// Where the open changeset is recorded: inside `.git`, which is a never-writable
/// zone for the agent (see `invariants::reach`). The brain therefore cannot open a
/// changeset, forge its membership, or close one — it can only ask, and be judged.
fn changeset_marker(repo: &Path) -> PathBuf {
    repo.join(".git").join("envelope-changeset")
}

fn changeset_is_open(repo: &Path) -> bool {
    changeset_marker(repo).exists()
}

/// Record an open changeset with, as yet, no members.
fn open_changeset(repo: &Path) -> Result<(), String> {
    std::fs::write(changeset_marker(repo), b"")
        .map_err(|e| format!("could not open the changeset: {e}"))
}

fn close_changeset(repo: &Path) {
    let _ = std::fs::remove_file(changeset_marker(repo));
}

/// Add a path to the open changeset's membership. Re-staging the same path (a
/// fix-and-retry after a red build) must not duplicate it.
fn record_staged(repo: &Path, rel_path: &str) -> Result<(), String> {
    let mut paths = staged_paths(repo)?;
    if !paths.iter().any(|p| p == rel_path) {
        paths.push(rel_path.to_string());
    }
    std::fs::write(changeset_marker(repo), paths.join("\n"))
        .map_err(|e| format!("could not record `{rel_path}` in the changeset: {e}"))
}

/// The paths staged into the open changeset, in staging order.
fn staged_paths(repo: &Path) -> Result<Vec<String>, String> {
    let raw = std::fs::read_to_string(changeset_marker(repo))
        .map_err(|e| format!("could not read the open changeset: {e}"))?;
    Ok(raw
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect())
}

/// Apply bytes to the tree, creating parent directories as needed.
fn write_file(repo: &Path, rel_path: &str, content: &[u8]) -> Result<(), String> {
    let abs = repo.join(rel_path);
    if let Some(parent) = abs.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not create parent dirs for `{rel_path}`: {e}"))?;
    }
    std::fs::write(&abs, content).map_err(|e| format!("could not write `{rel_path}`: {e}"))
}

/// Restore the tree to the changeset baseline (`HEAD`): drop staged modifications
/// and any newly created files. Well-defined because the baseline was clean and
/// nothing is committed until the changeset closes. Used to abandon a changeset
/// (an abort, or a one-shot edit that failed the build).
pub fn reset(repo: &Path) {
    let _ = git(repo, &["reset", "--hard", "HEAD"]);
    // `clean -fd` without `-x`: ignored paths (a maintained `node_modules/`) survive,
    // so abandoning a changeset costs a rebuild but never a reinstall.
    let _ = git(repo, &["clean", "-fd"]);
    close_changeset(repo);
}

/// Run a git subcommand in `repo`, returning (success, combined stdout+stderr).
fn git(repo: &Path, args: &[&str]) -> (bool, String) {
    run_in(repo, "git", args)
}

/// Run an arbitrary command in `repo`, returning (success, combined output).
fn run_in(repo: &Path, program: &str, args: &[&str]) -> (bool, String) {
    match Command::new(program).args(args).current_dir(repo).output() {
        Ok(out) => {
            let mut combined = String::from_utf8_lossy(&out.stdout).into_owned();
            combined.push_str(&String::from_utf8_lossy(&out.stderr));
            (out.status.success(), combined)
        }
        Err(e) => (false, format!("could not run `{program}`: {e}")),
    }
}

/// Keep only the last `max` bytes of build output, on a char boundary, so a
/// rollback reason is informative without dumping the whole log.
fn tail(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut start = s.len() - max;
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    format!("…{}", &s[start..])
}

/// Resolve a user-supplied repo path to an absolute path, canonicalising the
/// existing prefix. Use for repos that must already exist.
pub fn resolve_repo(raw: &str) -> std::io::Result<PathBuf> {
    std::fs::canonicalize(raw)
}

/// Resolve a workspace path that may not exist yet (no canonicalisation), making
/// it absolute against the current directory if it is relative. Use for the
/// target of `establish`.
pub fn resolve_new_repo(raw: &str) -> PathBuf {
    let p = PathBuf::from(raw);
    if p.is_absolute() {
        p
    } else {
        std::env::current_dir()
            .map(|c| c.join(&p))
            .unwrap_or(p)
    }
}
