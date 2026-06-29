//! Reach invariant: *where* the agent may write.
//!
//! This is a frontend codebase that consumes an external, provided backend API.
//! The two lists below ARE the agent's reach over that codebase. Keep them short
//! enough that a human can take in the whole boundary at a glance — that
//! readability is the source of the trust.
//!
//! Paths are normalised *lexically* before they are checked, so `.`/`..`
//! components cannot smuggle a path out of an allowed prefix and into a
//! forbidden one (e.g. `src/components/../api/client.ts`). Normalisation never
//! touches the filesystem — these are repository paths, not necessarily real
//! files.

use crate::types::{Action, Violation};

/// Path prefixes the agent is permitted to write. Anything not matching is
/// refused by default (deny-by-default).
const ALLOWED_WRITE_PREFIXES: &[&str] = &[
    "src/components/",
    "src/features/",
    "src/routes/",
    "src/styles/",
    "config/flags/",
];

/// Prefixes that are never writable, listed explicitly so the most sensitive
/// zones are obvious to an auditor even though deny-by-default already covers
/// them.
const FORBIDDEN_WRITE_PREFIXES: &[&str] = &[
    "src/api/",  // the contract/client for the provided backend — not the agent's to change
    "secrets/",  // credentials must never be written into the frontend bundle
    "infra/",    // build and deploy pipeline configuration
    "envelope/", // the trusted core may not be edited by the agent it governs
];

pub fn check(action: &Action) -> Vec<Violation> {
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

    fn write(path: &str) -> Vec<Violation> {
        check(&Action::WriteFile {
            path: path.to_string(),
            bytes: 1,
        })
    }

    #[test]
    fn allowed_path_passes() {
        assert!(write("src/components/UserTable.tsx").is_empty());
    }

    #[test]
    fn forbidden_path_denied() {
        // The provided backend contract is not the agent's to edit.
        assert!(!write("src/api/client.ts").is_empty());
    }

    #[test]
    fn outside_allowlist_denied() {
        assert!(!write("scripts/deploy.sh").is_empty());
    }

    #[test]
    fn traversal_into_forbidden_is_denied() {
        // A `..` must not move the path out of an allowed prefix and into a
        // forbidden one.
        assert!(!write("src/components/../api/client.ts").is_empty());
    }

    #[test]
    fn escaping_root_is_denied() {
        assert!(!write("../secrets").is_empty());
        assert!(!write("src/components/../../secrets/tokens.ts").is_empty());
    }

    /// Exhaustive check over every path up to length 4 built from a small
    /// alphabet that mixes `.`/`..` with allowed and forbidden segments: every
    /// *accepted* path, once normalised, must land inside an allowed prefix and
    /// never inside a forbidden one.
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
            if write(&path).is_empty() {
                let norm = normalize(&path).expect("an accepted path must normalise");
                assert!(
                    ALLOWED_WRITE_PREFIXES.iter().any(|p| norm.starts_with(p)),
                    "accepted but not in allowlist: {path:?} -> {norm:?}"
                );
                assert!(
                    !FORBIDDEN_WRITE_PREFIXES.iter().any(|p| norm.starts_with(p)),
                    "accepted but resolves into a forbidden zone: {path:?} -> {norm:?}"
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
