//! Invariants: the individual rules that make up the reference monitor.
//!
//! An invariant is *pure* — given a proposed action it returns any violations and
//! performs no I/O. Purity is what makes the rule set auditable: you can read each
//! one in isolation and know exactly what it forbids. `immutable_policy` is a free
//! function in [`crate::policy`]; reach is charter-parameterised
//! ([`reach::Charter`]) because *where* the agent may write depends on the
//! outcome's lifecycle stage.

pub mod change_shape;
pub mod reach;
