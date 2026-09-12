use crate::model::{
    DefinitionFacts, DefinitionStatus, LexicalDefinitionFacts, LexicalField, LexicalFieldTerms,
    LexicalFileFacts, LexicalStatus, SourceSpan,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;
use tree_sitter::{Node, Tree};

/// Maximum declarations retained for lexical extraction from one file.
pub const MAX_DEFINITIONS_PER_FILE: usize = 2_048;
/// Maximum distinct normalized terms retained in one declaration search field.
pub const MAX_TERMS_PER_FIELD: usize = 128;
/// Maximum Unicode scalar values retained in one normalized search term.
pub const MAX_TERM_CHARS: usize = 128;
/// Maximum source bytes inspected for one declaration's code search field.
pub const MAX_CODE_BYTES_PER_DEFINITION: usize = 4_096;
/// Maximum comment bytes inspected for one declaration's comment search field.
pub const MAX_COMMENT_BYTES_PER_DEFINITION: usize = 2_048;
/// Maximum comment syntax nodes inspected for lexical extraction in one file.
pub const MAX_COMMENT_NODES_PER_FILE: usize = 4_096;
/// Maximum syntax nodes visited during lexical extraction from one file.
pub const MAX_SYNTAX_NODES_PER_FILE: usize = 100_000;

/// Extract bounded field-specific search terms from the supplied source and shared declaration facts without I/O.
#[must_use]
pub fn extract(
    path: &Path,
    language: &str,
    content: &str,
    tree: Option<&Tree>,
    definitions: &DefinitionFacts,
) -> LexicalFileFacts {
    let (comments, comment_scan_truncated) = tree.map_or_else(
        || (Vec::new(), false),
        |tree| collect_comments(tree.root_node()),
    );
    let definitions_total = definitions.definitions.len();
    let retained = definitions_total.min(MAX_DEFINITIONS_PER_FILE);
    let path_terms = tokenize(&path.to_string_lossy());
    let mut facts = Vec::with_capacity(retained);
    for definition in definitions.definitions.iter().take(retained) {
        let name = tokenize(&definition.symbol.name);
        let mut signature = tokenize(&definition.symbol.signature);
        let name_terms = name.0.iter().map(String::as_str).collect::<BTreeSet<_>>();
        signature
            .0
            .retain(|term| !name_terms.contains(term.as_str()));
        let span = definition
            .source_span
            .unwrap_or(definition.declaration_span);
        let (comment, comment_field_truncated) =
            comment_terms(content, span, definition.declaration_span, &comments);
        let (mut code, code_truncated) = code_terms(content, span, &comments);
        let declaration_terms = name
            .0
            .iter()
            .chain(&signature.0)
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        code.0
            .retain(|term| !declaration_terms.contains(term.as_str()));
        facts.push(LexicalDefinitionFacts {
            definition: definition.clone(),
            fields: vec![
                field_terms(LexicalField::Name, name, false),
                field_terms(LexicalField::Path, path_terms.clone(), false),
                field_terms(LexicalField::Signature, signature, false),
                field_terms(
                    LexicalField::Comment,
                    comment,
                    comment_field_truncated || comment_scan_truncated,
                ),
                field_terms(LexicalField::Code, code, code_truncated),
            ],
        });
    }
    LexicalFileFacts {
        language: language.to_string(),
        sha256: source_hash(content),
        status: match definitions.status {
            DefinitionStatus::Available => LexicalStatus::Inspected,
            DefinitionStatus::ParseErrors => LexicalStatus::ParseErrors,
            DefinitionStatus::Unsupported => LexicalStatus::Unsupported,
            DefinitionStatus::Unavailable => LexicalStatus::Unavailable,
        },
        definitions_total,
        definitions_omitted: definitions_total.saturating_sub(retained),
        definitions: facts,
    }
}

fn field_terms(
    field: LexicalField,
    (terms, truncated): (Vec<String>, bool),
    additionally_truncated: bool,
) -> LexicalFieldTerms {
    LexicalFieldTerms {
        field,
        terms,
        truncated: truncated || additionally_truncated,
    }
}

fn source_hash(content: &str) -> String {
    Sha256::digest(content.as_bytes())
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            let _ = write!(output, "{byte:02x}");
            output
        })
}

fn collect_comments(root: Node<'_>) -> (Vec<SourceSpan>, bool) {
    let mut comments = Vec::new();
    let mut stack = vec![root];
    let mut visited = 0usize;
    let mut truncated = false;
    while let Some(node) = stack.pop() {
        visited += 1;
        if visited > MAX_SYNTAX_NODES_PER_FILE {
            truncated = true;
            break;
        }
        if is_comment(node.kind()) {
            if comments.len() == MAX_COMMENT_NODES_PER_FILE {
                truncated = true;
                continue;
            }
            comments.push(SourceSpan {
                start_byte: node.start_byte(),
                end_byte: node.end_byte(),
                start_line: node.start_position().row + 1,
                end_line: node.end_position().row + 1,
            });
            continue;
        }
        let mut cursor = node.walk();
        let mut children = node.children(&mut cursor).collect::<Vec<_>>();
        children.reverse();
        stack.extend(children);
    }
    comments.sort_by_key(|span| (span.start_byte, span.end_byte));
    (comments, truncated)
}

fn is_comment(kind: &str) -> bool {
    kind == "comment" || kind.ends_with("_comment")
}

fn comment_terms(
    content: &str,
    span: SourceSpan,
    declaration: SourceSpan,
    comments: &[SourceSpan],
) -> ((Vec<String>, bool), bool) {
    let mut selected = comments
        .iter()
        .copied()
        .filter(|comment| ranges_overlap(*comment, span))
        .collect::<Vec<_>>();
    let mut cursor = declaration.start_byte;
    for comment in comments.iter().rev().copied() {
        if comment.end_byte > cursor {
            continue;
        }
        let Some(gap) = content.get(comment.end_byte..cursor) else {
            break;
        };
        if !gap.chars().all(char::is_whitespace) {
            break;
        }
        selected.push(comment);
        cursor = comment.start_byte;
    }
    selected.sort_by_key(|comment| (comment.start_byte, comment.end_byte));
    selected.dedup_by_key(|comment| (comment.start_byte, comment.end_byte));

    let mut bytes = 0usize;
    let mut values = Vec::new();
    let mut byte_truncated = false;
    for comment in selected {
        if bytes >= MAX_COMMENT_BYTES_PER_DEFINITION {
            byte_truncated = true;
            break;
        }
        let remaining = MAX_COMMENT_BYTES_PER_DEFINITION - bytes;
        let Some(value) = bounded_slice(content, comment.start_byte, comment.end_byte, remaining)
        else {
            byte_truncated = true;
            continue;
        };
        bytes += value.len();
        byte_truncated |= value.len() < comment.end_byte.saturating_sub(comment.start_byte);
        values.push(value);
    }
    let mut terms = TermCollector::default();
    for value in values {
        terms.add_text(value);
    }
    let (terms, term_truncated) = terms.finish();
    ((terms, term_truncated), byte_truncated)
}

fn code_terms(
    content: &str,
    span: SourceSpan,
    comments: &[SourceSpan],
) -> ((Vec<String>, bool), bool) {
    let Some(end) = bounded_end(
        content,
        span.start_byte,
        span.end_byte,
        MAX_CODE_BYTES_PER_DEFINITION,
    ) else {
        return ((Vec::new(), false), true);
    };
    let byte_truncated = end < span.end_byte;
    let mut cursor = span.start_byte;
    let mut terms = TermCollector::default();
    for comment in comments
        .iter()
        .copied()
        .filter(|comment| comment.start_byte < end && comment.end_byte > span.start_byte)
    {
        let comment_start = comment.start_byte.clamp(span.start_byte, end);
        if cursor < comment_start
            && let Some(value) = content.get(cursor..comment_start)
        {
            terms.add_text(value);
        }
        cursor = cursor.max(comment.end_byte.min(end));
    }
    if cursor < end
        && let Some(value) = content.get(cursor..end)
    {
        terms.add_text(value);
    }
    let (terms, term_truncated) = terms.finish();
    ((terms, term_truncated), byte_truncated)
}

fn ranges_overlap(left: SourceSpan, right: SourceSpan) -> bool {
    left.start_byte < right.end_byte && left.end_byte > right.start_byte
}

fn bounded_slice(content: &str, start: usize, end: usize, max_bytes: usize) -> Option<&str> {
    let end = bounded_end(content, start, end, max_bytes)?;
    content.get(start..end)
}

fn bounded_end(content: &str, start: usize, end: usize, max_bytes: usize) -> Option<usize> {
    if start > end || end > content.len() || !content.is_char_boundary(start) {
        return None;
    }
    let mut bounded = end.min(start.saturating_add(max_bytes));
    while bounded > start && !content.is_char_boundary(bounded) {
        bounded -= 1;
    }
    Some(bounded)
}

fn tokenize(value: &str) -> (Vec<String>, bool) {
    let mut terms = TermCollector::default();
    terms.add_text(value);
    terms.finish()
}

pub(crate) fn query_terms(value: &str) -> (Vec<String>, bool) {
    tokenize(value)
}

#[derive(Default)]
struct TermCollector {
    terms: BTreeSet<String>,
    truncated: bool,
}

impl TermCollector {
    fn add_text(&mut self, value: &str) {
        let mut identifier = String::new();
        for character in value.chars() {
            if character.is_alphanumeric() {
                identifier.push(character);
            } else {
                self.add_identifier(&identifier);
                identifier.clear();
            }
        }
        self.add_identifier(&identifier);
    }

    fn add_identifier(&mut self, identifier: &str) {
        if identifier.is_empty() {
            return;
        }
        self.add_term(identifier);
        for component in identifier_components(identifier) {
            self.add_term(&component);
        }
    }

    fn add_term(&mut self, term: &str) {
        let normalized = term
            .chars()
            .flat_map(char::to_lowercase)
            .collect::<String>();
        if normalized.is_empty() {
            return;
        }
        if normalized.chars().count() > MAX_TERM_CHARS {
            self.truncated = true;
            return;
        }
        if self.terms.contains(&normalized) {
            return;
        }
        if self.terms.len() == MAX_TERMS_PER_FIELD {
            self.truncated = true;
            return;
        }
        self.terms.insert(normalized);
    }

    fn finish(self) -> (Vec<String>, bool) {
        (self.terms.into_iter().collect(), self.truncated)
    }
}

fn identifier_components(identifier: &str) -> Vec<String> {
    let characters = identifier.chars().collect::<Vec<_>>();
    let mut components = Vec::new();
    let mut start = 0usize;
    for index in 1..characters.len() {
        let previous = characters[index - 1];
        let current = characters[index];
        let next = characters.get(index + 1).copied();
        let boundary = (previous.is_lowercase() && current.is_uppercase())
            || (previous.is_uppercase()
                && current.is_uppercase()
                && next.is_some_and(char::is_lowercase))
            || (previous.is_numeric() != current.is_numeric());
        if boundary {
            components.push(characters[start..index].iter().collect());
            start = index;
        }
    }
    components.push(characters[start..].iter().collect());
    components
}
