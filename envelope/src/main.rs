//! Envelope — the trusted core's CLI.
//!
//! Every subcommand here is a real path onto the boundary that bounds an
//! untrusted agent: adjudicating a single write, running the begin/stage/commit
//! changeset lifecycle, establishing a workspace, refreshing dependencies, or
//! monitoring a deployed change's telemetry. Not a single outcome depends on
//! the agent being well-behaved — trust comes from the boundary, not the
//! brain, and this binary is how the boundary is invoked.

mod design;
mod invariants;
mod policy;
mod runtime;
mod types;
mod worktree;

use invariants::reach::Clearance;
use types::{Action, Verdict};
use worktree::{BuildVerifier, Disposition, Establish};

use std::io::Read;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        // One-shot maintenance edit (the one-stage changeset) and the dry-run seam.
        Some("adjudicate") => adjudicate(&args[2..]),
        // Trusted setup: establish a workspace baseline from a starting-point.
        Some("establish") => cmd_establish(&args[2..]),
        // Changeset lifecycle: begin → stage* → commit (ADR 0005).
        Some("begin") => cmd_begin(&args[2..]),
        Some("stage") => cmd_stage(&args[2..]),
        Some("commit") => cmd_commit(&args[2..]),
        // Read or flip a repo's persistent clearance stamp (operator-only write).
        Some("clearance") => cmd_clearance(&args[2..]),
        // Pure-transitive dependency maintenance: envelope-computed lockfile
        // refresh, no `package.json` change (ADR 0009).
        Some("refresh-deps") => cmd_refresh_deps(&args[2..]),
        // Abandon an open changeset: reset the tree to the clean baseline.
        Some("reset") => cmd_reset(&args[2..]),
        // The runtime envelope's fast loop (ADR 0011): read trusted telemetry,
        // revert the deployed change on an error-rate breach. No advisor input.
        Some("monitor") => cmd_monitor(&args[2..]),
        Some(other) => {
            eprintln!("unknown subcommand `{other}`");
            ExitCode::from(2)
        }
        None => {
            eprintln!(
                "usage: envelope <subcommand> [args...]\n\nsubcommands:\n  adjudicate    establish    begin      stage\n  commit        refresh-deps reset      monitor\n  clearance"
            );
            ExitCode::from(2)
        }
    }
}

/// The real adjudication path: the trusted core acting on the live outcome
/// working tree on behalf of the untrusted brain.
///
/// Channel discipline IS the trust boundary. The brain can express only a write —
/// `--repo`, `--path`, `--intent`, with the file body on stdin. The verdict is
/// decided by the same pure policy kernel every subcommand uses, then enacted,
/// verified by the repo's own build, and committed or reverted. The brain
/// supplies none of the things it is judged by; it cannot even name them here.
fn adjudicate(args: &[String]) -> ExitCode {
    let mut repo_raw = None;
    let mut path = None;
    let mut intent = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--repo" => repo_raw = args.get(i + 1).cloned(),
            "--path" => path = args.get(i + 1).cloned(),
            "--intent" => intent = args.get(i + 1).cloned(),
            other => {
                emit_error(&format!("unknown flag `{other}`"));
                return ExitCode::from(2);
            }
        }
        i += 2;
    }

    let (Some(repo_raw), Some(path), Some(intent)) = (repo_raw, path, intent) else {
        emit_error("usage: envelope adjudicate --repo <dir> --path <repo-relative> --intent <text>  (file body on stdin)");
        return ExitCode::from(2);
    };

    let repo = match worktree::resolve_repo(&repo_raw) {
        Ok(p) => p,
        Err(e) => {
            emit_error(&format!("repo `{repo_raw}` not found: {e}"));
            return ExitCode::from(2);
        }
    };

    let mut content = Vec::new();
    if let Err(e) = std::io::stdin().read_to_end(&mut content) {
        emit_error(&format!("could not read file body from stdin: {e}"));
        return ExitCode::from(2);
    }

    // The pure kernel: deny-by-default reach, under the clearance the repo's own
    // persistent stamp names (`worktree::stamped_clearance`) — never a clearance
    // the caller names.
    let action = Action::WriteFile { path: path.clone() };
    let clearance = clearance_from_stamp(&repo);
    if let Verdict::Deny(violations) = policy::Policy::for_clearance(clearance).evaluate(&action) {
        let pairs: Vec<(&'static str, String)> = violations
            .into_iter()
            .map(|v| (v.invariant, v.reason))
            .collect();
        emit_rejected(&pairs);
        return ExitCode::SUCCESS; // a delivered verdict is a successful adjudication
    }

    let disposition = worktree::adjudicate_write(
        &repo,
        &path,
        &content,
        &intent,
        &BuildVerifier::repo_build(),
    );
    emit_disposition(&path, &disposition);
    ExitCode::SUCCESS
}

// ---- changeset lifecycle: begin → stage* → commit (ADR 0005) ----

/// Open a changeset over a repo: assert a clean baseline, fail closed otherwise.
fn cmd_begin(args: &[String]) -> ExitCode {
    let mut flags = Flags::default();
    if let Err(code) = flags.parse(args) {
        return code;
    }
    let Some(repo_raw) = flags.repo else {
        emit_error("usage: envelope begin --repo <dir>");
        return ExitCode::from(2);
    };
    let repo = match worktree::resolve_repo(&repo_raw) {
        Ok(p) => p,
        Err(e) => {
            emit_error(&format!("repo `{repo_raw}` not found: {e}"));
            return ExitCode::from(2);
        }
    };
    emit_disposition("", &worktree::begin(&repo));
    ExitCode::SUCCESS
}

/// Stage one write into the open changeset, under the reach clearance in force.
/// The clearance is a property of the REPO — a persistent stamp at
/// `.git/envelope-clearance` (`worktree::stamped_clearance`), set by `establish`
/// and flipped only by the operator via `envelope clearance --set`. It is never
/// supplied on this command line: the write being judged cannot name the
/// boundary that judges it.
fn cmd_stage(args: &[String]) -> ExitCode {
    let mut flags = Flags::default();
    if let Err(code) = flags.parse(args) {
        return code;
    }
    let (Some(repo_raw), Some(path)) = (flags.repo, flags.path) else {
        emit_error(
            "usage: envelope stage --repo <dir> --path <repo-relative>  (file body on stdin)",
        );
        return ExitCode::from(2);
    };
    let mut content = Vec::new();
    if let Err(e) = std::io::stdin().read_to_end(&mut content) {
        emit_error(&format!("could not read file body from stdin: {e}"));
        return ExitCode::from(2);
    }

    let repo = match worktree::resolve_repo(&repo_raw) {
        Ok(p) => p,
        Err(e) => {
            emit_error(&format!("repo `{repo_raw}` not found: {e}"));
            return ExitCode::from(2);
        }
    };

    // Reach is still a PURE, deny-by-default decision over the action alone —
    // but the clearance it runs under is now a property of the repo (its own
    // persistent stamp), not the command line, so the repo must be resolved
    // FIRST to know which clearance is in force before reach can be evaluated.
    let clearance = clearance_from_stamp(&repo);
    let action = Action::WriteFile { path: path.clone() };
    if let Verdict::Deny(violations) = policy::Policy::for_clearance(clearance).evaluate(&action) {
        let pairs: Vec<(&'static str, String)> = violations
            .into_iter()
            .map(|v| (v.invariant, v.reason))
            .collect();
        emit_rejected(&pairs);
        return ExitCode::SUCCESS;
    }

    emit_disposition(&path, &worktree::stage(&repo, &path, &content));
    ExitCode::SUCCESS
}

/// Read or flip a repo's persistent clearance stamp (`.git/envelope-clearance`).
/// Read-only by default; `--set` is the sole path that writes it, gated on an
/// exact `genesis`/`maintenance` value — deliberately `--set`, not `--clearance`,
/// so that removing `--clearance` from `Flags` makes it an unknown flag
/// everywhere in this binary, including here.
fn cmd_clearance(args: &[String]) -> ExitCode {
    let mut flags = Flags::default();
    if let Err(code) = flags.parse(args) {
        return code;
    }
    let Some(repo_raw) = flags.repo else {
        emit_error("usage: envelope clearance --repo <dir> [--set <genesis|maintenance>]");
        return ExitCode::from(2);
    };
    let repo = match worktree::resolve_repo(&repo_raw) {
        Ok(p) => p,
        Err(e) => {
            emit_error(&format!("repo `{repo_raw}` not found: {e}"));
            return ExitCode::from(2);
        }
    };

    let Some(value) = flags.set else {
        println!(
            "{{\"outcome\":\"clearance\",\"clearance\":{}}}",
            json_str(&worktree::stamped_clearance(&repo))
        );
        return ExitCode::SUCCESS;
    };
    match value.as_str() {
        "genesis" | "maintenance" => {
            if let Err(e) = worktree::set_clearance(&repo, &value) {
                emit_error(&e);
                return ExitCode::from(2);
            }
            println!(
                "{{\"outcome\":\"clearance_set\",\"clearance\":{}}}",
                json_str(&value)
            );
            ExitCode::SUCCESS
        }
        other => {
            emit_error(&format!(
                "unknown clearance `{other}` (genesis|maintenance)"
            ));
            ExitCode::from(2)
        }
    }
}

/// The `Clearance` a repo's stamp names, for the pure policy kernel. Shared by
/// `adjudicate` and `cmd_stage` — `worktree::stamped_clearance` already fails
/// closed to `"maintenance"` on anything but an exact `"genesis"`, so this is
/// just the string-to-enum mapping.
fn clearance_from_stamp(repo: &std::path::Path) -> Clearance {
    if worktree::stamped_clearance(repo) == "genesis" {
        Clearance::Genesis
    } else {
        Clearance::Maintenance
    }
}

/// Close the changeset: verify with the outcome's own build, commit all on green
/// or revert all on red.
fn cmd_commit(args: &[String]) -> ExitCode {
    let mut flags = Flags::default();
    if let Err(code) = flags.parse(args) {
        return code;
    }
    let (Some(repo_raw), Some(intent)) = (flags.repo, flags.intent) else {
        emit_error("usage: envelope commit --repo <dir> --intent <text>");
        return ExitCode::from(2);
    };
    let repo = match worktree::resolve_repo(&repo_raw) {
        Ok(p) => p,
        Err(e) => {
            emit_error(&format!("repo `{repo_raw}` not found: {e}"));
            return ExitCode::from(2);
        }
    };
    emit_disposition(
        "",
        &worktree::commit(&repo, &intent, &BuildVerifier::repo_build()),
    );
    ExitCode::SUCCESS
}

/// Pure-transitive dependency maintenance (ADR 0009): re-resolve the lockfile
/// (or, with `--audit-fix`, apply `npm audit fix` within existing ranges) with
/// no `package.json` change, and fold the result into the open changeset.
/// Still trusted-core compute — the agent proposes the operation; it never
/// authors the lockfile itself (`invariants::reach`).
fn cmd_refresh_deps(args: &[String]) -> ExitCode {
    let mut flags = Flags::default();
    if let Err(code) = flags.parse(args) {
        return code;
    }
    let Some(repo_raw) = flags.repo else {
        emit_error("usage: envelope refresh-deps --repo <dir> [--audit-fix]");
        return ExitCode::from(2);
    };
    let repo = match worktree::resolve_repo(&repo_raw) {
        Ok(p) => p,
        Err(e) => {
            emit_error(&format!("repo `{repo_raw}` not found: {e}"));
            return ExitCode::from(2);
        }
    };
    emit_disposition("", &worktree::refresh_dependencies(&repo, flags.audit_fix));
    ExitCode::SUCCESS
}

/// Establish a fresh workspace baseline from the agent's chosen starting-point.
fn cmd_establish(args: &[String]) -> ExitCode {
    let mut flags = Flags::default();
    if let Err(code) = flags.parse(args) {
        return code;
    }
    let Some(repo_raw) = flags.repo else {
        emit_error(
            "usage: envelope establish --repo <workspace> --mode <empty|clone> [--source <dir>]",
        );
        return ExitCode::from(2);
    };
    let workspace = worktree::resolve_new_repo(&repo_raw);

    let mode = match flags.mode.as_deref() {
        Some("empty") | None => Establish::Empty,
        Some("clone") => {
            let Some(source_raw) = flags.source else {
                emit_error("clone mode needs --source <dir>");
                return ExitCode::from(2);
            };
            match worktree::resolve_repo(&source_raw) {
                Ok(source) => Establish::Clone { source },
                Err(e) => {
                    emit_error(&format!("source `{source_raw}` not found: {e}"));
                    return ExitCode::from(2);
                }
            }
        }
        Some(other) => {
            emit_error(&format!("unknown mode `{other}` (empty|clone)"));
            return ExitCode::from(2);
        }
    };

    emit_disposition(
        "",
        &worktree::establish(&workspace, mode, &BuildVerifier::repo_build()),
    );
    ExitCode::SUCCESS
}

/// Abandon an open changeset: reset the work tree to the clean baseline.
fn cmd_reset(args: &[String]) -> ExitCode {
    let mut flags = Flags::default();
    if let Err(code) = flags.parse(args) {
        return code;
    }
    let Some(repo_raw) = flags.repo else {
        emit_error("usage: envelope reset --repo <dir>");
        return ExitCode::from(2);
    };
    let repo = match worktree::resolve_repo(&repo_raw) {
        Ok(p) => p,
        Err(e) => {
            emit_error(&format!("repo `{repo_raw}` not found: {e}"));
            return ExitCode::from(2);
        }
    };
    worktree::reset(&repo);
    println!("{{\"outcome\":\"reset\"}}");
    ExitCode::SUCCESS
}

/// The runtime envelope's fast loop (ADR 0011): read trusted telemetry for the
/// currently-deployed change and, on an error-rate breach, revert it —
/// stamped as the envelope's own action. Takes NO advisor input: there is no
/// `--intent`, no proposal, nothing supplied by the agent — only a repo and a
/// telemetry file the agent does not control.
fn cmd_monitor(args: &[String]) -> ExitCode {
    let mut flags = Flags::default();
    if let Err(code) = flags.parse(args) {
        return code;
    }
    let (Some(repo_raw), Some(telemetry_raw)) = (flags.repo, flags.telemetry) else {
        emit_error("usage: envelope monitor --repo <dir> --telemetry <file> [--threshold <float>]");
        return ExitCode::from(2);
    };
    let repo = match worktree::resolve_repo(&repo_raw) {
        Ok(p) => p,
        Err(e) => {
            emit_error(&format!("repo `{repo_raw}` not found: {e}"));
            return ExitCode::from(2);
        }
    };
    let threshold = match flags.threshold {
        Some(raw) => match raw.parse::<f64>() {
            Ok(v) => Some(v),
            Err(_) => {
                emit_error(&format!("`--threshold {raw}` is not a valid number"));
                return ExitCode::from(2);
            }
        },
        None => None,
    };

    let disposition = runtime::monitor(&repo, std::path::Path::new(&telemetry_raw), threshold);
    emit_monitor_disposition(&disposition);
    ExitCode::SUCCESS
}

/// Flags shared by the changeset commands. Each command validates which it needs.
#[derive(Default)]
struct Flags {
    repo: Option<String>,
    path: Option<String>,
    intent: Option<String>,
    /// `clearance` only: the value to stamp with `--set`.
    set: Option<String>,
    mode: Option<String>,
    source: Option<String>,
    /// `refresh-deps` only: `npm audit fix` within existing ranges, rather than
    /// a plain re-resolve. A bare flag (no value), so it is parsed separately
    /// from the `--flag value` pairs below.
    audit_fix: bool,
    /// `monitor` only: path to the trusted telemetry file.
    telemetry: Option<String>,
    /// `monitor` only: override the telemetry file's own `threshold`.
    threshold: Option<String>,
}

impl Flags {
    fn parse(&mut self, args: &[String]) -> Result<(), ExitCode> {
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "--audit-fix" => {
                    self.audit_fix = true;
                    i += 1;
                    continue;
                }
                "--repo" => self.repo = args.get(i + 1).cloned(),
                "--path" => self.path = args.get(i + 1).cloned(),
                "--intent" => self.intent = args.get(i + 1).cloned(),
                "--set" => self.set = args.get(i + 1).cloned(),
                "--mode" => self.mode = args.get(i + 1).cloned(),
                "--source" => self.source = args.get(i + 1).cloned(),
                "--telemetry" => self.telemetry = args.get(i + 1).cloned(),
                "--threshold" => self.threshold = args.get(i + 1).cloned(),
                other => {
                    emit_error(&format!("unknown flag `{other}`"));
                    return Err(ExitCode::from(2));
                }
            }
            i += 2;
        }
        Ok(())
    }
}

// ---- machine-readable result on stdout (one JSON object per adjudication) ----

fn emit_disposition(path: &str, d: &Disposition) {
    match d {
        Disposition::Established { detail } => println!(
            "{{\"outcome\":\"established\",\"detail\":{}}}",
            json_str(detail)
        ),
        Disposition::Begun => println!("{{\"outcome\":\"begun\"}}"),
        Disposition::Staged { path } => {
            println!("{{\"outcome\":\"staged\",\"path\":{}}}", json_str(path))
        }
        Disposition::BuildFailed { detail } => println!(
            "{{\"outcome\":\"build_failed\",\"path\":{},\"reason\":\"verification failed\",\"detail\":{}}}",
            json_str(path),
            json_str(detail)
        ),
        Disposition::Committed { commit } => println!(
            "{{\"outcome\":\"committed\",\"path\":{},\"commit\":{}}}",
            json_str(path),
            json_str(commit)
        ),
        Disposition::RolledBack { detail } => println!(
            "{{\"outcome\":\"rolled_back\",\"path\":{},\"reason\":\"verification failed\",\"detail\":{}}}",
            json_str(path),
            json_str(detail)
        ),
        Disposition::Refused { reason } => println!(
            "{{\"outcome\":\"refused\",\"path\":{},\"reason\":{}}}",
            json_str(path),
            json_str(reason)
        ),
    }
}

fn emit_monitor_disposition(d: &runtime::Disposition) {
    match d {
        runtime::Disposition::Tripped {
            reverted_to,
            detail,
        } => println!(
            "{{\"outcome\":\"tripped\",\"reverted_to\":{},\"detail\":{}}}",
            json_str(reverted_to),
            json_str(detail)
        ),
        runtime::Disposition::Nominal { detail } => println!(
            "{{\"outcome\":\"nominal\",\"detail\":{}}}",
            json_str(detail)
        ),
        runtime::Disposition::Refused { reason } => println!(
            "{{\"outcome\":\"refused\",\"reason\":{}}}",
            json_str(reason)
        ),
    }
}

fn emit_rejected(violations: &[(&'static str, String)]) {
    let items: Vec<String> = violations
        .iter()
        .map(|(inv, reason)| {
            format!(
                "{{\"invariant\":{},\"reason\":{}}}",
                json_str(inv),
                json_str(reason)
            )
        })
        .collect();
    println!(
        "{{\"outcome\":\"rejected\",\"violations\":[{}]}}",
        items.join(",")
    );
}

fn emit_error(reason: &str) {
    println!("{{\"outcome\":\"error\",\"reason\":{}}}", json_str(reason));
}

/// Minimal RFC 8259 string escaping — enough for paths, intents, and build tails.
/// Kept here rather than pulling in a serializer; the trusted core stays zero-dep.
fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
