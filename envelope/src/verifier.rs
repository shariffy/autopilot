//! Verification: the trusted source of truth for whether a change is fit to ship.
//!
//! The verification gate reads its evidence from here, never from the agent. The
//! agent cannot run its own verification and report a pass, and it cannot even
//! express a verification result in its proposal — so it cannot self-certify.
//! That separation is the point: the entity being gated does not get to grade its
//! own homework.

use crate::types::Verification;
use std::collections::HashMap;

pub trait Verifier {
    /// The trusted verification result for a service's pending change — the
    /// output of CI (typecheck, tests) and agentic UI verification.
    fn verify(&self, service: &str) -> Verification;
}

/// A stand-in for the in-memory verification pipeline (CI + agentic UI
/// verification — the real gate is `worktree::BuildVerifier`'s e2e stage, ADR
/// 0013). Seeded out-of-band; the agent has no handle to it. An unseeded service
/// verifies as `Verification::default()` — all false — so the gate fails closed.
#[derive(Default)]
pub struct StubVerifier {
    results: HashMap<String, Verification>,
}

impl StubVerifier {
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed the verification result for a service (builder style).
    pub fn set(mut self, service: &str, verification: Verification) -> Self {
        self.results.insert(service.to_string(), verification);
        self
    }
}

impl Verifier for StubVerifier {
    fn verify(&self, service: &str) -> Verification {
        self.results.get(service).cloned().unwrap_or_default()
    }
}
