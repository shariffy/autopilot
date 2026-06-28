//! Change-shape gate: nothing user-visible ships without full verification.
//!
//! The harness applies this to the result of the TRUSTED verifier — CI plus
//! agentic UI verification — not to anything the agent claims about its own work.
//! The agent cannot self-certify: `Verification` is produced by the verifier and
//! never appears in the agent's proposal.

use crate::types::{Verification, Violation};

pub fn check(service: &str, verification: &Verification) -> Vec<Violation> {
    let mut violations = vec![];
    if !verification.typecheck {
        violations.push(viol(service, "typecheck did not pass"));
    }
    if !verification.tests {
        violations.push(viol(service, "tests did not pass"));
    }
    if !verification.ui_verified {
        violations.push(viol(service, "UI was not verified"));
    }
    violations
}

fn viol(service: &str, why: &str) -> Violation {
    Violation {
        invariant: "change_shape",
        reason: format!("deploy of `{service}` blocked: {why}"),
    }
}
