//! Language detection and per-language metadata (comment syntax, first-class
//! parser selection). This is a frozen shared contract used by every analyzer.

use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;

/// Languages with a bundled tree-sitter grammar (accurate structural analysis).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FirstClass {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Tsx,
    Go,
    Php,
}

pub const FIRST_CLASS_LANGUAGE_NAMES: &[&str] = &[
    "Rust",
    "Python",
    "JavaScript",
    "TypeScript",
    "TSX",
    "Go",
    "PHP",
];

pub const RECOGNIZED_LANGUAGE_NAMES: &[&str] = &[
    "Rust",
    "Python",
    "JavaScript",
    "TypeScript",
    "TSX",
    "Go",
    "PHP",
    "C",
    "C++",
    "C/C++ Header",
    "C#",
    "Java",
    "Kotlin",
    "Swift",
    "Ruby",
    "Shell",
    "Scala",
    "Haskell",
    "Lua",
    "SQL",
    "HTML",
    "CSS",
    "SCSS",
    "JSON",
    "YAML",
    "TOML",
    "Markdown",
    "XML",
    "Dockerfile",
    "Makefile",
    "Text",
];

/// Formats treated as authored program/build source by default. Repository
/// inventory still covers every recognized format; this list controls the
/// higher-signal health corpus used by actionable analyzers and rankings.
pub const SOURCE_LANGUAGE_NAMES: &[&str] = &[
    "Rust",
    "Python",
    "JavaScript",
    "TypeScript",
    "TSX",
    "Go",
    "PHP",
    "C",
    "C++",
    "C/C++ Header",
    "C#",
    "Java",
    "Kotlin",
    "Swift",
    "Ruby",
    "Shell",
    "Scala",
    "Haskell",
    "Lua",
    "SQL",
    "Dockerfile",
    "Makefile",
];

/// Recognized content formats excluded from health analyzers unless callers
/// opt in to one of them or select the all-content health scope.
pub const OPTIONAL_HEALTH_FORMAT_NAMES: &[&str] = &[
    "HTML", "CSS", "SCSS", "JSON", "YAML", "TOML", "Markdown", "XML", "Text",
];

/// Breadth of the actionable health corpus.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum HealthScope {
    /// Analyze authored program/build source and any explicitly included
    /// content formats.
    #[default]
    Source,
    /// Analyze every recognized format.
    All,
}

impl fmt::Display for HealthScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Source => "source",
            Self::All => "all",
        })
    }
}

/// A non-source format that can be added explicitly to the health corpus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum HealthInclude {
    Html,
    Css,
    Scss,
    Json,
    Yaml,
    Toml,
    Markdown,
    Xml,
    Text,
}

impl HealthInclude {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Css => "css",
            Self::Scss => "scss",
            Self::Json => "json",
            Self::Yaml => "yaml",
            Self::Toml => "toml",
            Self::Markdown => "markdown",
            Self::Xml => "xml",
            Self::Text => "text",
        }
    }

    pub fn language_name(self) -> &'static str {
        match self {
            Self::Html => "HTML",
            Self::Css => "CSS",
            Self::Scss => "SCSS",
            Self::Json => "JSON",
            Self::Yaml => "YAML",
            Self::Toml => "TOML",
            Self::Markdown => "Markdown",
            Self::Xml => "XML",
            Self::Text => "Text",
        }
    }
}

impl fmt::Display for HealthInclude {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Static metadata about a language used across analyzers.
#[derive(Debug, Clone, Copy)]
pub struct LangInfo {
    pub name: &'static str,
    pub first_class: Option<FirstClass>,
    pub line_comments: &'static [&'static str],
    pub block_comments: &'static [(&'static str, &'static str)],
}

impl LangInfo {
    pub fn is_first_class(&self) -> bool {
        self.first_class.is_some()
    }

    /// Whether this language has real control flow worth measuring for
    /// complexity. Prose, data, markup and style languages (Markdown, JSON,
    /// YAML, TOML, HTML, CSS, XML, …) return `false` so we don't compute
    /// meaningless cyclomatic/Halstead numbers over non-code text.
    pub fn is_code(&self) -> bool {
        self.first_class.is_some()
            || matches!(
                self.name,
                "C" | "C++"
                    | "C/C++ Header"
                    | "C#"
                    | "Java"
                    | "Kotlin"
                    | "Swift"
                    | "Ruby"
                    | "Shell"
                    | "Scala"
                    | "Haskell"
                    | "Lua"
                    | "SQL"
            )
    }

    /// Whether this format belongs to the concise, source-first health corpus.
    /// Build recipes participate even though they do not receive complexity.
    pub fn is_source(&self) -> bool {
        is_source_name(self.name)
    }
}

/// Classify a serialized language name without requiring a representative
/// path. Reporters use this to present source rows and one compact content
/// rollup from the same policy as the analyzers.
pub fn is_source_name(name: &str) -> bool {
    SOURCE_LANGUAGE_NAMES.contains(&name)
}

/// Decide whether a recognized format enters health analysis.
/// Inventory metrics deliberately do not use this policy.
pub fn included_in_health(info: &LangInfo, scope: HealthScope, includes: &[HealthInclude]) -> bool {
    scope == HealthScope::All
        || info.is_source()
        || includes
            .iter()
            .any(|include| include.language_name() == info.name)
}

macro_rules! lang {
    ($name:expr, $fc:expr, [$($lc:expr),*], [$($bo:expr => $bc:expr),*]) => {
        LangInfo {
            name: $name,
            first_class: $fc,
            line_comments: &[$($lc),*],
            block_comments: &[$(($bo, $bc)),*],
        }
    };
}

// First-class languages.
const RUST: LangInfo = lang!("Rust", Some(FirstClass::Rust), ["//"], ["/*" => "*/"]);
const PYTHON: LangInfo =
    lang!("Python", Some(FirstClass::Python), ["#"], ["\"\"\"" => "\"\"\"", "'''" => "'''"]);
const JAVASCRIPT: LangInfo =
    lang!("JavaScript", Some(FirstClass::JavaScript), ["//"], ["/*" => "*/"]);
const TYPESCRIPT: LangInfo =
    lang!("TypeScript", Some(FirstClass::TypeScript), ["//"], ["/*" => "*/"]);
const TSX: LangInfo = lang!("TSX", Some(FirstClass::Tsx), ["//"], ["/*" => "*/"]);
const GO: LangInfo = lang!("Go", Some(FirstClass::Go), ["//"], ["/*" => "*/"]);
const PHP: LangInfo = lang!("PHP", Some(FirstClass::Php), ["//", "#"], ["/*" => "*/"]);

// Generic languages (line/token metrics only).
const C: LangInfo = lang!("C", None, ["//"], ["/*" => "*/"]);
const CPP: LangInfo = lang!("C++", None, ["//"], ["/*" => "*/"]);
const CHEADER: LangInfo = lang!("C/C++ Header", None, ["//"], ["/*" => "*/"]);
const CSHARP: LangInfo = lang!("C#", None, ["//"], ["/*" => "*/"]);
const JAVA: LangInfo = lang!("Java", None, ["//"], ["/*" => "*/"]);
const KOTLIN: LangInfo = lang!("Kotlin", None, ["//"], ["/*" => "*/"]);
const SWIFT: LangInfo = lang!("Swift", None, ["//"], ["/*" => "*/"]);
const RUBY: LangInfo = lang!("Ruby", None, ["#"], ["=begin" => "=end"]);
const SHELL: LangInfo = lang!("Shell", None, ["#"], []);
const SCALA: LangInfo = lang!("Scala", None, ["//"], ["/*" => "*/"]);
const HASKELL: LangInfo = lang!("Haskell", None, ["--"], ["{-" => "-}"]);
const LUA: LangInfo = lang!("Lua", None, ["--"], ["--[[" => "]]"]);
const SQL: LangInfo = lang!("SQL", None, ["--"], ["/*" => "*/"]);
const HTML: LangInfo = lang!("HTML", None, [], ["<!--" => "-->"]);
const CSS: LangInfo = lang!("CSS", None, [], ["/*" => "*/"]);
const SCSS: LangInfo = lang!("SCSS", None, ["//"], ["/*" => "*/"]);
const JSON: LangInfo = lang!("JSON", None, [], []);
const YAML: LangInfo = lang!("YAML", None, ["#"], []);
const TOML_L: LangInfo = lang!("TOML", None, ["#"], []);
const MARKDOWN: LangInfo = lang!("Markdown", None, [], []);
const XML: LangInfo = lang!("XML", None, [], ["<!--" => "-->"]);
const DOCKERFILE: LangInfo = lang!("Dockerfile", None, ["#"], []);
const MAKEFILE: LangInfo = lang!("Makefile", None, ["#"], []);
const TEXT: LangInfo = lang!("Text", None, [], []);

/// Detect a language from a file path (by extension, then by file name).
pub fn detect(path: &Path) -> Option<&'static LangInfo> {
    if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "Dockerfile" || name.starts_with("Dockerfile."))
    {
        return Some(&DOCKERFILE);
    }

    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let info = match ext.to_ascii_lowercase().as_str() {
            "rs" => &RUST,
            "py" | "pyi" | "pyw" => &PYTHON,
            "js" | "mjs" | "cjs" | "jsx" => &JAVASCRIPT,
            "ts" | "mts" | "cts" => &TYPESCRIPT,
            "tsx" => &TSX,
            "go" => &GO,
            "c" => &C,
            "h" => &CHEADER,
            "cc" | "cpp" | "cxx" | "c++" => &CPP,
            "hh" | "hpp" | "hxx" => &CHEADER,
            "cs" => &CSHARP,
            "java" => &JAVA,
            "kt" | "kts" => &KOTLIN,
            "swift" => &SWIFT,
            "rb" => &RUBY,
            "php" | "php3" | "php4" | "php5" | "php7" | "php8" | "phps" | "phtml" | "phpt"
            | "ctp" | "inc" | "module" | "install" | "theme" | "profile" | "engine" => &PHP,
            "sh" | "bash" | "zsh" => &SHELL,
            "scala" | "sc" => &SCALA,
            "hs" => &HASKELL,
            "lua" => &LUA,
            "sql" => &SQL,
            "html" | "htm" => &HTML,
            "css" => &CSS,
            "scss" | "sass" => &SCSS,
            "json" | "jsonc" => &JSON,
            "yaml" | "yml" => &YAML,
            "toml" => &TOML_L,
            "md" | "markdown" => &MARKDOWN,
            "xml" => &XML,
            "txt" => &TEXT,
            _ => return None,
        };
        return Some(info);
    }

    match path.file_name().and_then(|n| n.to_str()) {
        Some("Dockerfile") => Some(&DOCKERFILE),
        Some("Makefile" | "makefile" | "GNUmakefile") => Some(&MAKEFILE),
        Some("artisan") => Some(&PHP),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_and_optional_health_formats_partition_recognized_formats() {
        let mut partition = SOURCE_LANGUAGE_NAMES
            .iter()
            .chain(OPTIONAL_HEALTH_FORMAT_NAMES)
            .copied()
            .collect::<Vec<_>>();
        partition.sort_unstable();
        partition.dedup();

        let mut recognized = RECOGNIZED_LANGUAGE_NAMES.to_vec();
        recognized.sort_unstable();

        assert_eq!(partition, recognized);
    }

    #[test]
    fn health_policy_is_source_first_with_explicit_content_opt_ins() {
        let rust = detect(Path::new("src/lib.rs")).unwrap();
        let makefile = detect(Path::new("Makefile")).unwrap();
        let json = detect(Path::new("fixture.json")).unwrap();

        assert!(included_in_health(rust, HealthScope::Source, &[]));
        assert!(included_in_health(makefile, HealthScope::Source, &[]));
        assert!(!included_in_health(json, HealthScope::Source, &[]));
        assert!(included_in_health(
            json,
            HealthScope::Source,
            &[HealthInclude::Json]
        ));
        assert!(included_in_health(json, HealthScope::All, &[]));
    }

    #[test]
    fn detects_common_php_source_extensions_as_first_class() {
        for path in [
            "index.php",
            "legacy.php5",
            "template.phtml",
            "extension.module",
            "package.install",
            "sample.phpt",
            "artisan",
        ] {
            let info = detect(Path::new(path)).unwrap();
            assert_eq!(info.name, "PHP", "{path}");
            assert_eq!(info.first_class, Some(FirstClass::Php), "{path}");
        }
    }

    #[test]
    fn detects_named_dockerfile_variants() {
        for path in [
            "Dockerfile",
            "docker/Dockerfile.nodejs.dev",
            "docker/Dockerfile.rust.dev",
        ] {
            assert_eq!(
                detect(Path::new(path)).map(|info| info.name),
                Some("Dockerfile")
            );
        }
    }
}
