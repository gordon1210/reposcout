//! Scan for annotation markers (TODO, FIXME, HACK, ...). Matches are
//! word-boundary aware to avoid counting substrings inside identifiers.

use crate::model::MarkerOccurrence;
use std::collections::BTreeMap;
use std::ops::Range;
use tree_sitter::{Node, Tree};
use xxhash_rust::xxh3::xxh3_64;

#[derive(Debug, Clone, Default)]
pub struct MarkerScan {
    pub counts: BTreeMap<String, usize>,
    pub occurrences: Vec<MarkerOccurrence>,
}

pub fn scan(content: &str, markers: &[String]) -> BTreeMap<String, usize> {
    scan_detailed(content, markers).counts
}

pub fn scan_detailed(content: &str, markers: &[String]) -> MarkerScan {
    scan_ranges(content, markers, std::iter::once(0..content.len()))
}

/// Scan marker occurrences only inside syntax-tree comment nodes.
///
/// Callers should use this for first-class languages after parsing and keep
/// [`scan_detailed`] as the fallback for formats without a syntax tree. Tree
/// sitter still exposes intact comments around most syntax errors, so a
/// partially malformed file remains useful without treating identifiers or
/// string literals as annotations.
pub fn scan_detailed_in_tree(content: &str, markers: &[String], tree: &Tree) -> MarkerScan {
    let mut ranges = Vec::new();
    collect_comment_ranges(tree.root_node(), &mut ranges);
    ranges.sort_unstable_by_key(|range| range.start);
    scan_ranges(content, markers, ranges.into_iter())
}

fn scan_ranges(
    content: &str,
    markers: &[String],
    ranges: impl Iterator<Item = Range<usize>> + Clone,
) -> MarkerScan {
    let mut counts = BTreeMap::new();
    let mut occurrences = Vec::new();
    let mut ordinals: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut line_starts = vec![0usize];
    line_starts.extend(
        content
            .match_indices('\n')
            .map(|(index, _)| index.saturating_add(1)),
    );

    for marker in markers {
        if marker.is_empty() {
            continue;
        }
        let positions = ranges
            .clone()
            .flat_map(|range| {
                word_positions(&content[range.clone()], marker)
                    .map(move |position| range.start + position)
            })
            .collect::<Vec<_>>();
        if !positions.is_empty() {
            counts.insert(marker.clone(), positions.len());
        }
        for position in positions {
            let line_index = line_starts.partition_point(|start| *start <= position) - 1;
            let line_start = line_starts[line_index];
            let line_end = content[position..]
                .find('\n')
                .map(|offset| position + offset)
                .unwrap_or(content.len());
            let normalized = content[line_start..line_end]
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let context_hash = format!("{:016x}", xxh3_64(normalized.as_bytes()));
            let ordinal = ordinals
                .entry((marker.clone(), context_hash.clone()))
                .or_default();
            *ordinal += 1;
            occurrences.push(MarkerOccurrence {
                marker: marker.clone(),
                line: line_index + 1,
                column: content[line_start..position].chars().count() + 1,
                context_hash,
                occurrence: *ordinal,
            });
        }
    }
    occurrences.sort_by(|a, b| {
        a.line
            .cmp(&b.line)
            .then(a.column.cmp(&b.column))
            .then(a.marker.cmp(&b.marker))
    });
    MarkerScan {
        counts,
        occurrences,
    }
}

fn collect_comment_ranges(node: Node<'_>, ranges: &mut Vec<Range<usize>>) {
    if is_comment_kind(node.kind()) {
        ranges.push(node.byte_range());
        return;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_comment_ranges(child, ranges);
    }
}

fn is_comment_kind(kind: &str) -> bool {
    kind == "comment" || kind.ends_with("_comment")
}

fn word_positions<'a>(content: &'a str, marker: &'a str) -> impl Iterator<Item = usize> + 'a {
    let bytes = content.as_bytes();
    content
        .match_indices(marker)
        .filter(|(pos, _)| {
            let p = *pos;
            let before_ok = p == 0 || !is_word_byte(bytes[p - 1]);
            let a = p + marker.len();
            let after_ok = a >= bytes.len() || !is_word_byte(bytes[a]);
            before_ok && after_ok
        })
        .map(|(position, _)| position)
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[cfg(test)]
mod tests {
    use super::{scan_detailed, scan_detailed_in_tree};
    use crate::lang::FirstClass;
    use crate::parse;

    #[test]
    fn detailed_scan_reports_stable_precise_occurrences() {
        let markers = vec!["TODO".to_string()];
        let scan = scan_detailed("// TODO first\n  // TODO first\n", &markers);

        assert_eq!(scan.counts["TODO"], 2);
        assert_eq!(scan.occurrences[0].line, 1);
        assert_eq!(scan.occurrences[0].column, 4);
        assert_eq!(scan.occurrences[1].line, 2);
        assert_eq!(scan.occurrences[1].column, 6);
        assert_eq!(
            scan.occurrences[0].context_hash,
            scan.occurrences[1].context_hash
        );
        assert_eq!(scan.occurrences[1].occurrence, 2);
    }

    #[test]
    fn syntax_aware_scan_ignores_rust_strings_and_identifiers() {
        assert_only_comment_markers(
            FirstClass::Rust,
            "const TODO: &str = \"TODO\";\n// TODO real\n/* FIXME real */\n",
        );
    }

    #[test]
    fn syntax_aware_scan_ignores_python_strings_and_identifiers() {
        assert_only_comment_markers(
            FirstClass::Python,
            "TODO = 'TODO'\n\"\"\"FIXME documentation\"\"\"\n# TODO real\n# FIXME real\n",
        );
    }

    #[test]
    fn syntax_aware_scan_ignores_javascript_strings_and_identifiers() {
        assert_only_comment_markers(
            FirstClass::JavaScript,
            "const TODO = 'TODO';\n// TODO real\n/* FIXME real */\n",
        );
    }

    #[test]
    fn syntax_aware_scan_ignores_typescript_strings_and_identifiers() {
        assert_only_comment_markers(
            FirstClass::TypeScript,
            "const TODO: string = `TODO`;\n// TODO real\n/* FIXME real */\n",
        );
    }

    #[test]
    fn syntax_aware_scan_handles_tsx_comments() {
        assert_only_comment_markers(
            FirstClass::Tsx,
            "const TODO = <div>TODO{/* TODO real */}</div>;\n// FIXME real\n",
        );
    }

    #[test]
    fn syntax_aware_scan_ignores_go_strings_and_identifiers() {
        assert_only_comment_markers(
            FirstClass::Go,
            "package main\nconst TODO = `TODO`\n// TODO real\n/* FIXME real */\n",
        );
    }

    #[test]
    fn syntax_aware_scan_ignores_php_strings_and_names() {
        assert_only_comment_markers(
            FirstClass::Php,
            "<?php\n$TODO = 'TODO';\n// TODO real\n/* FIXME real */\n",
        );
    }

    #[test]
    fn syntax_aware_scan_retains_comments_around_parse_errors() {
        let content = "fn broken( {\nlet TODO = \"TODO\";\n// TODO real\n";
        let tree = parse::parse(FirstClass::Rust, content).unwrap();
        assert!(tree.root_node().has_error());

        let scan = scan_detailed_in_tree(content, &["TODO".to_string()], &tree);
        assert_eq!(scan.counts["TODO"], 1);
        assert_eq!(scan.occurrences[0].line, 3);
    }

    fn assert_only_comment_markers(language: FirstClass, content: &str) {
        let markers = vec!["TODO".to_string(), "FIXME".to_string()];
        let tree = parse::parse(language, content).unwrap();
        let scan = scan_detailed_in_tree(content, &markers, &tree);

        assert_eq!(scan.counts.get("TODO"), Some(&1), "{language:?}: {content}");
        assert_eq!(
            scan.counts.get("FIXME"),
            Some(&1),
            "{language:?}: {content}"
        );
        assert_eq!(scan.occurrences.len(), 2, "{language:?}: {content}");
    }
}
