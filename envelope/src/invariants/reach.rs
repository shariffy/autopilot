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
    "src/pages/",
    "config/flags/",
];

/// Exact-match filenames the agent may write under Maintenance — checked by
/// equality, never by prefix, because `ALLOWED_WRITE_PREFIXES` is
/// prefix-matched and a prefix rule for `package.json` would wrongly admit
/// `package.json.bak` too. The agent proposes dependency *intent* here; the
/// envelope computes the lockfile from it (see `NEVER_WRITE_FILES` and
/// `docs/adr/0009-dependency-maintenance.md`).
const ALLOWED_WRITE_FILES: &[&str] = &["package.json"];

/// Lockfiles: never the agent's to author, under **any** clearance — not even
/// the broad Genesis one. A lockfile's `resolved` URL and `integrity` hash are
/// attacker-controllable *as a pair* (npm binds integrity to whatever was
/// downloaded, not to the registry's published artifact), and installing from
/// one runs dependency lifecycle scripts inside the envelope's own verifier, on
/// the host. The trusted core computes these from the agent's `package.json`
/// intent instead (`worktree::BuildVerifier`); expressive power costs nothing —
/// ranges, `overrides`, and exact pins are all sayable in `package.json`.
/// Exact-match, like `ALLOWED_WRITE_FILES`: a `package-lock.json.bak` is an
/// ordinary file governed by the normal rules, not this one.
const NEVER_WRITE_FILES: &[&str] = &["package-lock.json", "pnpm-lock.yaml", "yarn.lock"];

/// Prefixes that are never writable under Maintenance, listed explicitly so the
/// most sensitive zones are obvious to an auditor even though deny-by-default
/// already covers them.
const FORBIDDEN_WRITE_PREFIXES: &[&str] = &[
    "src/api/",  // the contract/client for the provided backend — not the agent's to change
    "src/data/", // the data/fixture contract — frozen under maintenance, exactly like src/api/
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
        if let Some(v) = never_write_file_violation(path, normalized) {
            return vec![v];
        }
        if NEVER_WRITE_PREFIXES
            .iter()
            .any(|p| normalized.starts_with(p))
        {
            return vec![Violation {
                invariant: "reach",
                reason: format!("`{path}` resolves into a never-writable zone"),
            }];
        }
        vec![]
    }

    fn check_maintenance(self, path: &str, normalized: &str) -> Vec<Violation> {
        if let Some(v) = never_write_file_violation(path, normalized) {
            return vec![v];
        }

        if FORBIDDEN_WRITE_PREFIXES
            .iter()
            .any(|p| normalized.starts_with(p))
        {
            return vec![Violation {
                invariant: "reach",
                reason: format!("`{path}` resolves into an explicitly forbidden zone"),
            }];
        }

        if ALLOWED_WRITE_FILES.contains(&normalized) {
            return vec![];
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

/// The reach denial for a lockfile, or `None` if `normalized` is not one.
/// Shared by `check_genesis` and `check_maintenance` so the rule reads
/// identically — and is enforced identically — under every clearance.
fn never_write_file_violation(path: &str, normalized: &str) -> Option<Violation> {
    if NEVER_WRITE_FILES.contains(&normalized) {
        Some(Violation {
            invariant: "reach",
            reason: format!(
                "`{path}` is a lockfile: computed by the trusted core from `package.json`, never authored by the agent"
            ),
        })
    } else {
        None
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
    fn maintenance_allows_page_components() {
        // The app's page components (src/pages/) are ordinary in-page surface,
        // fitted to how the real app is laid out — same footing as src/components/.
        assert!(maintenance("src/pages/ProductsPage.tsx").is_empty());
    }

    #[test]
    fn maintenance_forbids_data_contract() {
        // The data/fixture contract is frozen under maintenance, exactly like
        // src/api/ — a bounded in-page change must not be able to redefine what
        // data the app serves.
        assert!(!maintenance("src/data/products.json").is_empty());
    }

    #[test]
    fn genesis_allows_the_data_contract_maintenance_freezes() {
        // Genesis must be able to establish the data/fixture layer in the first
        // place; only Maintenance freezes it once the outcome has launched.
        assert!(genesis("src/data/products.json").is_empty());
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

    #[test]
    fn maintenance_allows_package_json_by_exact_match() {
        // The dependency-intent surface (ADR 0009): the agent may propose
        // `package.json` changes even though maintenance's prefix allowlist
        // otherwise covers only `src/`/`config/flags/`.
        assert!(maintenance("package.json").is_empty());
    }

    #[test]
    fn maintenance_forbids_package_json_bak_despite_the_exact_match_allowlist() {
        // Proves the allowlist is exact-match, not a prefix: a real prefix rule
        // for `package.json` would wrongly admit this too.
        assert!(!maintenance("package.json.bak").is_empty());
    }

    #[test]
    fn lockfiles_are_never_writable_under_any_clearance() {
        for name in ["package-lock.json", "pnpm-lock.yaml", "yarn.lock"] {
            let m = maintenance(name);
            assert!(!m.is_empty(), "maintenance accepted a lockfile: {name}");
            assert!(
                m[0].reason.contains("computed by the trusted core"),
                "reason should name the real cause: {:?}",
                m[0].reason
            );

            let g = genesis(name);
            assert!(!g.is_empty(), "genesis accepted a lockfile: {name}");
            assert!(
                g[0].reason.contains("computed by the trusted core"),
                "reason should name the real cause: {:?}",
                g[0].reason
            );
        }
    }

    #[test]
    fn lockfile_backup_files_are_ordinary_files() {
        // Exact-match again: a `.bak` sibling of a lockfile is not itself a
        // lockfile, so it is governed by the normal allow/forbid rules only.
        assert!(!maintenance("package-lock.json.bak").is_empty()); // outside allowlist, not a lockfile denial
        assert!(genesis("package-lock.json.bak").is_empty()); // genesis' broad clearance covers it
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
            "pages",
            "data",
            "api",
            "secrets",
            "infra",
            "x",
            "..",
            ".",
            "package.json",
            "package.json.bak",
            "package-lock.json",
            "pnpm-lock.yaml",
            "yarn.lock",
        ];
        let mut paths = Vec::new();
        enumerate(TOKENS, 4, &mut Vec::new(), &mut paths);

        for path in paths {
            if maintenance(&path).is_empty() {
                let norm = normalize(&path).expect("an accepted path must normalise");
                assert!(
                    ALLOWED_WRITE_PREFIXES.iter().any(|p| norm.starts_with(p))
                        || ALLOWED_WRITE_FILES.iter().any(|f| norm == *f),
                    "maintenance accepted but not in allowlist: {path:?} -> {norm:?}"
                );
                assert!(
                    !FORBIDDEN_WRITE_PREFIXES.iter().any(|p| norm.starts_with(p)),
                    "maintenance accepted but resolves into a forbidden zone: {path:?} -> {norm:?}"
                );
                assert!(
                    !NEVER_WRITE_FILES.iter().any(|f| norm == *f),
                    "maintenance accepted a lockfile: {path:?} -> {norm:?}"
                );
            }
            if genesis(&path).is_empty() {
                let norm = normalize(&path).expect("an accepted path must normalise");
                assert!(
                    !NEVER_WRITE_PREFIXES.iter().any(|p| norm.starts_with(p)),
                    "genesis accepted but resolves into a never-writable zone: {path:?} -> {norm:?}"
                );
                assert!(
                    !NEVER_WRITE_FILES.iter().any(|f| norm == *f),
                    "genesis accepted a lockfile: {path:?} -> {norm:?}"
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
