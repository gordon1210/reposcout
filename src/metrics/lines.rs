//! Physical/source line counting with comment awareness.
//!
//! First-class languages use tree-sitter comment ranges, so comment delimiters
//! inside strings cannot corrupt later lines. Other formats use a lightweight,
//! quote-aware scanner and are marked approximate when comment syntax exists.

use crate::lang::LangInfo;
use tree_sitter::{Node, Tree};

#[derive(Debug, Clone, Copy, Default)]
pub struct LineStats {
    pub loc: usize,
    pub sloc: usize,
    pub comment_lines: usize,
    pub blank_lines: usize,
    pub approximate: bool,
}

#[must_use]
pub fn measure(lang: &LangInfo, content: &str, tree: Option<&Tree>) -> LineStats {
    if lang.first_class.is_some()
        && let Some(tree) = tree
    {
        return measure_from_tree(content, tree);
    }

    let mut stats = measure_fallback(lang, content);
    stats.approximate = !lang.line_comments.is_empty() || !lang.block_comments.is_empty();
    stats
}

fn measure_from_tree(content: &str, tree: &Tree) -> LineStats {
    let mut comment_ranges = Vec::new();
    collect_comment_ranges(tree.root_node(), &mut comment_ranges);
    comment_ranges.sort_unstable();

    let mut stats = LineStats::default();
    let mut line_start = 0usize;
    let mut range_index = 0usize;

    for chunk in content.split_inclusive('\n') {
        stats.loc += 1;
        let line = chunk.strip_suffix('\n').unwrap_or(chunk);
        let mut has_code = false;
        let mut has_comment = false;

        for (offset, ch) in line.char_indices() {
            if ch.is_whitespace() {
                continue;
            }
            let position = line_start + offset;
            while range_index < comment_ranges.len() && comment_ranges[range_index].1 <= position {
                range_index += 1;
            }
            if comment_ranges
                .get(range_index)
                .is_some_and(|(start, end)| *start <= position && position < *end)
            {
                has_comment = true;
            } else {
                has_code = true;
            }
        }

        classify_line(&mut stats, has_code, has_comment);
        line_start += chunk.len();
    }

    stats
}

fn collect_comment_ranges(root: Node<'_>, ranges: &mut Vec<(usize, usize)>) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind().contains("comment") {
            ranges.push((node.start_byte(), node.end_byte()));
            continue;
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            stack.push(child);
        }
    }
}

fn measure_fallback(lang: &LangInfo, content: &str) -> LineStats {
    let mut stats = LineStats::default();
    let mut block_close: Option<&'static str> = None;
    let mut multiline_quote: Option<u8> = None;

    for line in content.lines() {
        stats.loc += 1;
        if line.trim().is_empty() {
            stats.blank_lines += 1;
            continue;
        }

        let bytes = line.as_bytes();
        let mut index = 0usize;
        let mut has_code = false;
        let mut has_comment = false;

        while index < bytes.len() {
            if let Some(close) = block_close {
                has_comment = true;
                if let Some(offset) = find_bytes(bytes, index, close.as_bytes()) {
                    index = offset + close.len();
                    block_close = None;
                    continue;
                }
                break;
            }

            if let Some(quote) = multiline_quote {
                has_code = true;
                let (next, closed) = consume_quoted(bytes, index, quote);
                index = next;
                if closed {
                    multiline_quote = None;
                    continue;
                }
                break;
            }

            if bytes[index].is_ascii_whitespace() {
                index += 1;
                continue;
            }

            if starts_with_at(bytes, index, lang.line_comments) {
                has_comment = true;
                break;
            }

            if let Some((open, close)) = block_at(bytes, index, lang.block_comments) {
                has_comment = true;
                index += open.len();
                if let Some(offset) = find_bytes(bytes, index, close.as_bytes()) {
                    index = offset + close.len();
                } else {
                    block_close = Some(close);
                    break;
                }
                continue;
            }

            if matches!(bytes[index], b'\'' | b'"' | b'`') {
                has_code = true;
                let quote = bytes[index];
                let (next, closed) = consume_quoted(bytes, index + 1, quote);
                index = next;
                if !closed && quote == b'`' {
                    multiline_quote = Some(quote);
                    break;
                }
                continue;
            }

            has_code = true;
            index += 1;
        }

        classify_line(&mut stats, has_code, has_comment);
    }

    stats
}

fn classify_line(stats: &mut LineStats, has_code: bool, has_comment: bool) {
    if has_code {
        stats.sloc += 1;
    } else if has_comment {
        stats.comment_lines += 1;
    } else {
        stats.blank_lines += 1;
    }
}

fn starts_with_at(bytes: &[u8], index: usize, prefixes: &[&str]) -> bool {
    prefixes
        .iter()
        .any(|prefix| bytes[index..].starts_with(prefix.as_bytes()))
}

fn block_at(
    bytes: &[u8],
    index: usize,
    blocks: &[(&'static str, &'static str)],
) -> Option<(&'static str, &'static str)> {
    blocks
        .iter()
        .copied()
        .find(|(open, _)| bytes[index..].starts_with(open.as_bytes()))
}

fn find_bytes(haystack: &[u8], start: usize, needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || start > haystack.len() {
        return None;
    }
    haystack[start..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|offset| start + offset)
}

fn consume_quoted(bytes: &[u8], mut index: usize, quote: u8) -> (usize, bool) {
    let mut escaped = false;
    while index < bytes.len() {
        let byte = bytes[index];
        index += 1;
        if escaped {
            escaped = false;
        } else if byte == b'\\' {
            escaped = true;
        } else if byte == quote {
            return (index, true);
        }
    }
    (index, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::{FirstClass, detect};
    use crate::parse::parse;
    use std::path::Path;

    fn first_class(path: &str, language: FirstClass, source: &str) -> LineStats {
        let info = detect(Path::new(path)).expect("language");
        let tree = parse(language, source).expect("parse");
        measure(info, source, Some(&tree))
    }

    #[test]
    fn tree_ranges_ignore_comment_delimiters_inside_strings() {
        let source =
            "fn value() {\n    let marker = \"/*\";\n    let answer = 42;\n    answer\n}\n";
        let stats = first_class("sample.rs", FirstClass::Rust, source);

        assert_eq!(stats.loc, 5);
        assert_eq!(stats.sloc, 5);
        assert_eq!(stats.comment_lines, 0);
        assert!(!stats.approximate);
    }

    #[test]
    fn tree_ranges_distinguish_comment_only_and_mixed_lines() {
        let source = "// heading\nfn value() { /* note */\n    42\n}\n";
        let stats = first_class("sample.rs", FirstClass::Rust, source);

        assert_eq!(stats.loc, 4);
        assert_eq!(stats.sloc, 3);
        assert_eq!(stats.comment_lines, 1);
        assert_eq!(stats.blank_lines, 0);
    }

    #[test]
    fn php_tree_ranges_ignore_comment_tokens_inside_strings() {
        let source =
            "<?php\n// heading\n$value = '/* not a comment */';\n$value++; # note\n/* footer */\n";
        let stats = first_class("sample.php", FirstClass::Php, source);

        assert_eq!(stats.loc, 5);
        assert_eq!(stats.sloc, 3);
        assert_eq!(stats.comment_lines, 2);
        assert!(!stats.approximate);
    }

    #[test]
    fn fallback_is_quote_aware_but_explicitly_approximate() {
        let info = detect(Path::new("sample.c")).expect("language");
        let source = "const char *marker = \"/*\";\nint answer = 42;\n/* comment */\n";
        let stats = measure(info, source, None);

        assert_eq!(stats.sloc, 2);
        assert_eq!(stats.comment_lines, 1);
        assert!(stats.approximate);
    }

    #[test]
    fn formats_without_comment_syntax_have_exact_nonblank_counts() {
        let info = detect(Path::new("sample.json")).expect("language");
        let stats = measure(info, "{\n\n  \"value\": 1\n}\n", None);

        assert_eq!(stats.loc, 4);
        assert_eq!(stats.sloc, 3);
        assert_eq!(stats.blank_lines, 1);
        assert!(!stats.approximate);
    }
}
