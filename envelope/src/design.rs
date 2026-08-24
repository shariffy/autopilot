//! Design-conformance stage (ADR 0012, Phase D): a content-level check that the
//! staged `.tsx` files in the app's UI zones actually compose the design system
//! in `src/design-system/`, rather than smuggling in ad-hoc raw markup.
//!
//! **Why this cannot be a `reach` invariant.** Reach sees only
//! `Action::WriteFile { path }` — a path, never the
//! bytes themselves (see `invariants/reach.rs`'s module doc: reach is about
//! WHERE the agent may write, never what it writes). Whether a file opens a raw
//! `<button>` instead of the design system's `Button` is a property of its
//! CONTENT, decidable only by a stage that reads the staged bytes back off
//! disk — so this lives beside `BuildVerifier`'s build/test/audit stages in
//! `worktree.rs`, invoked from `BuildVerifier::run`, not in `invariants/`.
//! `reach.rs` still does its part: `src/design-system/` itself is a frozen
//! zone under Maintenance (`FORBIDDEN_WRITE_PREFIXES`), so the agent composes
//! FROM the primitives but cannot fork or edit them. The two together are the
//! invariant: reach freezes the primitives, this stage requires their use.
//!
//! **Deliberately lint-level.** Line/substring scanning, exactly like
//! `worktree.rs`'s `parse_advisory_ids`/`json_object` — the crate stays
//! zero-dependency (`Cargo.toml`); a real JSX/TSX parser is not pulled in for
//! this. The rule set below is a small, explicit, ENFORCEABLE SUBSET of "use
//! the design system" — raw tags, inline styles, and import presence — not a
//! full composition grammar (which component may nest inside which, prop-level
//! constraints, whether a banned tag inside a string literal or comment should
//! really count). That gap is a documented residual, not an oversight: see
//! `docs/THREAT_MODEL.md` and `docs/adr/0012-the-design-system-invariant.md`.

use std::path::Path;

/// Raw tags the design system replaces, paired with the primitive that
/// replaces each — named explicitly here so a denial can quote the exact rule
/// rather than a generic "banned tag" message. Substring matches on the
/// staged file's raw text (not a tag-open grammar), so `<button` inside a
/// string literal or comment is also caught — the false-positive a lint-level
/// check accepts, in exchange for needing no parser (see the module doc).
/// `<a ` carries a trailing space so it does not also match `<article`/
/// `<aside`, and so JSX components like `<Link`/`Audio` are unaffected.
const BANNED_RAW_TAGS: &[(&str, &str)] = &[
    (
        "<button",
        "raw `<button` — compose `Button` from the design system instead",
    ),
    (
        "<input",
        "raw `<input` — compose `TextInput` from the design system instead",
    ),
    (
        "<select",
        "raw `<select` — compose `Select` from the design system instead",
    ),
    (
        "<a ",
        "raw `<a` (anchor) — compose `Link` from the design system instead",
    ),
];

/// Inline styling bypasses the design system's tokens entirely — styling
/// belongs in the primitives, not hand-rolled per call site.
const INLINE_STYLE_PATTERN: &str = "style={{";
const INLINE_STYLE_RULE: &str =
    "inline `style={{...}}` — styling belongs in the design system's primitives, not hand-rolled per call site";

/// A staged UI file must import at least one design-system primitive — closing
/// the gap the two rules above leave open: hand-rolling an equivalent that
/// merely avoids the literal banned substrings (a `<span onClick=...>`
/// pretending to be a button, styling via a bespoke class instead of
/// `style={{`) would otherwise still slip through.
const DESIGN_SYSTEM_IMPORT_RULE: &str =
    "no import from the design system — compose this file's UI from `src/design-system/` primitives";

/// Which staged paths this stage inspects: `.tsx` files under the app's UI
/// zones. Everything else (config, data, non-UI `.ts`) is out of scope — the
/// design system governs UI composition, not the whole tree.
fn in_scope(path: &str) -> bool {
    (path.starts_with("src/pages/") || path.starts_with("src/components/"))
        && path.ends_with(".tsx")
}

/// True if `content` has an import statement naming the design system, either
/// by relative path (`../design-system`, `../../design-system`, an import of
/// a specific primitive like `../design-system/Button`, …) or the
/// `@/design-system` alias form — both contain the substring `/design-system`.
fn imports_design_system(content: &str) -> bool {
    content
        .lines()
        .any(|line| line.trim_start().starts_with("import") && line.contains("/design-system"))
}

/// The rule violations `content` raises, if any. Checked independently — a
/// file failing more than one rule reports all of them, not just the first,
/// so a fix-and-retry does not surface violations one at a time.
fn violations(content: &str) -> Vec<&'static str> {
    let mut found: Vec<&'static str> = BANNED_RAW_TAGS
        .iter()
        .filter(|(pattern, _)| content.contains(pattern))
        .map(|(_, rule)| *rule)
        .collect();
    if content.contains(INLINE_STYLE_PATTERN) {
        found.push(INLINE_STYLE_RULE);
    }
    if !imports_design_system(content) {
        found.push(DESIGN_SYSTEM_IMPORT_RULE);
    }
    found
}

/// Lint every staged, in-scope file against the rule set above. `Ok(())` if
/// all are conformant (including: no in-scope files staged at all — this
/// stage has nothing to say about a changeset that never touches the UI
/// zones). `Err` names every offending file and the specific rule(s) it broke,
/// so a rejected changeset says exactly what to fix, not just that it failed.
pub fn check(repo: &Path, staged: &[String]) -> Result<(), String> {
    let mut failures = Vec::new();
    for path in staged {
        if !in_scope(path) {
            continue;
        }
        // Unreadable (binary, already removed, …) is not this stage's problem —
        // the build stage that already ran would have caught a real absence.
        let Ok(content) = std::fs::read_to_string(repo.join(path)) else {
            continue;
        };
        let file_violations = violations(&content);
        if !file_violations.is_empty() {
            failures.push(format!("`{path}`: {}", file_violations.join("; ")));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!("design conformance:\n{}", failures.join("\n")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- in_scope ----

    #[test]
    fn scopes_pages_and_components_tsx_only() {
        assert!(in_scope("src/pages/ProductsPage.tsx"));
        assert!(in_scope("src/components/Toolbar.tsx"));
        assert!(
            !in_scope("src/App.tsx"),
            "App.tsx is outside pages/components"
        );
        assert!(!in_scope("src/pages/helpers.ts"), "not .tsx");
        assert!(
            !in_scope("src/data/products.json"),
            "the data contract is out of scope for this stage"
        );
        assert!(
            !in_scope("src/design-system/Button.tsx"),
            "the primitives themselves are not linted by this stage"
        );
    }

    // ---- violations: banned raw tags ----

    #[test]
    fn flags_raw_button() {
        let content =
            "import { Card } from '../design-system'\nexport const x = () => <button>go</button>";
        let v = violations(content);
        assert!(
            v.iter().any(|r| r.contains("raw `<button`")),
            "expected a raw-button violation, got: {v:?}"
        );
    }

    #[test]
    fn flags_raw_input() {
        let content = "import { Card } from '../design-system'\nexport const x = () => <input />";
        let v = violations(content);
        assert!(v.iter().any(|r| r.contains("raw `<input`")), "{v:?}");
    }

    #[test]
    fn flags_raw_select() {
        let content = "import { Card } from '../design-system'\nexport const x = () => <select />";
        let v = violations(content);
        assert!(v.iter().any(|r| r.contains("raw `<select`")), "{v:?}");
    }

    #[test]
    fn flags_raw_anchor_but_not_link_component() {
        let raw_anchor =
            "import { Card } from '../design-system'\nexport const x = () => <a href=\"/x\">go</a>";
        assert!(
            violations(raw_anchor)
                .iter()
                .any(|r| r.contains("raw `<a`")),
            "a raw anchor should be flagged"
        );

        let link_component =
            "import { Card } from '../design-system'\nimport { Link } from 'react-router-dom'\nexport const x = () => <Link to=\"/x\">go</Link>";
        assert!(
            !violations(link_component)
                .iter()
                .any(|r| r.contains("raw `<a`")),
            "the <Link> component must not be mistaken for a raw anchor"
        );
    }

    #[test]
    fn flags_inline_style() {
        let content =
            "import { Card } from '../design-system'\nexport const x = () => <div style={{ color: 'red' }} />";
        let v = violations(content);
        assert!(v.iter().any(|r| r.contains("inline `style={{")), "{v:?}");
    }

    // ---- violations: design-system import requirement ----

    #[test]
    fn flags_missing_design_system_import() {
        let content = "export const x = () => <table><tbody /></table>";
        let v = violations(content);
        assert!(
            v.iter()
                .any(|r| r.contains("no import from the design system")),
            "{v:?}"
        );
    }

    #[test]
    fn accepts_the_alias_import_form() {
        let content =
            "import { Button } from '@/design-system'\nexport const x = () => <div>{Button}</div>";
        assert!(
            !violations(content)
                .iter()
                .any(|r| r.contains("no import from the design system")),
            "the @/design-system alias should satisfy the import rule"
        );
    }

    #[test]
    fn accepts_a_deep_relative_import_form() {
        let content = "import { Button } from '../../design-system'\nexport const x = () => <div>{Button}</div>";
        assert!(
            !violations(content)
                .iter()
                .any(|r| r.contains("no import from the design system")),
            "a deeper relative path should still satisfy the import rule"
        );
    }

    // ---- a conformant file has no violations at all ----

    #[test]
    fn conformant_file_composed_from_primitives_has_no_violations() {
        let content = "import { Card, Button } from '../design-system'\n\
             export const Toolbar = () => (\n\
               <Card title=\"Products\">\n\
                 <Button onClick={() => {}}>Clear filters</Button>\n\
               </Card>\n\
             )";
        assert!(
            violations(content).is_empty(),
            "expected no violations, got: {:?}",
            violations(content)
        );
    }

    // ---- check(): file scoping and multi-violation reporting over a real tree ----

    #[test]
    fn check_ignores_out_of_scope_staged_paths() {
        let dir =
            std::env::temp_dir().join(format!("envelope-design-check-oos-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src/data")).unwrap();
        std::fs::write(dir.join("src/data/products.json"), b"<button>").unwrap();

        let result = check(&dir, &["src/data/products.json".to_string()]);
        assert!(
            result.is_ok(),
            "a file outside src/pages|components must never be linted: {result:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn check_reports_the_offending_path_and_every_rule_it_breaks() {
        let dir = std::env::temp_dir().join(format!(
            "envelope-design-check-report-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src/pages")).unwrap();
        std::fs::write(
            dir.join("src/pages/Toolbar.tsx"),
            b"export const x = () => <button style={{ color: 'red' }}>go</button>",
        )
        .unwrap();

        let result = check(&dir, &["src/pages/Toolbar.tsx".to_string()]);
        let Err(detail) = result else {
            panic!("expected a violation, got Ok");
        };
        assert!(detail.contains("src/pages/Toolbar.tsx"), "{detail}");
        assert!(detail.contains("raw `<button`"), "{detail}");
        assert!(detail.contains("inline `style={{"), "{detail}");
        assert!(
            detail.contains("no import from the design system"),
            "{detail}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
