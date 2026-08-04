//! Classify files as skip candidates (generated, minified, bundled, or vendored).
//!
//! Returns a short reason string so callers can surface these files to users
//! without requiring them to open each file to discover machine-produced or
//! vendored noise.

const GENERATED_SUFFIXES: &[&str] = &[
    ".pb.go",
    "_pb2.py",
    ".pb.cc",
    ".pb.h",
    ".g.dart",
    ".freezed.dart",
    ".designer.cs",
    ".generated.ts",
];
const VENDORED_DIRS: &[&str] = &[
    "vendor",
    "node_modules",
    "third_party",
    "third-party",
    "dist",
    "build",
    ".next",
    "out",
];
const BUNDLED_DIRS: &[&str] = &["dist", "build", ".next", ".nuxt", ".svelte-kit", "out"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Classification {
    hint: Option<&'static str>,
    duplication_artifact: bool,
}

impl Classification {
    #[must_use]
    pub(crate) fn skip_hint(self) -> Option<&'static str> {
        self.hint
    }

    #[must_use]
    pub(crate) fn is_duplication_artifact(self) -> bool {
        self.duplication_artifact
    }
}

/// Classify source that is likely machine-produced or otherwise a poor
/// reading candidate. Minified and bundled build output are additionally
/// marked as duplication artifacts so scan orchestration can keep them out of
/// the default detector corpus without coupling to human-readable hint text.
#[must_use]
pub(crate) fn classify(rel_path: &str, content: &str) -> Classification {
    let filename = rel_path.rsplit(['/', '\\']).next().unwrap_or(rel_path);
    if filename.to_ascii_lowercase().contains(".min.") || looks_minified(content) {
        return Classification {
            hint: Some("minified"),
            duplication_artifact: true,
        };
    }
    if is_bundled_path(rel_path) {
        return Classification {
            hint: Some("bundled"),
            duplication_artifact: true,
        };
    }

    for suffix in GENERATED_SUFFIXES {
        if rel_path.ends_with(suffix) {
            return Classification {
                hint: Some("generated"),
                duplication_artifact: false,
            };
        }
    }
    for line in content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .take(5)
    {
        if is_generated_header(line) {
            return Classification {
                hint: Some("generated"),
                duplication_artifact: false,
            };
        }
    }

    let normalized = rel_path.replace('\\', "/");
    let mut components = normalized.split('/').collect::<Vec<_>>();
    components.pop();
    if components
        .iter()
        .any(|component| VENDORED_DIRS.contains(component))
    {
        return Classification {
            hint: Some("vendored"),
            duplication_artifact: false,
        };
    }

    Classification {
        hint: None,
        duplication_artifact: false,
    }
}

/// Return a reason string if `rel_path`/`content` looks like a file an agent
/// should skip, or `None` if it appears to be hand-authored source code.
///
/// Checks are applied in priority order: minified → bundled → generated → vendored.
#[must_use]
pub fn skip_hint(rel_path: &str, content: &str) -> Option<String> {
    classify(rel_path, content).skip_hint().map(str::to_string)
}

/// Interpret a cached hint without rescanning its source content.
#[must_use]
pub(crate) fn hint_is_duplication_artifact(hint: Option<&str>) -> bool {
    matches!(hint, Some("minified" | "bundled"))
}

fn looks_minified(content: &str) -> bool {
    let mut line_count = 0usize;
    let mut total = 0usize;
    for line in content.lines() {
        line_count = line_count.saturating_add(1);
        total = total.saturating_add(line.len());
        if line.len() > 2_000 {
            return true;
        }
    }
    line_count > 1 && total > line_count.saturating_mul(250)
}

fn is_bundled_path(rel_path: &str) -> bool {
    let normalized = rel_path.replace('\\', "/").to_ascii_lowercase();
    let components = normalized.split('/').collect::<Vec<_>>();
    let Some(filename) = components.last().copied() else {
        return false;
    };
    if !matches!(
        filename.rsplit_once('.').map(|(_, extension)| extension),
        Some("js" | "mjs" | "cjs" | "css")
    ) {
        return false;
    }
    let directories = &components[..components.len().saturating_sub(1)];
    filename.contains(".chunk.")
        || filename.contains(".bundle.")
        || is_chunk_filename(filename)
        || directories
            .iter()
            .any(|component| BUNDLED_DIRS.contains(component))
        || directories
            .windows(2)
            .any(|pair| matches!(pair, ["static", "chunks"]))
}

fn is_chunk_filename(filename: &str) -> bool {
    let stem = filename.rsplit_once('.').map_or(filename, |(stem, _)| stem);
    if matches!(stem, "chunk-vendors" | "chunk-common") {
        return true;
    }
    let Some(identifier) = stem
        .strip_prefix("chunk-")
        .or_else(|| stem.strip_prefix("chunk_"))
    else {
        return false;
    };
    let allowed = identifier
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'~'));
    let hash_like =
        identifier.len() >= 6 && identifier.bytes().all(|byte| byte.is_ascii_hexdigit());
    identifier.len() >= 6
        && allowed
        && (identifier.bytes().any(|byte| byte.is_ascii_digit()) || hash_like)
}

/// Match canonical generated-file comment headers, not incidental prose such
/// as documentation describing generated code.
fn is_generated_header(line: &str) -> bool {
    let line = line.trim_start();
    let comment = line
        .strip_prefix("//")
        .or_else(|| line.strip_prefix('#'))
        .or_else(|| line.strip_prefix("/*"))
        .map(str::trim_start);
    let Some(comment) = comment else {
        return false;
    };
    let lower = comment.to_ascii_lowercase();
    lower.starts_with("@generated")
        || lower.starts_with("code generated by")
        || lower.starts_with("this file was generated")
        || lower.starts_with("this file is generated")
        || lower.starts_with("automatically generated")
        || lower.starts_with("auto-generated")
        || (lower.starts_with("generated") && lower.contains("do not edit"))
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_single_line_is_minified() {
        let long_line = "a".repeat(2001);
        assert_eq!(
            skip_hint("bundle.js", &long_line),
            Some("minified".to_string())
        );
    }

    #[test]
    fn min_dot_filename_is_minified() {
        assert_eq!(
            skip_hint("jquery.min.js", "x=1"),
            Some("minified".to_string())
        );
    }

    #[test]
    fn minified_files_are_duplication_artifacts() {
        let long_line = "const value=1;".repeat(200);
        let classification = classify("assets/bundle.js", &long_line);

        assert_eq!(classification.skip_hint(), Some("minified"));
        assert!(classification.is_duplication_artifact());
    }

    #[test]
    fn chunk_outputs_are_duplication_artifacts() {
        for path in [
            "static/js/main.a1b2c3.chunk.js",
            "public/js/main.bundle.js",
            "public/js/chunk-a1b2c3d4.js",
            "public/js/chunk-abcdef.js",
            "public/js/chunk-abcdefab.js",
            ".next/static/chunks/1234.js",
            "packages/web/dist/assets/index-a1b2c3.js",
            "packages/web/build/static/js/main-a1b2c3.js",
        ] {
            let classification = classify(path, "export const value = 1;\n");

            assert_eq!(
                classification.skip_hint(),
                Some("bundled"),
                "unexpected hint for {path}"
            );
            assert!(
                classification.is_duplication_artifact(),
                "expected {path} to be excluded from duplication"
            );
        }
    }

    #[test]
    fn bundled_outputs_inside_vendored_trees_remain_duplication_artifacts() {
        for path in [
            "vendor/lodash/dist/lodash.js",
            "node_modules/example/build/styles.css",
        ] {
            let classification = classify(path, "export const value = 1;\n");

            assert_eq!(classification.skip_hint(), Some("bundled"));
            assert!(classification.is_duplication_artifact());
        }
    }

    #[test]
    fn ordinary_chunk_named_source_is_not_a_duplication_artifact() {
        for path in ["src/chunk-parser.js", "src/chunk-filter.js"] {
            let classification = classify(
                path,
                "export function parseChunk(value) { return value; }\n",
            );

            assert_eq!(
                classification.skip_hint(),
                None,
                "unexpected hint for {path}"
            );
            assert!(!classification.is_duplication_artifact());
        }
    }

    #[test]
    fn generated_hints_do_not_change_duplication_scope() {
        let classification = classify("api/types.pb.go", "package api\n");

        assert_eq!(classification.skip_hint(), Some("generated"));
        assert!(!classification.is_duplication_artifact());
    }

    #[test]
    fn generated_comment_header_is_detected() {
        let content = "// Code generated by protoc. DO NOT EDIT.\nfoo();";
        // Use a plain .go path so the path-suffix check doesn't fire first.
        assert_eq!(skip_hint("foo.go", content), Some("generated".to_string()));
    }

    #[test]
    fn prose_about_generated_code_is_not_a_generated_header() {
        let content =
            "//! Classify files as skip candidates.\n//! Auto-generated code can be noisy.";
        assert_eq!(skip_hint("src/classify.rs", content), None);
    }

    #[test]
    fn generated_path_suffix_is_detected() {
        assert_eq!(
            skip_hint("api/types.pb.go", "package api"),
            Some("generated".to_string())
        );
    }

    #[test]
    fn vendored_path_is_detected() {
        assert_eq!(
            skip_hint("node_modules/foo/bar.js", "let x = 1;"),
            Some("vendored".to_string())
        );
    }

    #[test]
    fn similar_named_dirs_are_not_vendored() {
        // Whole-segment matching: these contain vendor-dir substrings but are
        // ordinary source directories, so they must NOT be flagged.
        assert_eq!(skip_hint("src/rebuild/step.rs", "fn f() {}"), None);
        assert_eq!(skip_hint("app/layout/main.ts", "const x = 1;"), None);
        assert_eq!(skip_hint("workout/plan.py", "x = 1"), None);
    }

    #[test]
    fn ordinary_code_is_none() {
        let content = "fn main() { println!(\"hello\"); }";
        assert_eq!(skip_hint("src/main.rs", content), None);
    }
}
