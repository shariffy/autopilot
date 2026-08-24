//! Core domain types for the Envelope trusted core.
//!
//! These are deliberately small sum types: every action the untrusted agent
//! can propose, and every verdict the reference monitor can produce. Because
//! `match` over them is exhaustive, "forgot to handle a case" is a compile
//! error rather than a runtime hole — which is exactly the property you want
//! in the code that bounds an autonomous agent.

/// Everything the untrusted agent is *able to ask for*. If it isn't a variant
/// here, the agent cannot even express it.
#[derive(Debug, Clone)]
pub enum Action {
    /// Write source into the governed application repository. The action
    /// carries a path and nothing else: reach decides WHERE the agent may
    /// write, never what it writes (see `invariants/reach.rs`'s module doc).
    /// Content-level rules are a separate stage over the staged bytes
    /// (`design.rs`), deliberately not expressible here.
    WriteFile { path: String },
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
