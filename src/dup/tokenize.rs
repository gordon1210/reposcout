use crate::dup::{DuplicationMode, LiteralKind, Token, TokenKind};
use crate::lang::{LangInfo, detect};
use crate::parse;
use std::path::Path;
use tree_sitter::Node;

/// Tokenize one source file for duplication detection.
///
/// First-class languages use tree-sitter leaf ranges so malformed-but-readable
/// files still get precise tokens. Other formats use the fallback lexer below.
/// Trivia filtering is deliberately separate from Type-1/Type-2 matching.
pub(crate) fn tokenize_path(path: &Path, source: &str, mode: DuplicationMode) -> Vec<Token> {
    let info = detect(path);
    let mut tokens = info
        .and_then(|lang| {
            lang.first_class
                .and_then(|fc| parse::parse(fc, source))
                .map(|tree| tokenize_tree(tree.root_node(), source))
        })
        .filter(|tokens| !tokens.is_empty())
        .unwrap_or_else(|| tokenize_generic(source, info));

    tokens.retain(|token| match mode {
        DuplicationMode::Strict => true,
        DuplicationMode::Mild => token.kind != TokenKind::Whitespace,
        DuplicationMode::Weak => !matches!(token.kind, TokenKind::Whitespace | TokenKind::Comment),
    });
    tokens
}

/// Compatibility tokenizer used by callers/tests that do not have a path.
pub(crate) fn tokenize_unscoped(source: &str) -> Vec<Token> {
    tokenize_generic(source, None)
        .into_iter()
        .filter(|token| token.kind != TokenKind::Whitespace)
        .collect()
}

fn tokenize_tree(root: Node<'_>, source: &str) -> Vec<Token> {
    let map = SourceMap::new(source);
    let mut syntax = Vec::new();
    collect_tree_tokens(root, source, &map, &mut syntax);
    syntax.sort_by_key(|token| (token.start_byte, token.end_byte));

    // Tree-sitter omits trivia. Reinsert whitespace gaps so strict/mild/weak
    // remain an orthogonal filtering choice. Non-whitespace recovery gaps are
    // kept as `Other` rather than silently disappearing on malformed input.
    let mut tokens = Vec::with_capacity(syntax.len() * 2);
    let mut cursor = 0usize;
    for token in syntax {
        if token.start_byte > cursor {
            push_gap(&mut tokens, source, &map, cursor, token.start_byte);
        }
        cursor = cursor.max(token.end_byte);
        tokens.push(token);
    }
    if cursor < source.len() {
        push_gap(&mut tokens, source, &map, cursor, source.len());
    }
    tokens
}

fn collect_tree_tokens(node: Node<'_>, source: &str, map: &SourceMap<'_>, out: &mut Vec<Token>) {
    if node.is_missing() || node.end_byte() <= node.start_byte() {
        return;
    }

    let kind = node.kind();
    let atomic = is_comment_kind(kind)
        || is_identifier_kind(kind)
        || (literal_kind(kind, source.get(node.byte_range()).unwrap_or_default()).is_some()
            && !has_interpolation_child(node));

    if atomic || node.child_count() == 0 {
        let text = source.get(node.byte_range()).unwrap_or_default();
        if text.is_empty() {
            return;
        }
        out.push(make_token(
            classify_tree_token(kind, text),
            text,
            node.start_byte(),
            node.end_byte(),
            map,
        ));
        return;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_tree_tokens(child, source, map, out);
    }
}

fn has_interpolation_child(node: Node<'_>) -> bool {
    let mut cursor = node.walk();
    node.children(&mut cursor).any(|child| {
        let kind = child.kind();
        kind.contains("interpolation") || kind.contains("substitution")
    })
}

fn push_gap(tokens: &mut Vec<Token>, source: &str, map: &SourceMap<'_>, start: usize, end: usize) {
    let Some(text) = source.get(start..end) else {
        return;
    };
    if text.is_empty() {
        return;
    }
    let kind = if text.chars().all(char::is_whitespace) {
        TokenKind::Whitespace
    } else {
        TokenKind::Other
    };
    tokens.push(make_token(kind, text, start, end, map));
}

fn classify_tree_token(kind: &str, text: &str) -> TokenKind {
    if is_comment_kind(kind) {
        TokenKind::Comment
    } else if let Some(literal) = literal_kind(kind, text) {
        TokenKind::Literal(literal)
    } else if is_identifier_kind(kind) {
        if is_keyword(text) {
            TokenKind::Keyword
        } else {
            TokenKind::Identifier
        }
    } else if is_keyword(text) {
        TokenKind::Keyword
    } else if is_punctuation(text) {
        TokenKind::Punctuation
    } else if text.chars().all(char::is_whitespace) {
        TokenKind::Whitespace
    } else if node_like_operator(text) {
        TokenKind::Operator
    } else {
        TokenKind::Other
    }
}

fn is_comment_kind(kind: &str) -> bool {
    kind.contains("comment")
}

fn is_identifier_kind(kind: &str) -> bool {
    kind.contains("identifier")
        || matches!(
            kind,
            "lifetime"
                | "metavariable"
                | "shorthand_property_identifier"
                | "name"
                | "variable_name"
        )
}

fn literal_kind(kind: &str, text: &str) -> Option<LiteralKind> {
    let lower = kind.to_ascii_lowercase();
    if lower.contains("string") || lower.contains("template") {
        Some(LiteralKind::String)
    } else if lower.contains("char") || lower == "rune_literal" {
        Some(LiteralKind::Character)
    } else if lower.contains("float") || lower.contains("decimal") {
        Some(LiteralKind::Float)
    } else if lower.contains("integer") || lower.contains("number") || lower.contains("numeric") {
        Some(LiteralKind::Integer)
    } else if matches!(text.to_ascii_lowercase().as_str(), "true" | "false") {
        Some(LiteralKind::Boolean)
    } else if matches!(text.to_ascii_lowercase().as_str(), "null" | "none" | "nil") {
        Some(LiteralKind::Null)
    } else {
        None
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "the fallback lexer is a single ordered scanner state machine where rule precedence must remain visible"
)]
fn tokenize_generic(source: &str, info: Option<&LangInfo>) -> Vec<Token> {
    let map = SourceMap::new(source);
    let mut tokens = Vec::new();
    let mut i = 0usize;

    while i < source.len() {
        let Some(ch) = source[i..].chars().next() else {
            break;
        };

        if ch.is_whitespace() {
            let end = take_while(source, i, char::is_whitespace);
            tokens.push(make_token(
                TokenKind::Whitespace,
                &source[i..end],
                i,
                end,
                &map,
            ));
            i = end;
            continue;
        }

        if let Some((open, close)) = block_comment_at(source, i, info) {
            let content_start = i + open.len();
            let end = source[content_start..]
                .find(close)
                .map_or(source.len(), |at| content_start + at + close.len());
            tokens.push(make_token(
                TokenKind::Comment,
                &source[i..end],
                i,
                end,
                &map,
            ));
            i = end;
            continue;
        }

        if let Some(prefix) = line_comment_at(source, i, info) {
            let end = source[i + prefix.len()..]
                .find('\n')
                .map_or(source.len(), |at| i + prefix.len() + at);
            tokens.push(make_token(
                TokenKind::Comment,
                &source[i..end],
                i,
                end,
                &map,
            ));
            i = end;
            continue;
        }

        if matches!(ch, '\'' | '"' | '`') {
            let end = take_string(source, i, ch);
            let kind = if ch == '\'' && source[i..end].chars().count() <= 4 {
                LiteralKind::Character
            } else {
                LiteralKind::String
            };
            tokens.push(make_token(
                TokenKind::Literal(kind),
                &source[i..end],
                i,
                end,
                &map,
            ));
            i = end;
            continue;
        }

        if ch.is_ascii_digit() {
            let (end, kind) = take_number(source, i);
            tokens.push(make_token(
                TokenKind::Literal(kind),
                &source[i..end],
                i,
                end,
                &map,
            ));
            i = end;
            continue;
        }

        if is_identifier_start(ch) {
            let end = take_while(source, i, is_identifier_continue);
            let text = &source[i..end];
            let kind = if matches!(text.to_ascii_lowercase().as_str(), "true" | "false") {
                TokenKind::Literal(LiteralKind::Boolean)
            } else if matches!(text.to_ascii_lowercase().as_str(), "null" | "none" | "nil") {
                TokenKind::Literal(LiteralKind::Null)
            } else if is_keyword(text) {
                TokenKind::Keyword
            } else {
                TokenKind::Identifier
            };
            tokens.push(make_token(kind, text, i, end, &map));
            i = end;
            continue;
        }

        let end = matching_operator(source, i)
            .map_or_else(|| i + ch.len_utf8(), |operator| i + operator.len());
        let text = &source[i..end];
        let kind = if is_punctuation(text) {
            TokenKind::Punctuation
        } else {
            TokenKind::Operator
        };
        tokens.push(make_token(kind, text, i, end, &map));
        i = end;
    }

    tokens
}

fn block_comment_at<'a>(
    source: &str,
    offset: usize,
    info: Option<&'a LangInfo>,
) -> Option<(&'a str, &'a str)> {
    info?.block_comments.iter().copied().find(|(open, _)| {
        // Python triple-quoted strings are literals/docstrings, not comments.
        !open.starts_with(['\'', '"']) && source[offset..].starts_with(open)
    })
}

fn line_comment_at<'a>(source: &str, offset: usize, info: Option<&'a LangInfo>) -> Option<&'a str> {
    info?
        .line_comments
        .iter()
        .copied()
        .filter(|prefix| source[offset..].starts_with(prefix))
        .max_by_key(|prefix| prefix.len())
}

fn take_string(source: &str, start: usize, quote: char) -> usize {
    let triple = quote != '`' && source[start..].starts_with(&quote.to_string().repeat(3));
    let delimiter_len = if triple {
        quote.len_utf8() * 3
    } else {
        quote.len_utf8()
    };
    let mut i = start + delimiter_len;
    let mut escaped = false;
    while i < source.len() {
        if triple && source[i..].starts_with(&quote.to_string().repeat(3)) {
            return i + delimiter_len;
        }
        let Some(ch) = source[i..].chars().next() else {
            break;
        };
        i += ch.len_utf8();
        if !triple && ch == quote && !escaped {
            return i;
        }
        escaped = ch == '\\' && !escaped;
    }
    source.len()
}

fn take_number(source: &str, start: usize) -> (usize, LiteralKind) {
    let bytes = source.as_bytes();
    let mut i = start;
    let mut radix = 10u32;
    let mut float = false;

    if bytes.get(start) == Some(&b'0') {
        match bytes.get(start + 1).copied() {
            Some(b'x' | b'X') => {
                radix = 16;
                i += 2;
            }
            Some(b'b' | b'B') => {
                radix = 2;
                i += 2;
            }
            Some(b'o' | b'O') => {
                radix = 8;
                i += 2;
            }
            _ => {}
        }
    }

    i = take_radix_digits(bytes, i, radix);
    if bytes.get(i) == Some(&b'.') && bytes.get(i + 1) != Some(&b'.') {
        float = true;
        i = take_radix_digits(bytes, i + 1, radix);
    }

    let exponent_marker = match radix {
        10 => matches!(bytes.get(i), Some(b'e' | b'E')),
        16 => matches!(bytes.get(i), Some(b'p' | b'P')),
        _ => false,
    };
    if exponent_marker {
        let marker = i;
        let mut exponent = i + 1;
        if matches!(bytes.get(exponent), Some(b'+' | b'-')) {
            exponent += 1;
        }
        let end = take_radix_digits(bytes, exponent, 10);
        if bytes[exponent..end].iter().any(u8::is_ascii_digit) {
            float = true;
            i = end;
        } else {
            i = marker;
        }
    }

    // Preserve language-specific suffixes such as `u64`, `UL`, and `f32`.
    while bytes
        .get(i)
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        i += 1;
    }

    (
        i,
        if float {
            LiteralKind::Float
        } else {
            LiteralKind::Integer
        },
    )
}

fn take_radix_digits(source: &[u8], mut start: usize, radix: u32) -> usize {
    while source
        .get(start)
        .is_some_and(|byte| *byte == b'_' || (*byte as char).is_digit(radix))
    {
        start += 1;
    }
    start
}

fn take_while(source: &str, start: usize, predicate: impl Fn(char) -> bool) -> usize {
    let mut i = start;
    while i < source.len() {
        let Some(ch) = source[i..].chars().next() else {
            break;
        };
        if !predicate(ch) {
            break;
        }
        i += ch.len_utf8();
    }
    i
}

fn make_token(kind: TokenKind, text: &str, start: usize, end: usize, map: &SourceMap<'_>) -> Token {
    let (line, start_column) = map.location(start);
    let (end_line, end_column) = map.location(end);
    Token {
        kind,
        text: text.to_string(),
        line,
        end_line,
        start_column,
        end_column,
        start_byte: start,
        end_byte: end,
    }
}

struct SourceMap<'a> {
    source: &'a str,
    line_starts: Vec<usize>,
}

impl<'a> SourceMap<'a> {
    fn new(source: &'a str) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(
            source
                .bytes()
                .enumerate()
                .filter_map(|(index, byte)| (byte == b'\n').then_some(index + 1)),
        );
        Self {
            source,
            line_starts,
        }
    }

    /// One-based line and Unicode-code-point column.
    fn location(&self, byte: usize) -> (usize, usize) {
        let byte = byte.min(self.source.len());
        let line_index = self.line_starts.partition_point(|start| *start <= byte) - 1;
        let line_start = self.line_starts[line_index];
        let column = self.source[line_start..byte].chars().count() + 1;
        (line_index + 1, column)
    }
}

fn is_identifier_start(ch: char) -> bool {
    ch.is_alphabetic() || matches!(ch, '_' | '$')
}

fn is_identifier_continue(ch: char) -> bool {
    ch.is_alphanumeric() || matches!(ch, '_' | '$')
}

fn is_punctuation(text: &str) -> bool {
    matches!(
        text,
        "(" | ")" | "[" | "]" | "{" | "}" | "," | ";" | ":" | "."
    )
}

fn node_like_operator(text: &str) -> bool {
    text.chars()
        .all(|ch| !ch.is_alphanumeric() && !ch.is_whitespace())
}

fn matching_operator(source: &str, offset: usize) -> Option<&'static str> {
    const OPERATORS: &[&str] = &[
        ">>>=", "<<=", ">>=", "**=", "...", "===", "!==", "=>", "->", "::", "==", "!=", "<=", ">=",
        "&&", "||", "++", "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<", ">>", "**",
        "??", "?.", "..", ":=", "<-",
    ];
    OPERATORS
        .iter()
        .copied()
        .find(|operator| source[offset..].starts_with(operator))
}

pub(crate) fn is_keyword(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "abstract"
            | "and"
            | "as"
            | "assert"
            | "async"
            | "await"
            | "become"
            | "box"
            | "break"
            | "case"
            | "catch"
            | "chan"
            | "class"
            | "const"
            | "continue"
            | "crate"
            | "declare"
            | "def"
            | "default"
            | "defer"
            | "del"
            | "do"
            | "dyn"
            | "elif"
            | "else"
            | "enum"
            | "except"
            | "export"
            | "extends"
            | "extern"
            | "fallthrough"
            | "final"
            | "finally"
            | "fn"
            | "for"
            | "from"
            | "func"
            | "function"
            | "global"
            | "go"
            | "if"
            | "impl"
            | "implements"
            | "import"
            | "in"
            | "instanceof"
            | "interface"
            | "is"
            | "lambda"
            | "let"
            | "loop"
            | "map"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "namespace"
            | "new"
            | "nonlocal"
            | "not"
            | "of"
            | "or"
            | "override"
            | "package"
            | "pass"
            | "private"
            | "protected"
            | "pub"
            | "public"
            | "raise"
            | "range"
            | "readonly"
            | "ref"
            | "return"
            | "select"
            | "self"
            | "static"
            | "struct"
            | "super"
            | "switch"
            | "this"
            | "throw"
            | "trait"
            | "try"
            | "type"
            | "typeof"
            | "undefined"
            | "unsafe"
            | "use"
            | "var"
            | "virtual"
            | "void"
            | "where"
            | "while"
            | "with"
            | "yield"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weak_ignores_comments_but_mild_retains_them() {
        let source = "let value = 42; // explanation\nreturn value;";
        let mild = tokenize_path(Path::new("a.rs"), source, DuplicationMode::Mild);
        let weak = tokenize_path(Path::new("a.rs"), source, DuplicationMode::Weak);

        assert!(mild.iter().any(|token| token.kind == TokenKind::Comment));
        assert!(!weak.iter().any(|token| token.kind == TokenKind::Comment));
    }

    #[test]
    fn strings_and_numbers_have_distinct_literal_categories() {
        let tokens = tokenize_path(
            Path::new("a.rs"),
            "let text = \"42\"; let number = 42;",
            DuplicationMode::Mild,
        );

        assert!(
            tokens
                .iter()
                .any(|token| token.kind == TokenKind::Literal(LiteralKind::String))
        );
        assert!(
            tokens
                .iter()
                .any(|token| token.kind == TokenKind::Literal(LiteralKind::Integer))
        );
    }

    #[test]
    fn generic_numbers_accept_signs_only_directly_after_exponent_markers() {
        let tokens = tokenize_path(
            Path::new("numbers.c"),
            "1e-3 + 2E+4 - 0x1.fp-2 + 7+8 - 9-10",
            DuplicationMode::Mild,
        );
        let texts = tokens
            .iter()
            .map(|token| token.text.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            texts,
            vec![
                "1e-3", "+", "2E+4", "-", "0x1.fp-2", "+", "7", "+", "8", "-", "9", "-", "10"
            ]
        );
        assert!(matches!(
            tokens[0].kind,
            TokenKind::Literal(LiteralKind::Float)
        ));
        assert!(matches!(
            tokens[4].kind,
            TokenKind::Literal(LiteralKind::Float)
        ));
    }

    #[test]
    fn locations_use_code_point_columns_and_half_open_bytes() {
        let source = "let café = 1;";
        let tokens = tokenize_path(Path::new("a.rs"), source, DuplicationMode::Mild);
        let cafe = tokens
            .iter()
            .find(|token| token.text == "café")
            .expect("identifier token");

        assert_eq!(cafe.start_column, 5);
        assert_eq!(cafe.end_column, 9);
        assert_eq!(&source[cafe.start_byte..cafe.end_byte], "café");
    }

    #[test]
    fn malformed_first_class_source_is_tokenized_without_panicking() {
        let result = std::panic::catch_unwind(|| {
            tokenize_path(
                Path::new("broken.ts"),
                "function broken( { const text = `unterminated ${value`; ",
                DuplicationMode::Mild,
            )
        });

        assert!(result.is_ok());
        assert!(!result.unwrap().is_empty());
    }

    #[test]
    fn php_names_and_variables_are_structured_identifiers() {
        let tokens = tokenize_path(
            Path::new("sample.php"),
            "<?php function total(array $values): int { $sum = $values[0]; return $sum; }",
            DuplicationMode::Mild,
        );

        for identifier in ["total", "$values", "$sum"] {
            assert!(
                tokens.iter().any(|token| {
                    token.text == identifier && token.kind == TokenKind::Identifier
                }),
                "missing PHP identifier {identifier}: {tokens:?}"
            );
        }
    }
}
