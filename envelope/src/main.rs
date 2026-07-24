//! Envelope — a runnable skeleton of the trusted core.
//!
//! An untrusted agent proposes a batch of changes; the reference monitor decides
//! the fate of each one deterministically. The point: not a single
//! outcome depends on the agent being well-behaved. Trust comes from the
//! boundary, not the brain.

mod agent;
mod decision_log;
mod design;
mod guardrails;
mod harness;
mod invariants;
mod policy;
mod reversible;
mod runtime;
mod telemetry;
mod types;
mod verifier;
mod worktree;

use harness::Harness;
use invariants::reach::Clearance;
use telemetry::StubTelemetry;
use types::{Action, Outcome, Verdict, Verification};
use verifier::StubVerifier;
use worktree::{BuildVerifier, Disposition, Establish};

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::process::ExitCode;

/// Aggregate result of running a batch of proposals through the harness.
struct Summary {
    committed: u32,
    rejected: u32,
    denials: BTreeMap<&'static str, u32>,
    reverted: Vec<String>,
}

impl Summary {
    fn rolled_back(&self) -> usize {
        self.reverted.len()
    }
}

/// Build the harness wired to the showcase's trusted sources. Shared by `main` and
/// the end-to-end test so they exercise exactly the same configuration.
fn showcase_harness() -> Harness {
    // Trusted telemetry, seeded out-of-band: a stand-in for a monitoring system
    // the agent cannot write to.
    let telemetry = StubTelemetry::new()
        .set(
            "admin-dashboard",
            vec![("error_rate", 0.004), ("task_completion", 0.94)],
        )
        .set(
            "admin-onboarding",
            vec![("error_rate", 0.006), ("task_completion", 0.71)],
        );

    // Trusted verifier, seeded out-of-band: a stand-in for CI plus agentic UI
    // verification. The agent cannot self-certify.
    let verifier = StubVerifier::new()
        .set(
            "admin-dashboard",
            Verification {
                typecheck: true,
                tests: true,
                ui_verified: true,
            },
        )
        .set(
            "admin-reports",
            Verification {
                typecheck: true,
                tests: true,
                ui_verified: false,
            },
        )
        .set(
            "admin-onboarding",
            Verification {
                typecheck: true,
                tests: true,
                ui_verified: true,
            },
        );

    Harness::new(Box::new(telemetry), Box::new(verifier))
}

/// Run every proposal through the harness, returning the aggregate verdicts.
/// Reads the structured `Outcome` return values — proof they are machine-usable,
/// not merely lines in a log.
fn run(harness: &mut Harness) -> Summary {
    let mut summary = Summary {
        committed: 0,
        rejected: 0,
        denials: BTreeMap::new(),
        reverted: vec![],
    };

    for (intent, action) in agent::proposals() {
        println!("──────────────────────────────────────────────────────────");
        match harness.enact(&intent, action) {
            Outcome::Committed => summary.committed += 1,
            Outcome::Rejected(violations) => {
                summary.rejected += 1;
                for v in &violations {
                    *summary.denials.entry(v.invariant).or_insert(0) += 1;
                }
            }
            Outcome::RolledBack { breached } => summary.reverted.push(breached),
        }
    }

    summary
}

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
        // Pure-transitive dependency maintenance: envelope-computed lockfile
        // refresh, no `package.json` change (ADR 0009).
        Some("refresh-deps") => cmd_refresh_deps(&args[2..]),
        // Abandon an open changeset: reset the tree to the clean baseline.
        Some("reset") => cmd_reset(&args[2..]),
        // The runtime envelope's fast loop (ADR 0011): read trusted telemetry,
        // revert the deployed change on an error-rate breach. No advisor input.
        Some("monitor") => cmd_monitor(&args[2..]),
        Some(other) => {
            eprintln!("unknown subcommand `{other}`; run with no arguments for the showcase");
            ExitCode::from(2)
        }
        None => {
            run_showcase();
            ExitCode::SUCCESS
        }
    }
}

/// The real adjudication path: the trusted core acting on the live outcome
/// working tree on behalf of the untrusted brain.
///
/// Channel discipline IS the trust boundary. The brain can express only a write —
/// `--repo`, `--path`, `--intent`, with the file body on stdin. The verdict is
/// decided by the same pure policy kernel the showcase uses, then enacted, verified by
/// the repo's own build, and committed or reverted. The brain supplies none of
/// the things it is judged by; it cannot even name them here.
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

    // Same pure kernel as the showcase: deny-by-default reach plus immutable-policy.
    let action = Action::WriteFile {
        path: path.clone(),
        bytes: content.len(),
    };
    if let Verdict::Deny(violations) = policy::Policy::reference_monitor().evaluate(&action) {
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
/// The clearance is chosen by lifecycle stage (Genesis vs Maintenance), supplied
/// by the caller — never by the agent, whose write is what is being judged here.
fn cmd_stage(args: &[String]) -> ExitCode {
    let mut flags = Flags::default();
    if let Err(code) = flags.parse(args) {
        return code;
    }
    let (Some(repo_raw), Some(path)) = (flags.repo, flags.path) else {
        emit_error("usage: envelope stage --repo <dir> --path <repo-relative> --clearance <genesis|maintenance>  (file body on stdin)");
        return ExitCode::from(2);
    };
    let clearance = match flags.clearance.as_deref() {
        Some("genesis") => Clearance::Genesis,
        Some("maintenance") | None => Clearance::Maintenance,
        Some(other) => {
            emit_error(&format!(
                "unknown clearance `{other}` (genesis|maintenance)"
            ));
            return ExitCode::from(2);
        }
    };
    let mut content = Vec::new();
    if let Err(e) = std::io::stdin().read_to_end(&mut content) {
        emit_error(&format!("could not read file body from stdin: {e}"));
        return ExitCode::from(2);
    }

    // Reach (and immutable-policy) is a PURE decision — it does not depend on the
    // repo existing, so it is made before resolving the path. A denial means the
    // write never touches the tree (and the seam can be probed without a workspace).
    let action = Action::WriteFile {
        path: path.clone(),
        bytes: content.len(),
    };
    if let Verdict::Deny(violations) = policy::Policy::for_clearance(clearance).evaluate(&action) {
        let pairs: Vec<(&'static str, String)> = violations
            .into_iter()
            .map(|v| (v.invariant, v.reason))
            .collect();
        emit_rejected(&pairs);
        return ExitCode::SUCCESS;
    }

    let repo = match worktree::resolve_repo(&repo_raw) {
        Ok(p) => p,
        Err(e) => {
            emit_error(&format!("repo `{repo_raw}` not found: {e}"));
            return ExitCode::from(2);
        }
    };
    emit_disposition(&path, &worktree::stage(&repo, &path, &content));
    ExitCode::SUCCESS
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
    clearance: Option<String>,
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
                "--clearance" => self.clearance = args.get(i + 1).cloned(),
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

fn run_showcase() {
    println!("ENVELOPE — trusted core showcase");
    println!("The agent is untrusted. Every proposal passes through the reference");
    println!("monitor, which produces each verdict below deterministically.\n");

    let mut harness = showcase_harness();
    let summary = run(&mut harness);

    let world = harness.world();
    println!("══════════════════════════════════════════════════════════");
    println!(
        "SUMMARY   committed={}  rejected={}  rolled_back={}",
        summary.committed,
        summary.rejected,
        summary.rolled_back()
    );
    println!(
        "WORLD     files={}  deployed={}",
        world.file_count(),
        world.deploy_count()
    );
    if !summary.denials.is_empty() {
        let breakdown: Vec<String> = summary
            .denials
            .iter()
            .map(|(invariant, n)| format!("{invariant}={n}"))
            .collect();
        println!("DENIALS   {}", breakdown.join("  "));
    }
    if !summary.reverted.is_empty() {
        println!("REVERTED  {}", summary.reverted.join(", "));
    }

    // Persist the append-only audit trail. An accountability surface has to be
    // durable and re-readable, not merely streamed to stdout.
    let trail = harness.log().entries();
    let audit_path = "target/envelope-audit.log";
    match fs::write(audit_path, format!("{}\n", trail.join("\n"))) {
        Ok(()) => println!(
            "AUDIT     {} entries persisted to {audit_path}",
            trail.len()
        ),
        Err(e) => println!("AUDIT     could not persist trail: {e}"),
    }

    println!("\nNone of these outcomes required the agent to be trustworthy.");
    println!("The envelope produced them by construction.");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Locks in the showcase's headline result: the exact verdicts the batch
    /// depends on. If a future change alters the boundary's behaviour, this fails
    /// rather than the showcase silently telling a different story.
    #[test]
    fn showcase_batch_produces_expected_verdicts() {
        let mut harness = showcase_harness();
        let summary = run(&mut harness);

        assert_eq!(summary.committed, 3);
        assert_eq!(summary.rejected, 4);
        assert_eq!(summary.rolled_back(), 1);
        assert_eq!(summary.reverted, vec!["task_completion".to_string()]);
        assert_eq!(summary.denials.get("reach"), Some(&2));
        assert_eq!(summary.denials.get("change_shape"), Some(&1));
        assert_eq!(summary.denials.get("immutable_policy"), Some(&1));

        assert_eq!(harness.world().file_count(), 2);
        assert_eq!(harness.world().deploy_count(), 1);
    }
}
