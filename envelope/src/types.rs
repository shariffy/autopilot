//! Core domain types for the Envelope trusted core.
//!
//! These are deliberately small sum types: every action the untrusted agent
//! can propose, and every verdict/outcome the trusted harness can produce.
//! Because `match` over them is exhaustive, "forgot to handle a case" is a
//! compile error rather than a runtime hole — which is exactly the property
//! you want in the code that bounds an autonomous agent.

/// Everything the untrusted agent is *able to ask for*. If it isn't a variant
/// here, the agent cannot even express it.
#[derive(Debug, Clone)]
pub enum Action {
    /// Write source into the governed application repository.
    WriteFile { path: String, bytes: usize },

    /// Ship a service at a given canary traffic share. It deliberately carries
    /// NO verification evidence, observed metrics, or guardrails: the agent
    /// cannot supply, weaken, or fake any of the things it will be judged by.
    /// Verification comes from the trusted verifier; health from trusted
    /// telemetry and guardrail policy.
    Deploy { service: String, traffic_pct: u8 },

    /// Attempt to alter the rules themselves. Exists so the showcase can show it is
    /// *structurally* refused — the agent can never widen its own envelope. It
    /// carries no payload: the request is denied without being inspected.
    ModifyPolicy,
}

/// Evidence that a change is fit to ship: typecheck, tests, and UI verification.
/// Produced by the trusted verifier, never self-reported by the agent. The
/// `Default` (all false) means "no evidence", which the change-shape gate treats
/// as not fit to ship — i.e. it fails closed.
#[derive(Debug, Clone, Default)]
pub struct Verification {
    pub typecheck: bool,
    pub tests: bool,
    pub ui_verified: bool,
}

#[derive(Debug, Clone)]
pub struct Guardrail {
    pub metric: String,
    pub limit: Limit,
}

#[derive(Debug, Clone)]
pub enum Limit {
    /// Observed value must not exceed this (e.g. error rate).
    Max(f64),
    /// Observed value must not fall below this (e.g. task completion).
    Min(f64),
}

impl Guardrail {
    pub fn breached_by(&self, value: f64) -> bool {
        match self.limit {
            Limit::Max(m) => value > m,
            Limit::Min(m) => value < m,
        }
    }
}

/// A single rule violation produced by an invariant.
#[derive(Debug, Clone)]
pub struct Violation {
    pub invariant: &'static str,
    pub reason: String,
}

/// The deterministic verdict of the policy engine.
#[derive(Debug, Clone)]
pub enum Verdict {
    Allow,
    Deny(Vec<Violation>),
}

/// Final disposition of a proposed action after passing through the harness.
#[derive(Debug)]
pub enum Outcome {
    /// Refused by policy; never touched the world.
    Rejected(Vec<Violation>),
    /// Enacted and kept (guardrails held).
    Committed,
    /// Enacted, breached a guardrail, automatically reverted.
    RolledBack { breached: String },
}
