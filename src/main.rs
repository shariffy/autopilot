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

use harness::Harness;
use telemetry::StubTelemetry;
use types::{Outcome, Verification};
use verifier::StubVerifier;

use std::collections::BTreeMap;
use std::fs;

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

fn main() {
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
