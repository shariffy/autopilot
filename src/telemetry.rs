//! Telemetry: the trusted source of truth for what actually happened in
//! production after a rollout.
//!
//! The outcome gate reads observed metrics from here, never from the agent. The
//! agent cannot write to telemetry and cannot even express a metric in its
//! proposal — so it cannot fake the numbers it will be judged by. That
//! separation is the entire fix: the entity being gated does not get to report
//! its own grades.

use std::collections::HashMap;

pub trait Telemetry {
    /// Observed metric readings for a service after its canary rollout.
    fn observe(&self, service: &str) -> Vec<(String, f64)>;
}

/// A stand-in for a real monitoring system (Prometheus, Datadog, ...). Seeded
/// out-of-band with the readings the canary really produced; the agent has no
/// handle to it.
#[derive(Default)]
pub struct StubTelemetry {
    readings: HashMap<String, Vec<(String, f64)>>,
}

impl StubTelemetry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed the readings for a service (builder style).
    pub fn set(mut self, service: &str, readings: Vec<(&str, f64)>) -> Self {
        let owned = readings
            .into_iter()
            .map(|(m, v)| (m.to_string(), v))
            .collect();
        self.readings.insert(service.to_string(), owned);
        self
    }
}

impl Telemetry for StubTelemetry {
    fn observe(&self, service: &str) -> Vec<(String, f64)> {
        self.readings.get(service).cloned().unwrap_or_default()
    }
}
