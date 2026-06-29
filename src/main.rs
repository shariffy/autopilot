//! Envelope — a runnable skeleton of the trusted core.
//!
//! An untrusted agent proposes a batch of changes; the reference monitor decides
//! the fate of each one deterministically. The point of the demo: not a single
//! outcome depends on the agent being well-behaved. Trust comes from the
//! boundary, not the brain.

mod agent;
mod decision_log;
mod guardrails;
mod harness;
mod invariants;
mod policy;
mod reversible;
mod telemetry;
mod types;
mod verifier;
mod worktree;

use harness::Harness;
use telemetry::StubTelemetry;
use types::{Action, Outcome, Verdict, Verification};
use verifier::StubVerifier;
use worktree::{BuildVerifier, Disposition};

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

/// Build the harness wired to the demo's trusted sources. Shared by `main` and
/// the end-to-end test so they exercise exactly the same configuration.
fn demo_harness() -> Harness {
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
        Some("adjudicate") => adjudicate(&args[2..]),
        Some(other) => {
            eprintln!("unknown subcommand `{other}`; run with no arguments for the demo");
            ExitCode::from(2)
        }
        None => {
            run_demo();
            ExitCode::SUCCESS
        }
    }
}

/// The real adjudication path: the trusted core acting on the live `roli-admin`
/// working tree on behalf of the untrusted brain.
///
/// Channel discipline IS the trust boundary. The brain can express only a write —
/// `--repo`, `--path`, `--intent`, with the file body on stdin. The verdict is
/// decided by the same pure policy kernel the demo uses, then enacted, verified by
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

    // Same pure kernel as the demo: deny-by-default reach plus immutable-policy.
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

    let disposition =
        worktree::adjudicate_write(&repo, &path, &content, &intent, &BuildVerifier::npm_build());
    emit_disposition(&path, &disposition);
    ExitCode::SUCCESS
}

// ---- machine-readable result on stdout (one JSON object per adjudication) ----

fn emit_disposition(path: &str, d: &Disposition) {
    match d {
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

fn run_demo() {
    println!("ENVELOPE — trusted core demo");
    println!("The agent is untrusted. Every proposal passes through the reference");
    println!("monitor, which produces each verdict below deterministically.\n");

    let mut harness = demo_harness();
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

    /// Locks in the demo's headline result: the exact verdicts the showcase
    /// depends on. If a future change alters the boundary's behaviour, this fails
    /// rather than the demo silently telling a different story.
    #[test]
    fn demo_batch_produces_expected_verdicts() {
        let mut harness = demo_harness();
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
