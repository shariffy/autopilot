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

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The identity stamped as **author** on a changeset commit and as **author
/// and committer** on the `establish` baseline: `(name, email)`. The advisor
/// proposed the change; it never runs `git` itself and holds no git identity of
/// its own — this is a fixed, hermetic label for "what the untrusted brain
/// proposed", not a real account (ADR 0010).
const ADVISOR_IDENT: (&str, &str) = ("Autopilot advisor", "advisor@autopilot.invalid");

/// The identity stamped as **committer** on every commit the envelope makes, and
/// as **author** too wherever the envelope itself is the sole author of the work
/// (the `establish` baseline — trusted setup, not the advisor's proposal). The
/// trusted core commits everything; this says so on every commit, hermetically,
/// regardless of the host's ambient git config (ADR 0010).
const ENVELOPE_IDENT: (&str, &str) = ("Autopilot envelope", "envelope@autopilot.invalid");

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
///
/// As of ADR 0009 it is also where dependency maintenance happens: the agent
/// proposes intent (`package.json`); this is the ONLY place a lockfile is
/// computed, resolved before install, installed strictly (never mutated) before
/// build, and checked for newly introduced advisories after build. Resolution,
/// install, and audit are npm-specific — they compute `package-lock.json`, the
/// one lockfile the envelope authors regardless of which manager the outcome's
/// own build script uses; the build step itself keeps the existing multi-manager
/// detection.
pub struct BuildVerifier {
    /// The audit gate's summary from the most recently completed `run`, read by
    /// `describe` for the commit message. `commit` always calls the two as a
    /// pair, in that order, on the same instance — simpler than threading an
    /// extra return value through every call site.
    last_audit: RefCell<Option<String>>,
}

impl BuildVerifier {
    /// The verifier for an outcome: its own build, decided from its files.
    pub fn repo_build() -> Self {
        BuildVerifier {
            last_audit: RefCell::new(None),
        }
    }

    /// The package manager the outcome's *build script* runs under, inferred
    /// from its lockfile. Dependency resolution/install/audit (below) are npm's
    /// job regardless — this is only about which command runs `build`.
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

    /// True when this changeset must resolve dependencies before installing:
    /// its `package.json` intent changed this changeset, or there is no
    /// lockfile at all yet (the first genesis changeset). Resolution is the
    /// only path that ever writes `package-lock.json` — the agent cannot
    /// (`invariants::reach`).
    fn needs_resolve(repo: &Path, staged: &[String]) -> bool {
        staged.iter().any(|p| p == "package.json") || !repo.join("package-lock.json").exists()
    }

    /// True when the on-disk lockfile differs from the one committed at `HEAD`
    /// (or exists where `HEAD` had none) — the trigger for a strict `npm ci`
    /// reinstall, so a resolved-but-not-yet-installed change is never verified
    /// against a stale `node_modules`.
    fn lockfile_changed_since_head(repo: &Path) -> bool {
        let current = std::fs::read(repo.join("package-lock.json")).ok();
        let (ok, head) = git(repo, &["show", "HEAD:package-lock.json"]);
        if !ok {
            return current.is_some();
        }
        current.as_deref() != Some(head.as_bytes())
    }

    /// Resolve → install → build → audit, once per changeset (ADR 0005, ADR
    /// 0009). `staged` is the changeset's own membership so far — resolution
    /// consults it (was `package.json` part of this changeset's intent?) and,
    /// on success, extends it: the envelope-computed lockfile becomes an
    /// explicitly envelope-authored member of THIS changeset, so `commit`'s
    /// `git add -- <staged>` includes it and it lands in the same commit as the
    /// manifest change that produced it.
    fn run(&self, repo: &Path, staged: &[String]) -> (bool, String) {
        let mut log = String::new();

        // 1. Resolution: the agent's `package.json` is intent, never bytes on
        // the wire — only the trusted core computes the lockfile from it
        // (ADR 0009). `--package-lock-only` touches nothing but the lockfile
        // (no `node_modules`); `--ignore-scripts` means no dependency
        // lifecycle script runs here, before the agent's write has even
        // passed the build gate below.
        if Self::needs_resolve(repo, staged) {
            let (ok, out) = run_in(
                repo,
                "npm",
                &["install", "--package-lock-only", "--ignore-scripts"],
            );
            log.push_str(&out);
            if !ok {
                return (false, log);
            }
            let _ = record_staged(repo, "package-lock.json");
        }

        // 2. Install strictly from the lockfile: `npm ci` never mutates it, so
        // a green build here can never itself be the source of drift the next
        // changeset would have to explain.
        if Self::needs_install(repo) || Self::lockfile_changed_since_head(repo) {
            let (ok, out) = run_in(repo, "npm", &["ci", "--ignore-scripts"]);
            log.push_str(&out);
            if !ok {
                return (false, log);
            }
        }

        // 3. Build: unchanged multi-manager detection/command.
        let pm = Self::package_manager(repo);
        let (ok, out) = run_in(repo, pm, Self::build_args(pm));
        log.push_str(&out);
        if !ok {
            return (false, log);
        }

        // 4. Audit: a non-regression gate, not a zero-vulns gate. Pre-existing
        // findings (already present at `HEAD`) never block — the demo already
        // carries some; only an advisory THIS changeset newly introduces does.
        let current = Self::advisory_ids(repo);
        let Some(baseline) = Self::baseline_advisory_ids(repo) else {
            // Nothing established yet: this changeset *is* the baseline, so there
            // is no regression to detect. Blocking here would make the envelope
            // unable to bring any real app into existence — every mainstream
            // stack carries some transitive advisory on the day it is installed,
            // and refusing that is not a security property, just an inability to
            // start. Genesis is bounded instead by a disposable workspace, atomic
            // reversibility, and the human launch gate (ADR 0005); the findings
            // are recorded here so that review sees them.
            let summary = format!(
                "npm audit: {} advisories (baseline established by this changeset)",
                current.len()
            );
            log.push_str(&format!("\n{summary}\n"));
            *self.last_audit.borrow_mut() = Some(summary);
            return (true, log);
        };
        let introduced: BTreeSet<i64> = current.difference(&baseline).copied().collect();
        let summary = format!(
            "npm audit: {} advisories ({} pre-existing, {} newly introduced{})",
            current.len(),
            current.intersection(&baseline).count(),
            introduced.len(),
            if introduced.is_empty() {
                String::new()
            } else {
                format!(
                    ": {}",
                    introduced
                        .iter()
                        .map(i64::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        );
        *self.last_audit.borrow_mut() = Some(summary.clone());
        if !introduced.is_empty() {
            log.push_str(&format!("\n{summary}\n"));
            return (false, log);
        }

        (true, log)
    }

    /// Advisory IDs `npm audit` reports against the tree as it stands right now
    /// (post-install, post-build).
    fn advisory_ids(repo: &Path) -> BTreeSet<i64> {
        run_capturing_stdout(repo, "npm", &["audit", "--json"])
            .map(|s| parse_advisory_ids(&s))
            .unwrap_or_default()
    }

    /// Advisory IDs at `HEAD`: extract `HEAD:package.json` (and the lockfile,
    /// if any) into a scratch dir and audit there — cheap
    /// (`--package-lock-only`, no install, no network beyond the advisory
    /// lookup itself). No manifest at `HEAD` means the first genesis
    /// changeset: nothing existed yet to have pre-existing findings, so the
    /// baseline is empty.
    fn baseline_advisory_ids(repo: &Path) -> Option<BTreeSet<i64>> {
        let (ok, manifest) = git(repo, &["show", "HEAD:package.json"]);
        if !ok {
            // No manifest at `HEAD`: nothing has been established yet, so there is
            // no baseline to regress *from*. Distinct from an established baseline
            // that happens to be clean — see the gate in `run`.
            return None;
        }
        let tmp = std::env::temp_dir().join(format!(
            "envelope-audit-baseline-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        if std::fs::create_dir_all(&tmp).is_err() {
            return None;
        }
        let _ = std::fs::write(tmp.join("package.json"), manifest);
        let (lock_ok, lockfile) = git(repo, &["show", "HEAD:package-lock.json"]);
        if lock_ok {
            let _ = std::fs::write(tmp.join("package-lock.json"), lockfile);
        }
        let ids = run_capturing_stdout(&tmp, "npm", &["audit", "--json", "--package-lock-only"])
            .map(|s| parse_advisory_ids(&s))
            .unwrap_or_default();
        let _ = std::fs::remove_dir_all(&tmp);
        Some(ids)
    }

    fn describe(&self, repo: &Path) -> String {
        let pm = Self::package_manager(repo);
        let mut desc = format!(
            "npm install --package-lock-only --ignore-scripts && npm ci --ignore-scripts && {pm} {} && npm audit --json",
            Self::build_args(pm).join(" ")
        );
        if let Some(summary) = self.last_audit.borrow().as_ref() {
            desc.push_str(&format!(" — {summary}"));
        }
        desc
    }
}

/// Run a command, returning only its stdout, ignoring the exit status. `npm
/// audit` exits non-zero whenever it finds ANY advisory — an ordinary result
/// here, not a run failure; the JSON on stdout is what the audit gate judges.
fn run_capturing_stdout(repo: &Path, program: &str, args: &[&str]) -> Option<String> {
    Command::new(program)
        .args(args)
        .current_dir(repo)
        .output()
        .ok()
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Extract advisory IDs from `npm audit --json` output: the numeric `"source"`
/// field npm assigns each finding inside a `vulnerabilities.*.via[]` entry (it
/// resolves to a GHSA advisory URL). A targeted string scan, not a JSON parser
/// — the crate is deliberately zero-dependency (`Cargo.toml`), and the one field
/// the audit gate needs is stable enough that scanning for it is simpler, and no
/// less correct, than hand-rolling a parser for a document we otherwise never
/// look at.
fn parse_advisory_ids(json: &str) -> BTreeSet<i64> {
    const KEY: &str = "\"source\"";
    let mut ids = BTreeSet::new();
    let mut rest = json;
    while let Some(idx) = rest.find(KEY) {
        rest = &rest[idx + KEY.len()..];
        // Skip the `:` and any whitespace npm's pretty-printed JSON puts before
        // the value (`npm audit --json` is NOT compact — there is a space after
        // the colon), so this parses both spacings identically.
        let digits: String = rest
            .trim_start_matches(|c: char| c == ':' || c.is_whitespace())
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        if let Ok(id) = digits.parse() {
            ids.insert(id);
        }
    }
    ids
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

    // Verify with the repository's own build — the trusted gate. Resolution
    // (inside `run`) may itself extend the changeset's membership with an
    // envelope-computed lockfile (ADR 0009), so `staged` is re-read below
    // rather than reused from above.
    let (passed, output) = verifier.run(repo, &staged);
    if !passed {
        // Leave the staged tree as-is: nothing is committed, so `HEAD` is still
        // the clean baseline, and the agent can fix the offending file and retry
        // without re-staging the whole changeset. Cleanup is `reset`, on abort.
        return Disposition::BuildFailed {
            detail: tail(&output, 1600),
        };
    }
    let staged = match staged_paths(repo) {
        Ok(paths) => paths,
        Err(reason) => return Disposition::Refused { reason },
    };

    // Commit exactly what this changeset staged — never `add -A`. The commit's
    // contents must equal the adjudicated set, or the attributable-changeset claim
    // is empty: anything else in the tree (a leftover from an earlier changeset
    // whose build went red) would ride along under an intent that never covered it
    // and a verification that never judged it. (The lockfile above is not an
    // exception: it is a member the ENVELOPE recorded, adjudicated by the same
    // build+audit gate as everything else in this commit.)
    let message = format!(
        "{intent}\n\n[envelope] verified by `{}`",
        verifier.describe(repo)
    );
    let mut add = vec!["add", "--"];
    add.extend(staged.iter().map(String::as_str));
    if !git(repo, &add).0 {
        reset(repo);
        return Disposition::Refused {
            reason: "could not stage the changeset for commit".to_string(),
        };
    }
    // Stamped identity, not the host's ambient git config (ADR 0010): the advisor
    // proposed this changeset, the envelope committed it on green, and every
    // changeset commit says exactly that regardless of what `git config` happens
    // to hold on the machine running the envelope.
    if !git_commit_as(
        repo,
        &["commit", "--quiet", "-m", &message],
        ADVISOR_IDENT,
        ENVELOPE_IDENT,
    )
    .0
    {
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

/// The pure-transitive dependency-maintenance operation (ADR 0009): re-resolve
/// the lockfile (or, with `audit_fix`, run `npm audit fix --package-lock-only`
/// — a fix entirely within the ranges `package.json` already allows) with no
/// `package.json` change at all, and fold the result into the open changeset.
/// Still trusted-core compute, never the agent's — `invariants::reach` refuses
/// an agent-authored lockfile under any clearance regardless of how this
/// function is invoked.
///
/// Stages the lockfile only; it does not itself verify or commit. The caller
/// (`commit`) still runs the full resolve→install→build→audit gate over the
/// result, exactly as for any other staged change — this just computes the one
/// artifact the agent cannot author itself and records it as a changeset member.
///
/// Opens a changeset if none is open yet (mirrors `stage`'s auto-open), so a
/// pure lockfile refresh can be the first and only operation of its changeset.
pub fn refresh_dependencies(repo: &Path, audit_fix: bool) -> Disposition {
    if !changeset_is_open(repo) {
        if let Err(reason) = ensure_clean(repo) {
            return Disposition::Refused { reason };
        }
        if let Err(reason) = open_changeset(repo) {
            return Disposition::Refused { reason };
        }
    }

    let args: &[&str] = if audit_fix {
        &["audit", "fix", "--package-lock-only", "--ignore-scripts"]
    } else {
        &["install", "--package-lock-only", "--ignore-scripts"]
    };
    let (ok, out) = run_in(repo, "npm", args);
    if !ok {
        return Disposition::Refused {
            reason: format!("dependency refresh failed: {}", tail(&out, 800)),
        };
    }

    if let Err(reason) = record_staged(repo, "package-lock.json") {
        return Disposition::Refused { reason };
    }

    Disposition::Staged {
        path: "package-lock.json".to_string(),
    }
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
/// `package-lock.json` is deliberately NOT here (ADR 0009): it is now tracked.
/// The trusted core, not the agent, computes it — from the agent's `package.json`
/// intent, during verification — and folds it into the changeset that produced
/// it (`BuildVerifier::run` records it as a changeset member; `reach.rs` refuses
/// the agent's own write of one under any clearance). A tracked, envelope-authored
/// lockfile belongs in history like any other envelope-authored artifact.
/// `node_modules/`, build output, and machine-local files remain ignored: they
/// are reproducible from the lockfile and would only ever be residue.
const BASELINE_IGNORES: &str = concat!(
    "node_modules/\n",
    "dist/\n",
    "build/\n",
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
    // Trusted setup, not the advisor's work (ADR 0010): author and committer are
    // both the envelope. Stamped explicitly, so this never depends on whether the
    // host running the envelope happens to have a `git config user.name/email`.
    if !git_commit_as(
        workspace,
        &["commit", "--quiet", "-m", "baseline"],
        ENVELOPE_IDENT,
        ENVELOPE_IDENT,
    )
    .0
    {
        return Disposition::Refused {
            reason: "could not create the baseline commit".to_string(),
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
    // No changeset is open here (adoption precedes one), so this is an empty
    // staged set — resolution still runs if the predecessor has no lockfile.
    let (passed, output) = verifier.run(workspace, &[]);
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

/// Require a git work tree whose *tracked* content matches `HEAD`, so the baseline
/// a changeset reverts to is exact.
///
/// Untracked files are deliberately tolerated. They cannot enter a commit — `commit`
/// adds only the paths this changeset staged — and `reset` removes them, so they
/// threaten neither the atomicity of a changeset nor the exactness of a revert.
/// Treating them as disqualifying instead cost liveness, and did so in the one case
/// the agent could not escape: verification itself writes untracked build products
/// (a lockfile, caches) into the tree, and once they were there every remedy needed
/// a write, every write needed a clean tree, and the agent had no way to clean one.
/// A run wedged permanently on residue the trusted core had created itself. Sound
/// liveness here also stops correctness from resting on `.gitignore`, which is
/// inside the agent's reach and which it will rewrite for its own stack.
fn ensure_clean(repo: &Path) -> Result<(), String> {
    if !git(repo, &["rev-parse", "--is-inside-work-tree"]).0 {
        return Err(format!("`{}` is not a git work tree", repo.display()));
    }
    let (ok, status) = git(repo, &["status", "--porcelain", "--untracked-files=no"]);
    if !ok {
        return Err("could not read git status".to_string());
    }
    if !status.trim().is_empty() {
        return Err(
            "tracked files differ from HEAD; refusing so a revert stays well-defined".to_string(),
        );
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

/// Run a git subcommand in `repo` with an explicit author/committer identity
/// (ADR 0010), each a `(name, email)` pair. Sets `GIT_AUTHOR_NAME/EMAIL` and
/// `GIT_COMMITTER_NAME/EMAIL` on the child process — `Command::env` always wins
/// over whatever the parent process inherited or the host's git config holds, so
/// the identity on a commit the envelope makes is never the ambient operator's,
/// hermetically, regardless of what runs the envelope. Used only for the git
/// subcommands that actually create a commit; every other git call keeps using
/// `git`, which carries no opinion about identity because it never needs one.
fn git_commit_as(
    repo: &Path,
    args: &[&str],
    author: (&str, &str),
    committer: (&str, &str),
) -> (bool, String) {
    match Command::new("git")
        .args(args)
        .current_dir(repo)
        .env("GIT_AUTHOR_NAME", author.0)
        .env("GIT_AUTHOR_EMAIL", author.1)
        .env("GIT_COMMITTER_NAME", committer.0)
        .env("GIT_COMMITTER_EMAIL", committer.1)
        .output()
    {
        Ok(out) => {
            let mut combined = String::from_utf8_lossy(&out.stdout).into_owned();
            combined.push_str(&String::from_utf8_lossy(&out.stderr));
            (out.status.success(), combined)
        }
        Err(e) => (false, format!("could not run `git`: {e}")),
    }
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
        std::env::current_dir().map(|c| c.join(&p)).unwrap_or(p)
    }
}

#[cfg(test)]
mod audit_parsing_tests {
    use super::parse_advisory_ids;

    #[test]
    fn extracts_every_source_id_in_document_order_deduplicated() {
        let json = r#"{"vulnerabilities":{"minimatch":{"via":[
            {"source":1093710,"severity":"high"},
            {"source":1096485,"severity":"high"},
            {"source":1093710,"severity":"high"}
        ]}}}"#;
        let ids: Vec<i64> = parse_advisory_ids(json).into_iter().collect();
        assert_eq!(ids, vec![1093710, 1096485]);
    }

    #[test]
    fn extracts_ids_from_npms_actual_pretty_printed_spacing() {
        // `npm audit --json` is NOT compact JSON — it puts a space after every
        // `:`. This is the exact shape that broke a first version of the parser.
        let json = "{\n  \"source\": 1093710,\n  \"severity\": \"high\"\n}";
        let ids: Vec<i64> = parse_advisory_ids(json).into_iter().collect();
        assert_eq!(ids, vec![1093710]);
    }

    #[test]
    fn no_findings_is_an_empty_set() {
        let json = r#"{"vulnerabilities":{},"metadata":{"vulnerabilities":{"total":0}}}"#;
        assert!(parse_advisory_ids(json).is_empty());
    }
}
