//! Invariants: the individual rules that make up the reference monitor.
//!
//! An invariant is a *pure function* — given a proposed action, it returns any
//! violations and performs no I/O. Purity is what makes the rule set auditable:
//! you can read each one in isolation and know exactly what it forbids.

pub mod change_shape;
pub mod reach;

use crate::types::{Action, Violation};

/// The shape every invariant takes.
pub type Invariant = fn(&Action) -> Vec<Violation>;
