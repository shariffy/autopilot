//! Reach invariant: *where* the agent may write.
//!
//! Reach is not a single global list — it is a **clearance**, and which
//! clearance is in force is decided by the outcome's *lifecycle stage*, never by
//! the agent (see ADR 0005):
//!
//! - [`Clearance::Maintenance`] — a governed app has launched. Reach is the narrow,
//!   human-auditable allowlist below; the API contract, secrets, and infra are
//!   frozen. This is the stage the structural-trust thesis is about.
//! - [`Clearance::Genesis`] — establishment, before launch. The agent is bringing an
//!   app into existence (or doing structural surgery on adopted code), so it must
//!   be able to write config, the API client, and auth. Reach is the whole
//!   workspace *except* the two zones that are never the agent's to touch:
//!   `secrets/` and `.git/`. Genesis is not bounded by reach alone — it is bounded
//!   by a disposable workspace, atomic reversibility, and a human launch gate.
//!
//! Paths are normalised *lexically* before they are checked, so `.`/`..`
//! components cannot smuggle a path out of an allowed prefix and into a forbidden
//! one (e.g. `src/components/../api/client.ts`). Normalisation never touches the
//! filesystem — these are repository paths, not necessarily real files.

use crate::types::{Action, Violation};

/// The reach clearance in force over an outcome, selected by lifecycle stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clearance {
    /// Establishment, pre-launch: the whole workspace minus the never-touch zones.
    Genesis,
    /// Post-launch maintenance: the narrow fitted allowlist.
    Maintenance,
}

/// Path prefixes the agent may write under the Maintenance clearance. Anything not
/// matching is refused by default (deny-by-default).
const ALLOWED_WRITE_PREFIXES: &[&str] = &[
    "src/components/",
    "src/features/",
    "src/routes/",
    "src/styles/",
    "config/flags/",
];

/// Prefixes that are never writable under Maintenance, listed explicitly so the
/// most sensitive zones are obvious to an auditor even though deny-by-default
/// already covers them.
const FORBIDDEN_WRITE_PREFIXES: &[&str] = &[
    "src/api/",  // the contract/client for the provided backend — not the agent's to change
    "secrets/",  // credentials must never be written into the frontend bundle
    "infra/",    // build and deploy pipeline configuration
    "envelope/", // the trusted core may not be edited by the agent it governs
];

/// Zones that are never the agent's to touch under *any* clearance — even the broad
/// Genesis clearance is confined to the workspace minus these.
const NEVER_WRITE_PREFIXES: &[&str] = &[
    "secrets/", // credentials
    ".git/",    // the version history that makes reversibility well-defined
];

impl Clearance {
    /// Find any reach violations this clearance raises for `action`. Pure: no I/O.
    pub fn check(self, action: &Action) -> Vec<Violation> {
        let path = match action {
            Action::WriteFile { path, .. } => path,
            _ => return vec![],
        };

        let Some(normalized) = normalize(path) else {
            return vec![Violation {
                invariant: "reach",
                reason: format!("`{path}` escapes the repository root"),
            }];
        };

        match self {
            Clearance::Genesis => self.check_genesis(path, &normalized),
            Clearance::Maintenance => self.check_maintenance(path, &normalized),
        }
    }

    fn check_genesis(self, path: &str, normalized: &str) -> Vec<Violation> {
        if NEVER_WRITE_PREFIXES.iter().any(|p| normalized.starts_with(p)) {
            return vec![Violation {
                invariant: "reach",
                reason: format!("`{path}` resolves into a never-writable zone"),
            }];
        }
        vec![]
    }

    fn check_maintenance(self, path: &str, normalized: &str) -> Vec<Violation> {
        if FORBIDDEN_WRITE_PREFIXES
            .iter()
            .any(|p| normalized.starts_with(p))
        {
            return vec![Violation {
                invariant: "reach",
                reason: format!("`{path}` resolves into an explicitly forbidden zone"),
            }];
        }

        if !ALLOWED_WRITE_PREFIXES
            .iter()
            .any(|p| normalized.starts_with(p))
        {
            return vec![Violation {
                invariant: "reach",
                reason: format!("`{path}` is outside the write allowlist"),
            }];
        }

        vec![]
    }
}

/// Lexically normalise a repository path, resolving `.` and `..` without
/// touching the filesystem. Returns `None` if the path escapes the root (a `..`
/// with nothing above it) — itself a reason to refuse.
fn normalize(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for component in path.split('/') {
        match component {
            "" | "." => continue,
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Action;

    fn maintenance(path: &str) -> Vec<Violation> {
        Clearance::Maintenance.check(&Action::WriteFile {
            path: path.to_string(),
            bytes: 1,
        })
    }

    fn genesis(path: &str) -> Vec<Violation> {
        Clearance::Genesis.check(&Action::WriteFile {
            path: path.to_string(),
            bytes: 1,
        })
    }

    #[test]
    fn maintenance_allows_allowlisted_path() {
        assert!(maintenance("src/components/UserTable.tsx").is_empty());
    }

    #[test]
    fn maintenance_forbids_api_contract() {
        // The provided backend contract is not the agent's to edit.
        assert!(!maintenance("src/api/client.ts").is_empty());
    }

    #[test]
    fn maintenance_forbids_outside_allowlist() {
        assert!(!maintenance("scripts/deploy.sh").is_empty());
    }

    #[test]
    fn maintenance_forbids_traversal_into_forbidden() {
        // A `..` must not move the path out of an allowed prefix and into a
        // forbidden one.
        assert!(!maintenance("src/components/../api/client.ts").is_empty());
    }

    #[test]
    fn maintenance_forbids_escaping_root() {
        assert!(!maintenance("../secrets").is_empty());
        assert!(!maintenance("src/components/../../secrets/tokens.ts").is_empty());
    }

    #[test]
    fn genesis_allows_what_maintenance_freezes() {
        // Genesis must be able to bring the app into existence: config, the API
        // client, auth — the very things Maintenance freezes.
        assert!(genesis("package.json").is_empty());
        assert!(genesis("vite.config.ts").is_empty());
        assert!(genesis("src/api/client.ts").is_empty());
        assert!(genesis("src/lib/auth.tsx").is_empty());
    }

    #[test]
    fn genesis_still_forbids_never_zones() {
        // Even the broad clearance never writes credentials or rewrites history.
        assert!(!genesis("secrets/tokens.ts").is_empty());
        assert!(!genesis(".git/config").is_empty());
    }

    #[test]
    fn genesis_still_refuses_escaping_root() {
        assert!(!genesis("../../etc/passwd").is_empty());
        // ...and traversal back into a never-zone after a detour.
        assert!(!genesis("src/x/../../secrets/tokens.ts").is_empty());
    }

    #[test]
    fn dotgitignore_is_not_the_git_dir() {
        // `.gitignore` is a normal file; only the `.git/` directory is off-limits.
        assert!(genesis(".gitignore").is_empty());
    }

    /// Exhaustive check over every path up to length 4 built from a small alphabet
    /// that mixes `.`/`..` with allowed and forbidden segments: under *each*
    /// clearance, every *accepted* path, once normalised, must stay within that
    /// clearance's permitted region and never inside its forbidden zones.
    #[test]
    fn accepted_paths_never_resolve_into_forbidden_zones() {
        const TOKENS: &[&str] = &[
            "src",
            "components",
            "features",
            "api",
            "secrets",
            "infra",
            "x",
            "..",
            ".",
        ];
        let mut paths = Vec::new();
        enumerate(TOKENS, 4, &mut Vec::new(), &mut paths);

        for path in paths {
            if maintenance(&path).is_empty() {
                let norm = normalize(&path).expect("an accepted path must normalise");
                assert!(
                    ALLOWED_WRITE_PREFIXES.iter().any(|p| norm.starts_with(p)),
                    "maintenance accepted but not in allowlist: {path:?} -> {norm:?}"
                );
                assert!(
                    !FORBIDDEN_WRITE_PREFIXES.iter().any(|p| norm.starts_with(p)),
                    "maintenance accepted but resolves into a forbidden zone: {path:?} -> {norm:?}"
                );
            }
            if genesis(&path).is_empty() {
                let norm = normalize(&path).expect("an accepted path must normalise");
                assert!(
                    !NEVER_WRITE_PREFIXES.iter().any(|p| norm.starts_with(p)),
                    "genesis accepted but resolves into a never-writable zone: {path:?} -> {norm:?}"
                );
            }
        }
    }

    fn enumerate(tokens: &[&str], depth: usize, prefix: &mut Vec<String>, out: &mut Vec<String>) {
        if !prefix.is_empty() {
            out.push(prefix.join("/"));
        }
        if depth == 0 {
            return;
        }
        for &token in tokens {
            prefix.push(token.to_string());
            enumerate(tokens, depth - 1, prefix, out);
            prefix.pop();
        }
    }
}
