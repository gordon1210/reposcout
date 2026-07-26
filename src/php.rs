//! Shared PHP syntax helpers used by per-file metrics and repository topology.
//!
//! The tree-sitter grammar identifies the enclosing declaration/expression; these
//! helpers normalize the small pieces of PHP syntax whose meaning is shared by
//! more than one analyzer.

use serde::{Deserialize, Serialize};
use tree_sitter::Node;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum StaticInclude {
    /// A quoted path whose runtime base is not explicit. The graph resolver
    /// tries the importing directory and repository root and rejects ambiguity.
    Literal(String),
    /// A path rooted at `__DIR__`, optionally wrapped in `dirname(...)`.
    DirectoryRelative { parents: usize, path: String },
}

/// Return fully-qualified names introduced by one `use` declaration.
pub(crate) fn use_namespaces(node: Node<'_>, source: &str) -> Vec<String> {
    if node.kind() != "namespace_use_declaration" {
        return Vec::new();
    }

    let Ok(text) = node.utf8_text(source.as_bytes()) else {
        return Vec::new();
    };
    parse_use_declaration(text)
}

/// Return the leading namespace/package component from a PHP symbol name.
pub(crate) fn namespace_root(symbol: &str) -> Option<String> {
    symbol
        .trim()
        .trim_start_matches('\\')
        .split('\\')
        .next()
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
}

/// Evaluate the common static forms of `include`/`require` expressions.
/// Dynamic expressions are deliberately ignored instead of guessed.
pub(crate) fn static_include(node: Node<'_>, source: &str) -> Option<StaticInclude> {
    if !matches!(
        node.kind(),
        "include_expression"
            | "include_once_expression"
            | "require_expression"
            | "require_once_expression"
    ) {
        return None;
    }
    let expression = node.named_child(0)?;
    let text = expression.utf8_text(source.as_bytes()).ok()?.trim();
    parse_static_include(text)
}

fn parse_static_include(text: &str) -> Option<StaticInclude> {
    let text = strip_wrapping_parentheses(text.trim());
    if let Some(path) = quoted_path(text) {
        return Some(StaticInclude::Literal(path));
    }

    let (parents, remainder) = directory_base(text)?;
    let mut path = String::new();
    let mut remainder = remainder.trim();
    while !remainder.is_empty() {
        remainder = remainder.strip_prefix('.')?.trim_start();
        let (segment, rest) = take_quoted(remainder)?;
        path.push_str(&segment);
        remainder = rest.trim_start();
    }
    Some(StaticInclude::DirectoryRelative { parents, path })
}

fn directory_base(text: &str) -> Option<(usize, &str)> {
    if let Some(remainder) = text.strip_prefix("__DIR__") {
        return Some((0, remainder));
    }

    let mut parents = 0usize;
    let mut current = text;
    while let Some(inner) = current.strip_prefix("dirname(") {
        parents += 1;
        current = inner;
    }
    let current = current.strip_prefix("__DIR__")?;
    let mut remainder = current;
    for _ in 0..parents {
        remainder = remainder.strip_prefix(')')?;
    }
    Some((parents, remainder))
}

fn strip_wrapping_parentheses(mut text: &str) -> &str {
    loop {
        let Some(inner) = text
            .strip_prefix('(')
            .and_then(|value| value.strip_suffix(')'))
        else {
            return text;
        };
        text = inner.trim();
    }
}

fn quoted_path(text: &str) -> Option<String> {
    let (path, rest) = take_quoted(text)?;
    rest.trim().is_empty().then_some(path)
}

fn take_quoted(text: &str) -> Option<(String, &str)> {
    let quote = text.as_bytes().first().copied()?;
    if !matches!(quote, b'\'' | b'"') {
        return None;
    }
    let mut escaped = false;
    for (offset, byte) in text.as_bytes().iter().copied().enumerate().skip(1) {
        if escaped {
            escaped = false;
            continue;
        }
        if byte == b'\\' {
            escaped = true;
        } else if byte == quote {
            let raw = &text[1..offset];
            if quote == b'"' && (raw.contains('$') || raw.contains('{')) {
                return None;
            }
            let value = raw.replace("\\\\", "\\").replace(
                if quote == b'\'' { "\\'" } else { "\\\"" },
                &char::from(quote).to_string(),
            );
            return Some((value, &text[offset + 1..]));
        }
    }
    None
}

fn parse_use_declaration(text: &str) -> Vec<String> {
    let mut declaration = text.trim();
    declaration = declaration
        .strip_prefix("use")
        .unwrap_or(declaration)
        .trim();
    declaration = declaration.trim_end_matches(';').trim();
    declaration = strip_import_kind(declaration);

    if let (Some(open), Some(close)) = (declaration.find('{'), declaration.rfind('}'))
        && open < close
    {
        let prefix = declaration[..open].trim().trim_end_matches('\\');
        return split_top_level(&declaration[open + 1..close])
            .into_iter()
            .filter_map(imported_name)
            .map(|member| {
                if prefix.is_empty() {
                    member
                } else {
                    format!("{prefix}\\{member}")
                }
            })
            .collect();
    }

    split_top_level(declaration)
        .into_iter()
        .filter_map(imported_name)
        .collect()
}

fn split_top_level(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0_u32;
    let mut start = 0;

    for (index, ch) in text.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(text[start..index].trim());
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(text[start..].trim());
    parts
}

fn imported_name(part: &str) -> Option<String> {
    let part = strip_import_kind(part.trim());
    let name = part
        .split_ascii_whitespace()
        .next()
        .unwrap_or_default()
        .trim()
        .trim_start_matches('\\');
    (!name.is_empty()).then(|| name.to_string())
}

fn strip_import_kind(text: &str) -> &str {
    text.strip_prefix("function ")
        .or_else(|| text.strip_prefix("const "))
        .unwrap_or(text)
        .trim()
}

#[cfg(test)]
mod tests {
    use super::{StaticInclude, namespace_root, parse_static_include, parse_use_declaration};

    #[test]
    fn parses_simple_aliased_and_grouped_uses() {
        assert_eq!(
            parse_use_declaration(
                "use Symfony\\Component\\HttpFoundation\\Request as HttpRequest, Psr\\Log\\LoggerInterface;"
            ),
            [
                "Symfony\\Component\\HttpFoundation\\Request",
                "Psr\\Log\\LoggerInterface"
            ]
        );
        assert_eq!(
            parse_use_declaration(
                "use Vendor\\Package\\{Client, Contracts\\Driver as DriverContract};"
            ),
            [
                "Vendor\\Package\\Client",
                "Vendor\\Package\\Contracts\\Driver"
            ]
        );
        assert_eq!(
            parse_use_declaration("use function Framework\\Helpers\\route;"),
            ["Framework\\Helpers\\route"]
        );
    }

    #[test]
    fn extracts_namespace_roots() {
        assert_eq!(
            namespace_root("\\Symfony\\Console\\Application"),
            Some("Symfony".into())
        );
        assert_eq!(namespace_root(""), None);
    }

    #[test]
    fn evaluates_static_include_forms_without_guessing_dynamic_paths() {
        assert_eq!(
            parse_static_include("'bootstrap/app.php'"),
            Some(StaticInclude::Literal("bootstrap/app.php".into()))
        );
        assert_eq!(
            parse_static_include("__DIR__ . '/../config/app.php'"),
            Some(StaticInclude::DirectoryRelative {
                parents: 0,
                path: "/../config/app.php".into(),
            })
        );
        assert_eq!(
            parse_static_include("dirname(__DIR__) . '/bootstrap.php'"),
            Some(StaticInclude::DirectoryRelative {
                parents: 1,
                path: "/bootstrap.php".into(),
            })
        );
        assert_eq!(parse_static_include("$base . '/dynamic.php'"), None);
        assert_eq!(parse_static_include("\"$base/file.php\""), None);
    }
}
