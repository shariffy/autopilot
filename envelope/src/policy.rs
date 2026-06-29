//! The policy engine: the reference monitor's fixed rule set.
//!
//! This is the small, static, human-auditable surface where trust actually
//! lives. It is deterministic — the same action always yields the same verdict —
//! and the agent can neither reach it nor change it.
//!
//! These are the pure, action-only invariants (`reach`, `immutable_policy`). The
//! two checks that need a *trusted external source* — verification (`verifier` +
//! `change_shape`) and health (`telemetry` + `guardrails`) — are applied by the
//! harness, not here, because they cannot be decided from the action alone.

use crate::invariants::{self, Invariant};
use crate::types::{Action, Verdict, Violation};

pub struct Policy {
    invariants: Vec<Invariant>,
}

impl Policy {
    /// Construct the canonical reference monitor. To audit the action-only rules,
    /// read this function plus the invariant modules it names; the verification
    /// and outcome gates live in the harness.
    pub fn reference_monitor() -> Self {
        Policy {
            invariants: vec![immutable_policy, invariants::reach::check],
        }
    }

    /// Evaluate every invariant and aggregate their findings. Deny if any rule
    /// is violated; otherwise allow.
    pub fn evaluate(&self, action: &Action) -> Verdict {
        let mut violations: Vec<Violation> = vec![];
        for invariant in &self.invariants {
            violations.extend(invariant(action));
        }
        if violations.is_empty() {
            Verdict::Allow
        } else {
            Verdict::Deny(violations)
        }
    }
}

/// The agent may never alter the rules that bound it. This asymmetry — the
/// governed cannot rewrite the government — is the foundation of structural
/// trust. Without it, every other invariant is advisory.
fn immutable_policy(action: &Action) -> Vec<Violation> {
    match action {
        Action::ModifyPolicy => vec![Violation {
            invariant: "immutable_policy",
            reason: "the agent cannot modify the policy that governs it".to_string(),
        }],
        _ => vec![],
    }
}
