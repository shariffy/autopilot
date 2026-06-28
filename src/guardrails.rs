//! The outcome gate's policy.
//!
//! Guardrails are TRUSTED configuration, authored once by a human. The agent
//! cannot supply or weaken them, and the values they are checked against come
//! from trusted telemetry — so a change cannot be kept unless production itself,
//! as measured by a source the agent does not control, says it is healthy.
//!
//! Fails closed: a guardrail whose metric was not measured counts as breached.
//! Absence of evidence is not evidence of safety.

use crate::types::{Guardrail, Limit};

pub struct Guardrails {
    rails: Vec<Guardrail>,
}

impl Guardrails {
    /// The standard service-level objectives applied to every deploy.
    pub fn standard() -> Self {
        Guardrails {
            rails: vec![
                Guardrail {
                    metric: "error_rate".to_string(),
                    limit: Limit::Max(0.02),
                },
                Guardrail {
                    metric: "task_completion".to_string(),
                    limit: Limit::Min(0.90),
                },
            ],
        }
    }

    /// Returns the first breached guardrail metric, given observed readings from
    /// trusted telemetry.
    pub fn first_breach(&self, observed: &[(String, f64)]) -> Option<String> {
        for rail in &self.rails {
            match observed.iter().find(|(metric, _)| metric == &rail.metric) {
                Some((_, value)) if rail.breached_by(*value) => return Some(rail.metric.clone()),
                Some(_) => {} // measured and within limits
                None => return Some(format!("{} (not measured)", rail.metric)),
            }
        }
        None
    }
}
