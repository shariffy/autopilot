//! The runtime envelope: flight-envelope protection for a change that already
//! landed and already deployed.
//!
//! Everything else in this crate governs whether a proposed change is allowed
//! to reach production (reach, verification, the outcome gate). This module is
//! the seam AFTER that: a change that cleared every one of those gates — reach
//! allowed it, the build was green, tests were green if the outcome had any —
//! can still be wrong in a way none of that catches (ADR 0011; THREAT_MODEL.md
//! R7). Production telemetry is the only thing that ever finds out.
//!
//! `monitor` is the fast loop: read trusted telemetry, and if the error rate
//! it reports exceeds the threshold, revert the currently-deployed change —
//! `git revert --no-edit HEAD`, stamped with the envelope's own identity
//! ([`crate::worktree::ENVELOPE_IDENT`]), both as author and committer. That
//! stamping is not incidental: this path takes **no advisor input of any
//! kind** — no proposal, no intent string, nothing the agent supplied — it is
//! the trusted core alone, reacting to a trusted sensor. Nothing here depends
//! on the agent, or even involves it. That property is the entire point: an
//! elevated error rate trips the revert regardless of whether an advisor is
//! running, reachable, or has any idea anything went wrong.

use std::path::Path;

use crate::worktree::{self, ENVELOPE_IDENT};

/// A guardrail ceiling used only when the telemetry file supplies no
/// `threshold` of its own and the caller passes no `--threshold` override.
/// Matches `guardrails::Guardrails::standard`'s `error_rate` ceiling, so a
/// production deploy and a runtime monitor read the same default SLO absent
/// more specific instruction.
const DEFAULT_ERROR_RATE_THRESHOLD: f64 = 0.02;

/// What the monitor decided, mirroring [`crate::worktree::Disposition`] in
/// shape but naming outcomes specific to a runtime trip.
pub enum Disposition {
    /// The observed error rate exceeded the threshold: the envelope reverted
    /// the deployed change with `git revert --no-edit HEAD`, stamped as the
    /// envelope's own action. `reverted_to` is the short hash of the new
    /// `HEAD` (the revert commit) — the state now live.
    Tripped { reverted_to: String, detail: String },
    /// The observed error rate was within the threshold: nothing changed.
    Nominal { detail: String },
    /// Telemetry could not be read/parsed, or the revert itself failed (e.g. a
    /// dirty tree, or `HEAD` has nothing to revert). Fails closed: nothing is
    /// touched when the monitor cannot form a sound verdict.
    Refused { reason: String },
}

/// Read `telemetry_path` for an `error_rate` (and, as a default, a
/// `threshold`), and trip the runtime envelope if the rate exceeds it.
///
/// `threshold_override`, when given (the `--threshold` CLI flag), wins over
/// whatever the telemetry file itself records — an operator overriding the
/// sensor's own recorded SLO for this one call. Absent both, the standard
/// guardrail ceiling applies.
///
/// This function's ONLY inputs are the telemetry file and the threshold — no
/// agent proposal, no intent, nothing supplied by anything the agent
/// controls. Contrast every other entry point in this crate (`stage`,
/// `commit`, `adjudicate_write`), each of which starts from something the
/// agent proposed and judges it. There is nothing to judge here; there is
/// only a sensor reading and a fixed reaction to it.
pub fn monitor(repo: &Path, telemetry_path: &Path, threshold_override: Option<f64>) -> Disposition {
    let content = match std::fs::read_to_string(telemetry_path) {
        Ok(c) => c,
        Err(e) => {
            return Disposition::Refused {
                reason: format!(
                    "could not read telemetry `{}`: {e}",
                    telemetry_path.display()
                ),
            }
        }
    };

    let Some(error_rate) = extract_number_field(&content, "error_rate") else {
        return Disposition::Refused {
            reason: format!(
                "telemetry `{}` has no numeric `error_rate` field",
                telemetry_path.display()
            ),
        };
    };

    let threshold = threshold_override
        .or_else(|| extract_number_field(&content, "threshold"))
        .unwrap_or(DEFAULT_ERROR_RATE_THRESHOLD);

    if error_rate <= threshold {
        return Disposition::Nominal {
            detail: format!("error_rate {error_rate} <= threshold {threshold}; nothing reverted"),
        };
    }

    trip(repo, error_rate, threshold)
}

/// Revert the currently-deployed change: pure envelope action, stamped as
/// both author and committer by [`ENVELOPE_IDENT`] — the same identity
/// `establish` uses for its baseline commit, for the same reason (ADR 0010):
/// nothing here is the advisor's work to attribute.
fn trip(repo: &Path, error_rate: f64, threshold: f64) -> Disposition {
    // A revert needs a well-defined `HEAD` to revert FROM — the same
    // precondition `begin` asserts before opening a changeset. Refusing on a
    // dirty tree here is not a missed trip: it means the deployed state is
    // not what git says it is, which is a worse problem than a slow response
    // to one, and reverting on top of it would not be reverting to a sound
    // baseline.
    if let Err(reason) = worktree::ensure_clean(repo) {
        return Disposition::Refused { reason };
    }

    let (ok, out) = worktree::git_commit_as(
        repo,
        &["revert", "--no-edit", "HEAD"],
        ENVELOPE_IDENT,
        ENVELOPE_IDENT,
    );
    if !ok {
        return Disposition::Refused {
            reason: format!("git revert failed: {}", tail(&out, 800)),
        };
    }

    let (_, reverted_to) = worktree::git(repo, &["rev-parse", "--short", "HEAD"]);
    Disposition::Tripped {
        reverted_to: reverted_to.trim().to_string(),
        detail: format!(
            "error_rate {error_rate} > threshold {threshold}; reverted the deployed change"
        ),
    }
}

/// Extract a numeric field's value from a small hand-authored JSON telemetry
/// document. A targeted scan, not a JSON parser — same rationale as
/// `worktree::parse_advisory_ids`: the crate stays zero-dependency, and the
/// one field this needs is stable enough that scanning for it is simpler,
/// and no less correct, than hand-rolling a parser for a document otherwise
/// unused.
fn extract_number_field(json: &str, key: &str) -> Option<f64> {
    let pat = format!("\"{key}\"");
    let idx = json.find(&pat)?;
    let rest = json[idx + pat.len()..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')))
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// Keep only the last `max` bytes of command output, on a char boundary —
/// mirrors `worktree::tail`.
fn tail(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut start = s.len() - max;
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    format!("…{}", &s[start..])
}

#[cfg(test)]
mod tests {
    use super::extract_number_field;

    #[test]
    fn extracts_a_plain_field() {
        let json = r#"{"error_rate": 0.183, "threshold": 0.02}"#;
        assert_eq!(extract_number_field(json, "error_rate"), Some(0.183));
        assert_eq!(extract_number_field(json, "threshold"), Some(0.02));
    }

    #[test]
    fn missing_field_is_none() {
        let json = r#"{"error_rate": 0.01}"#;
        assert_eq!(extract_number_field(json, "threshold"), None);
    }

    #[test]
    fn handles_prettyprinted_spacing_and_nesting() {
        let json = "{\n  \"deploy\": {\"commit\": \"abc\"},\n  \"error_rate\":   0.5\n}";
        assert_eq!(extract_number_field(json, "error_rate"), Some(0.5));
    }

    #[test]
    fn non_numeric_value_is_none() {
        let json = r#"{"error_rate": "high"}"#;
        assert_eq!(extract_number_field(json, "error_rate"), None);
    }
}
