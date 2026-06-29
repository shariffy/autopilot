//! The trusted reference monitor: the ONLY path from a proposed action to a real
//! effect on the world.
//!
//! Read it top to bottom — it is small on purpose. The agent has no other way to
//! touch anything; `enact` is the single chokepoint, and every branch of it is
//! deterministic.

use crate::decision_log::DecisionLog;
use crate::guardrails::Guardrails;
use crate::invariants::change_shape;
use crate::policy::Policy;
use crate::reversible::{Effect, World};
use crate::telemetry::Telemetry;
use crate::types::{Action, Outcome, Verdict};
use crate::verifier::Verifier;

pub struct Harness {
    policy: Policy,
    verifier: Box<dyn Verifier>,
    guardrails: Guardrails,
    telemetry: Box<dyn Telemetry>,
    world: World,
    log: DecisionLog,
}

impl Harness {
    /// Build a reference monitor over a trusted verifier and a trusted telemetry
    /// source. Both are injected, never supplied by the agent — that separation
    /// is what makes the verification gate and the outcome gate trustworthy.
    pub fn new(telemetry: Box<dyn Telemetry>, verifier: Box<dyn Verifier>) -> Self {
        Harness {
            policy: Policy::reference_monitor(),
            verifier,
            guardrails: Guardrails::standard(),
            telemetry,
            world: World::default(),
            log: DecisionLog::new(),
        }
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    /// The append-only audit trail accumulated across all `enact` calls.
    pub fn log(&self) -> &DecisionLog {
        &self.log
    }

    /// Submit a proposed action. This is the entire trust boundary.
    pub fn enact(&mut self, intent: &str, action: Action) -> Outcome {
        self.log.record("PROPOSED", intent.to_string());

        // 1. Deterministic policy check on the action itself (reach,
        //    immutable_policy). A denial here means the action never touches the
        //    world — structurally, not by convention.
        if let Verdict::Deny(violations) = self.policy.evaluate(&action) {
            return self.reject(violations);
        }

        // 2. Pre-deploy verification gate. Verification evidence comes from the
        //    TRUSTED verifier (CI + agentic UI checks) — never the agent's
        //    self-report, which it cannot even express. Fails closed.
        if let Action::Deploy { service, .. } = &action {
            let verification = self.verifier.verify(service);
            let violations = change_shape::check(service, &verification);
            if !violations.is_empty() {
                return self.reject(violations);
            }
        }

        // 3. Obtain a reversible effect BEFORE enacting. Holding the returned
        //    undo handle is the precondition for committing anything.
        let effect = plan_effect(&action, &self.world);
        self.log.record("ENACT", effect.describe.clone());
        let undo = effect.apply(&mut self.world);

        // 4. Outcome gate. Observed metrics come from TRUSTED telemetry and are
        //    judged against the TRUSTED guardrail policy — never from the agent.
        //    A breach reverts automatically.
        if let Action::Deploy { service, .. } = &action {
            let observed = self.telemetry.observe(service);
            if let Some(metric) = self.guardrails.first_breach(&observed) {
                undo.revert(&mut self.world);
                self.log.record(
                    "ROLLED_BACK",
                    format!("guardrail `{metric}` breached; reverted automatically"),
                );
                return Outcome::RolledBack { breached: metric };
            }
        }

        self.log.record("COMMITTED", "verified and guardrails held");
        Outcome::Committed
    }

    fn reject(&mut self, violations: Vec<crate::types::Violation>) -> Outcome {
        for v in &violations {
            self.log
                .record("DENIED", format!("[{}] {}", v.invariant, v.reason));
        }
        Outcome::Rejected(violations)
    }
}

/// Translate an allowed action into a reversible effect, capturing prior state
/// so the undo restores exactly what was there before.
fn plan_effect(action: &Action, world: &World) -> Effect {
    match action {
        Action::WriteFile { path, bytes } => {
            let path = path.clone();
            let bytes = *bytes;
            let prior = world.file(&path);
            let describe = format!("write {path} ({bytes} bytes)");
            let path_undo = path.clone();
            Effect::new(
                describe,
                move |w| w.set_file(path, bytes),
                move |w| w.restore_file(path_undo, prior),
            )
        }
        Action::Deploy {
            service,
            traffic_pct,
        } => {
            let service = service.clone();
            let pct = *traffic_pct;
            let prior = world.deploy(&service);
            let describe = format!("deploy {service} to {pct}% canary");
            let service_undo = service.clone();
            Effect::new(
                describe,
                move |w| w.set_deploy(service, pct),
                move |w| w.restore_deploy(service_undo, prior),
            )
        }
        // ModifyPolicy is always denied upstream and never reaches here, but the
        // match must be exhaustive — the compiler guarantees we considered it.
        Action::ModifyPolicy => Effect::new(
            "unreachable: policy modification is always denied",
            |_w| {},
            |_w| {},
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::Harness;
    use crate::telemetry::StubTelemetry;
    use crate::types::{Action, Outcome, Verification};
    use crate::verifier::StubVerifier;

    fn passing() -> Verification {
        Verification {
            typecheck: true,
            tests: true,
            ui_verified: true,
        }
    }

    fn deploy(service: &str) -> Action {
        Action::Deploy {
            service: service.into(),
            traffic_pct: 5,
        }
    }

    fn harness(telemetry: StubTelemetry, verifier: StubVerifier) -> Harness {
        Harness::new(Box::new(telemetry), Box::new(verifier))
    }

    #[test]
    fn forbidden_write_is_rejected_and_has_no_effect() {
        let mut h = harness(StubTelemetry::new(), StubVerifier::new());
        let out = h.enact(
            "api",
            Action::WriteFile {
                path: "src/api/client.ts".into(),
                bytes: 1,
            },
        );
        assert!(matches!(out, Outcome::Rejected(_)));
        assert_eq!(h.world().file_count(), 0);
    }

    #[test]
    fn policy_modification_is_denied() {
        let mut h = harness(StubTelemetry::new(), StubVerifier::new());
        assert!(matches!(
            h.enact("escalate", Action::ModifyPolicy),
            Outcome::Rejected(_)
        ));
    }

    #[test]
    fn unverified_deploy_is_denied_and_has_no_effect() {
        // The trusted verifier reports the UI did not pass — the agent has no say.
        let verifier = StubVerifier::new().set(
            "reports",
            Verification {
                typecheck: true,
                tests: true,
                ui_verified: false,
            },
        );
        let mut h = harness(StubTelemetry::new(), verifier);
        assert!(matches!(
            h.enact("ship", deploy("reports")),
            Outcome::Rejected(_)
        ));
        assert_eq!(h.world().deploy_count(), 0);
    }

    #[test]
    fn deploy_with_no_verification_evidence_fails_closed() {
        // No verifier result for the service ⇒ treated as unverified ⇒ rejected.
        let mut h = harness(StubTelemetry::new(), StubVerifier::new());
        assert!(matches!(
            h.enact("blind", deploy("unknown")),
            Outcome::Rejected(_)
        ));
    }

    #[test]
    fn healthy_deploy_commits() {
        let verifier = StubVerifier::new().set("dashboard", passing());
        let telemetry = StubTelemetry::new().set(
            "dashboard",
            vec![("error_rate", 0.001), ("task_completion", 0.95)],
        );
        let mut h = harness(telemetry, verifier);
        assert!(matches!(
            h.enact("ship", deploy("dashboard")),
            Outcome::Committed
        ));
        assert_eq!(h.world().deploy_count(), 1);
    }

    #[test]
    fn breached_guardrail_leaves_world_untouched() {
        let verifier = StubVerifier::new().set("onboarding", passing());
        let telemetry = StubTelemetry::new().set(
            "onboarding",
            vec![("error_rate", 0.001), ("task_completion", 0.50)],
        );
        let mut h = harness(telemetry, verifier);
        let before = h.world().deploy_count();
        let out = h.enact("breach", deploy("onboarding"));
        assert!(matches!(out, Outcome::RolledBack { .. }));
        assert_eq!(
            h.world().deploy_count(),
            before,
            "a rolled-back deploy must leave no trace"
        );
    }

    #[test]
    fn deploy_without_telemetry_fails_closed() {
        // Verified, but no telemetry to confirm health ⇒ rolled back.
        let verifier = StubVerifier::new().set("svc", passing());
        let mut h = harness(StubTelemetry::new(), verifier);
        assert!(matches!(
            h.enact("blind", deploy("svc")),
            Outcome::RolledBack { .. }
        ));
    }

    #[test]
    fn telemetry_decides_outcome_not_the_agent() {
        // Identical agent proposals; opposite outcomes, decided solely by trusted
        // telemetry the agent cannot touch.
        let healthy = StubTelemetry::new().set(
            "svc",
            vec![("error_rate", 0.001), ("task_completion", 0.99)],
        );
        let mut h1 = harness(healthy, StubVerifier::new().set("svc", passing()));
        assert!(matches!(h1.enact("x", deploy("svc")), Outcome::Committed));

        let unhealthy = StubTelemetry::new().set(
            "svc",
            vec![("error_rate", 0.001), ("task_completion", 0.10)],
        );
        let mut h2 = harness(unhealthy, StubVerifier::new().set("svc", passing()));
        assert!(matches!(
            h2.enact("x", deploy("svc")),
            Outcome::RolledBack { .. }
        ));
    }
}
