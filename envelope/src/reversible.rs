//! Reversible effects: the mechanism that makes "every enacted change is
//! undoable" a property of the code rather than a hope.
//!
//! The harness obtains an [`Effect`] for an action and applies it, receiving an
//! [`Undo`] handle in return. Holding that handle is the proof — enforced by the
//! type system — that the change can be reverted. There is no `apply` that does
//! not yield an `undo`.

use std::collections::HashMap;

/// The slice of the world the harness can change. Tiny and in-memory for the
/// skeleton; in a real system each field is backed by git and the deployment
/// system — each of which already supports undo.
///
/// Its fields are private. It exposes read-only views publicly and crate-private
/// mutators that only the harness drives via [`Effect`]s. Combined with the fact
/// that the harness never hands out `&mut World` (only `&World`), this makes
/// "mutate the world without going through `enact`" unreachable from outside the
/// trusted core — complete mediation by construction, not by convention.
#[derive(Default)]
pub struct World {
    files: HashMap<String, usize>, // path -> size in bytes
    deployed: HashMap<String, u8>, // service -> live traffic percentage
}

impl World {
    // ---- read-only views (public) ----
    pub fn file_count(&self) -> usize {
        self.files.len()
    }
    pub fn deploy_count(&self) -> usize {
        self.deployed.len()
    }

    // ---- prior-state lookups, used to build undos (crate-internal) ----
    pub(crate) fn file(&self, path: &str) -> Option<usize> {
        self.files.get(path).copied()
    }
    pub(crate) fn deploy(&self, service: &str) -> Option<u8> {
        self.deployed.get(service).copied()
    }

    // ---- mutations (crate-internal; only reached through an Effect) ----
    pub(crate) fn set_file(&mut self, path: String, bytes: usize) {
        self.files.insert(path, bytes);
    }
    pub(crate) fn restore_file(&mut self, path: String, prior: Option<usize>) {
        match prior {
            Some(bytes) => {
                self.files.insert(path, bytes);
            }
            None => {
                self.files.remove(&path);
            }
        }
    }
    pub(crate) fn set_deploy(&mut self, service: String, pct: u8) {
        self.deployed.insert(service, pct);
    }
    pub(crate) fn restore_deploy(&mut self, service: String, prior: Option<u8>) {
        match prior {
            Some(pct) => {
                self.deployed.insert(service, pct);
            }
            None => {
                self.deployed.remove(&service);
            }
        }
    }
}

/// A reversible effect. `apply` enacts it and returns the matching `undo`.
pub struct Effect {
    pub describe: String,
    apply_fn: Box<dyn FnOnce(&mut World)>,
    undo_fn: Box<dyn FnOnce(&mut World)>,
}

impl Effect {
    pub fn new(
        describe: impl Into<String>,
        apply_fn: impl FnOnce(&mut World) + 'static,
        undo_fn: impl FnOnce(&mut World) + 'static,
    ) -> Self {
        Effect {
            describe: describe.into(),
            apply_fn: Box::new(apply_fn),
            undo_fn: Box::new(undo_fn),
        }
    }

    /// Enact the effect. Consumes the effect and hands back the undo, so the
    /// only thing you can hold after applying is the means to reverse it.
    pub fn apply(self, world: &mut World) -> Undo {
        (self.apply_fn)(world);
        Undo {
            undo_fn: self.undo_fn,
        }
    }
}

/// The undo handle returned after applying an effect.
pub struct Undo {
    undo_fn: Box<dyn FnOnce(&mut World)>,
}

impl Undo {
    pub fn revert(self, world: &mut World) {
        (self.undo_fn)(world);
    }
}
