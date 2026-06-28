//! Append-only audit trail.
//!
//! Every proposal and every disposition is recorded with its rationale. This is
//! the system's memory and its accountability surface: it is never rewritten,
//! and it is where the "communicate what changed and why" story comes from.

#[derive(Default)]
pub struct DecisionLog {
    entries: Vec<String>,
}

impl DecisionLog {
    pub fn new() -> Self {
        DecisionLog { entries: vec![] }
    }

    pub fn record(&mut self, stage: &str, detail: impl Into<String>) {
        let line = format!("  {stage:<12} {}", detail.into());
        println!("{line}");
        self.entries.push(line);
    }

    pub fn entries(&self) -> &[String] {
        &self.entries
    }
}
