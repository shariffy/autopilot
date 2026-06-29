//! The real, git-backed effector: how the trusted core actually touches the
//! governed repository.
//!
//! The conceptual demo (`main`, `reversible::World`) proves the *shape* of the
//! guarantees in-memory. This module is where they become real: a proposed write
//! is applied to a working tree on disk, verified by the repository's own build,
//! and then either committed or reverted — using git itself as the reversibility
//! substrate. Nothing here is trusted to the agent; it is the trusted core doing
//! I/O on the agent's behalf.
//!
//! The reach decision is NOT re-implemented here. It comes from the same pure
//! [`crate::policy::Policy`] the conceptual demo uses, so there is exactly one
//! place where "where may the agent write" is decided.

use std::path::{Path, PathBuf};
use std::process::Command;

/// What actually happened to a single proposed write, mirroring
/// [`crate::types::Outcome`] but carrying the real-world evidence (a commit hash,
/// a build-failure tail) the caller needs.
pub enum Disposition {
    /// Written, verified by the repo build, and committed.
    Committed { commit: String },
    /// Written, failed the repo build, reverted; the tree is clean again.
    RolledBack { detail: String },
    /// A precondition for safe adjudication was not met (e.g. dirty tree).
    /// Fails closed: nothing was written.
    Refused { reason: String },
}

/// A trusted verifier backed by a real command run inside the governed repo.
/// Defaults to the repository's own `npm run build` — the honest "is this fit to
/// ship" gate. The agent cannot influence which command runs.
pub struct BuildVerifier {
    program: String,
    args: Vec<String>,
}

impl BuildVerifier {
    pub fn npm_build() -> Self {
        BuildVerifier {
            program: "npm".to_string(),
            args: vec!["run".to_string(), "build".to_string()],
        }
    }

    fn run(&self, repo: &Path) -> (bool, String) {
        match Command::new(&self.program)
            .args(&self.args)
            .current_dir(repo)
            .output()
        {
            Ok(out) => {
                let mut combined = String::from_utf8_lossy(&out.stdout).into_owned();
                combined.push_str(&String::from_utf8_lossy(&out.stderr));
                (out.status.success(), combined)
            }
            Err(e) => (
                false,
                format!("could not run verifier `{}`: {e}", self.program),
            ),
        }
    }

    fn describe(&self) -> String {
        format!("{} {}", self.program, self.args.join(" "))
    }
}

/// Adjudicate a single proposed write against the real repository.
///
/// `policy_decision` is the verdict from the shared trust kernel; passing it in
/// (rather than computing it here) keeps reach decisions in one auditable place.
/// `existed`/`tracked` are not the caller's concern — this fn derives everything
/// it needs from git so reverting is exact.
pub fn adjudicate_write(
    repo: &Path,
    rel_path: &str,
    content: &[u8],
    intent: &str,
    verifier: &BuildVerifier,
) -> Disposition {
    // Precondition: a git work tree we can revert against.
    if !git(repo, &["rev-parse", "--is-inside-work-tree"]).0 {
        return Disposition::Refused {
            reason: format!("`{}` is not a git work tree", repo.display()),
        };
    }

    // Precondition: clean tree. Reversibility is only well-defined if our single
    // write is the only change present. Fails closed.
    let (ok, status) = git(repo, &["status", "--porcelain"]);
    if !ok {
        return Disposition::Refused {
            reason: "could not read git status".to_string(),
        };
    }
    if !status.trim().is_empty() {
        return Disposition::Refused {
            reason: "working tree is not clean; refusing so a revert stays well-defined"
                .to_string(),
        };
    }

    // Was the target already tracked? Determines how we revert: restore a tracked
    // file, or delete a newly-created one.
    let tracked = git(repo, &["ls-files", "--error-unmatch", rel_path]).0;

    // Apply the write to the real tree.
    let abs = repo.join(rel_path);
    if let Some(parent) = abs.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return Disposition::Refused {
                reason: format!("could not create parent dirs for `{rel_path}`: {e}"),
            };
        }
    }
    if let Err(e) = std::fs::write(&abs, content) {
        return Disposition::Refused {
            reason: format!("could not write `{rel_path}`: {e}"),
        };
    }

    // Verify with the repository's own build — the trusted gate.
    let (passed, output) = verifier.run(repo);
    if !passed {
        revert(repo, rel_path, tracked, &abs);
        return Disposition::RolledBack {
            detail: tail(&output, 1600),
        };
    }

    // Commit the verified change. Each landed change is one attributable commit.
    let message = format!(
        "{intent}\n\n[envelope] verified by `{}`",
        verifier.describe()
    );
    if !git(repo, &["add", "--", rel_path]).0 {
        revert(repo, rel_path, tracked, &abs);
        return Disposition::Refused {
            reason: format!("could not stage `{rel_path}`"),
        };
    }
    if !git(repo, &["commit", "--quiet", "-m", &message]).0 {
        revert(repo, rel_path, tracked, &abs);
        return Disposition::Refused {
            reason: "could not commit the verified change".to_string(),
        };
    }
    let commit = git(repo, &["rev-parse", "--short", "HEAD"])
        .1
        .trim()
        .to_string();
    Disposition::Committed { commit }
}

/// Restore the tree to the pre-write state: drop a new file, or restore a tracked
/// one. Scoped to the single path we touched, since we required a clean tree.
fn revert(repo: &Path, rel_path: &str, was_tracked: bool, abs: &Path) {
    if was_tracked {
        // Restore the tracked file's committed contents.
        let _ = git(repo, &["checkout", "--", rel_path]);
    } else {
        // Newly created — remove it.
        let _ = std::fs::remove_file(abs);
    }
}

/// Run a git subcommand in `repo`, returning (success, combined stdout+stderr).
fn git(repo: &Path, args: &[&str]) -> (bool, String) {
    match Command::new("git").args(args).current_dir(repo).output() {
        Ok(out) => {
            let mut combined = String::from_utf8_lossy(&out.stdout).into_owned();
            combined.push_str(&String::from_utf8_lossy(&out.stderr));
            (out.status.success(), combined)
        }
        Err(e) => (false, format!("could not run git: {e}")),
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

/// Resolve a user-supplied repo path to an absolute path without requiring it to
/// exist yet beyond canonicalization of its existing prefix.
pub fn resolve_repo(raw: &str) -> std::io::Result<PathBuf> {
    std::fs::canonicalize(raw)
}
