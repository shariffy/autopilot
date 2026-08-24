//! The policy engine: the reference monitor's fixed rule set.
//!
//! This is the small, static, human-auditable surface where trust actually
//! lives. It is deterministic — the same action always yields the same verdict —
//! and the agent can neither reach it nor change it.
//!
//! The rule applied here is the pure, action-only reach invariant
//! (`invariants::reach`), decided entirely from the proposed action with no
//! I/O.

use crate::invariants::reach::Clearance;
use crate::types::{Action, Verdict, Violation};

pub struct Policy {
    clearance: Clearance,
}

impl Policy {
    /// Construct the policy for a given reach clearance. To audit the action-only
    /// rules, read this struct plus the invariant modules it names; verification
    /// and the commit/revert gate live in `worktree.rs`.
    pub fn for_clearance(clearance: Clearance) -> Self {
        Policy { clearance }
    }

    /// Evaluate every action-only invariant and aggregate their findings. Deny if
    /// any rule is violated; otherwise allow. The rule is the reach clearance
    /// currently in force.
    pub fn evaluate(&self, action: &Action) -> Verdict {
        let violations: Vec<Violation> = self.clearance.check(action);
        if violations.is_empty() {
            Verdict::Allow
        } else {
            Verdict::Deny(violations)
        }
    }
}
