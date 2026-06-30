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

use crate::invariants::reach::Charter;
use crate::types::{Action, Verdict, Violation};

pub struct Policy {
    charter: Charter,
}

impl Policy {
    /// Construct the policy for a given reach charter. To audit the action-only
    /// rules, read this struct plus the invariant modules it names; the
    /// verification and outcome gates live in the harness.
    pub fn for_charter(charter: Charter) -> Self {
        Policy { charter }
    }

    /// The canonical reference monitor for an app under maintenance — the narrow,
    /// fitted charter that the structural-trust thesis is about. Genesis-stage
    /// adjudication is constructed explicitly with [`Policy::for_charter`].
    pub fn reference_monitor() -> Self {
        Policy::for_charter(Charter::Maintenance)
    }

    /// Evaluate every action-only invariant and aggregate their findings. Deny if
    /// any rule is violated; otherwise allow. The rules are the immutable-policy
    /// guarantee and the reach charter currently in force.
    pub fn evaluate(&self, action: &Action) -> Verdict {
        let mut violations: Vec<Violation> = immutable_policy(action);
        violations.extend(self.charter.check(action));
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
