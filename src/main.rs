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

fn main() {
    println!("ENVELOPE — trusted core demo");
    println!("The agent is untrusted. Every proposal passes through the reference");
    println!("monitor, which produces each verdict below deterministically.\n");

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

    let mut harness = Harness::new(Box::new(telemetry), Box::new(verifier));
    let mut committed = 0;
    let mut rejected = 0;
    // Structured signals read back from the harness's return values — proof the
    // outcomes are machine-usable, not merely lines in a log.
    let mut denials_by_invariant: BTreeMap<&'static str, u32> = BTreeMap::new();
    let mut reverted_metrics: Vec<String> = vec![];

    for (intent, action) in agent::proposals() {
        println!("──────────────────────────────────────────────────────────");
        match harness.enact(&intent, action) {
            Outcome::Committed => committed += 1,
            Outcome::Rejected(violations) => {
                rejected += 1;
                for v in &violations {
                    *denials_by_invariant.entry(v.invariant).or_insert(0) += 1;
                }
            }
            Outcome::RolledBack { breached } => reverted_metrics.push(breached),
        }
    }

    let world = harness.world();
    println!("══════════════════════════════════════════════════════════");
    println!(
        "SUMMARY   committed={committed}  rejected={rejected}  rolled_back={}",
        reverted_metrics.len()
    );
    println!(
        "WORLD     files={}  deployed={}",
        world.file_count(),
        world.deploy_count()
    );
    if !denials_by_invariant.is_empty() {
        let breakdown: Vec<String> = denials_by_invariant
            .iter()
            .map(|(invariant, n)| format!("{invariant}={n}"))
            .collect();
        println!("DENIALS   {}", breakdown.join("  "));
    }
    if !reverted_metrics.is_empty() {
        println!("REVERTED  {}", reverted_metrics.join(", "));
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
