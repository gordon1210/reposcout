//! Unified-diff hunks split into change blocks, and the token edits inside each block.

use crate::query::changed_mapping::changed_hunks;
use anyhow::Result;
use std::ops::Range;

/// Token edits are computed only for blocks with at most this many tokens per side.
const MAX_BLOCK_TOKENS: usize = 2_000;

/// One diff line; `number` is the base line for `-` lines and the head line otherwise.
#[derive(Debug, Clone)]
pub(super) struct Line<'a> {
    pub tag: char,
    pub text: &'a str,
    pub number: usize,
}

/// Consecutive removed lines followed by consecutive added lines, as ranges into `Hunk::lines`.
#[derive(Debug, Clone)]
pub(super) struct Block {
    pub removed: Range<usize>,
    pub added: Range<usize>,
}

/// One token edit. Single-token replacements reduce to their differing part between word
/// boundaries (`inside`), so `laborIncrementMinutes` -> `laborStepMinutes` reads `Increment` ->
/// `Step`; other edits keep the changed token sequence joined by single spaces.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) struct Edit {
    pub old: String,
    pub new: String,
    pub inside: bool,
    /// Complete old and new tokens of a single-token replacement.
    pub whole: Option<(String, String)>,
    /// An identifier token replaced by an identifier token, as a whole or in part.
    pub rename: bool,
}

impl Edit {
    pub(super) fn key(&self) -> (&str, &str) {
        (&self.old, &self.new)
    }

    /// The edit's tokens and class: renames and other edits with the same text never share a
    /// rewrite.
    pub(super) fn class_key(&self) -> (&str, &str, bool) {
        (&self.old, &self.new, self.rename)
    }
}

#[derive(Debug, Clone)]
pub(super) struct Hunk<'a> {
    pub change: usize,
    pub old_start: usize,
    pub old_lines: usize,
    pub new_start: usize,
    pub new_lines: usize,
    /// The header line and every following line of the hunk, verbatim.
    pub raw: &'a str,
    pub lines: Vec<Line<'a>>,
    pub blocks: Vec<Block>,
    /// Edits of each block; `None` for blocks too large to compare token by token.
    pub edits: Vec<Option<Vec<Edit>>>,
}

impl Hunk<'_> {
    /// Every block has as many removed as added lines.
    pub(super) fn compact(&self) -> bool {
        self.blocks
            .iter()
            .all(|block| !block.removed.is_empty() && block.removed.len() == block.added.len())
    }

    /// Some block's non-blank removed and added lines differ in leading whitespace, compared in
    /// order.
    pub(super) fn reindented(&self) -> bool {
        let indents = |range: Range<usize>| {
            self.lines[range]
                .iter()
                .filter(|line| !line.text.trim().is_empty())
                .map(|line| indent(line.text))
                .collect::<Vec<_>>()
        };
        self.blocks.iter().any(|block| {
            !block.removed.is_empty()
                && !block.added.is_empty()
                && indents(block.removed.clone()) != indents(block.added.clone())
        })
    }

    /// First changed line on the surviving side: its head line, or for a removal the head line
    /// that follows it; base lines when the file no longer exists in head.
    pub(super) fn site_line(&self, head: bool) -> usize {
        let mut position = self.new_start;
        for line in &self.lines {
            match line.tag {
                '-' if !head => return line.number,
                '-' => return position.max(1),
                '+' if head => return line.number,
                '+' => {}
                _ => position += 1,
            }
        }
        if head { self.new_start } else { self.old_start }
    }

    pub(super) fn range_header(&self) -> String {
        format!(
            "@@ -{} +{} @@",
            span(self.old_start, self.old_lines),
            span(self.new_start, self.new_lines)
        )
    }
}

fn span(start: usize, lines: usize) -> String {
    if lines == 1 {
        start.to_string()
    } else {
        format!("{start},{lines}")
    }
}

/// Hunks of one captured unified diff in order.
pub(super) fn parse(change: usize, diff: &str) -> Vec<Hunk<'_>> {
    let mut hunks: Vec<Hunk<'_>> = Vec::new();
    let mut starts = Vec::new();
    let mut offset = 0;
    let mut old = 0;
    let mut new = 0;
    for piece in diff.split_inclusive('\n') {
        let start = offset;
        offset += piece.len();
        let text = piece.strip_suffix('\n').unwrap_or(piece);
        if let Some(header) = text.strip_prefix("@@ ") {
            if let Some((old_start, old_lines, new_start, new_lines)) = header_ranges(header) {
                old = old_start;
                new = new_start;
                starts.push(start);
                hunks.push(Hunk {
                    change,
                    old_start,
                    old_lines,
                    new_start,
                    new_lines,
                    raw: "",
                    lines: Vec::new(),
                    blocks: Vec::new(),
                    edits: Vec::new(),
                });
            }
            continue;
        }
        let Some(hunk) = hunks.last_mut() else {
            continue;
        };
        let (tag, body) = match text.chars().next() {
            Some(tag @ (' ' | '-' | '+')) => (tag, &text[1..]),
            _ => continue,
        };
        let number = if tag == '-' { old } else { new };
        match tag {
            '-' => old += 1,
            '+' => new += 1,
            _ => {
                old += 1;
                new += 1;
            }
        }
        hunk.lines.push(Line {
            tag,
            text: body,
            number,
        });
    }
    let ends: Vec<_> = starts.iter().skip(1).copied().chain([diff.len()]).collect();
    for ((hunk, start), end) in hunks.iter_mut().zip(starts).zip(ends) {
        hunk.raw = &diff[start..end];
        hunk.blocks = blocks(&hunk.lines);
    }
    hunks
}

/// Leading whitespace of a line.
pub(super) fn indent(text: &str) -> &str {
    &text[..text.len() - text.trim_start().len()]
}

/// Keywords, literal words and value-like names; replacing one changes behavior, so it is never
/// a rename.
const KEYWORDS: [&str; 116] = [
    "Err",
    "False",
    "Infinity",
    "NULL",
    "NaN",
    "None",
    "Ok",
    "Self",
    "Some",
    "True",
    "and",
    "any",
    "arguments",
    "as",
    "assert",
    "async",
    "await",
    "break",
    "case",
    "catch",
    "chan",
    "class",
    "classmethod",
    "const",
    "continue",
    "crate",
    "def",
    "default",
    "defer",
    "del",
    "delete",
    "do",
    "elif",
    "else",
    "enum",
    "except",
    "export",
    "extends",
    "false",
    "final",
    "finally",
    "fn",
    "for",
    "from",
    "func",
    "function",
    "globalThis",
    "go",
    "goto",
    "if",
    "impl",
    "implements",
    "import",
    "in",
    "instanceof",
    "interface",
    "is",
    "lambda",
    "let",
    "loop",
    "match",
    "mod",
    "move",
    "mut",
    "never",
    "new",
    "nil",
    "nonlocal",
    "not",
    "null",
    "nullptr",
    "object",
    "of",
    "or",
    "override",
    "package",
    "pass",
    "private",
    "property",
    "protected",
    "pub",
    "public",
    "raise",
    "readonly",
    "ref",
    "return",
    "satisfies",
    "select",
    "self",
    "static",
    "staticmethod",
    "struct",
    "super",
    "switch",
    "this",
    "throw",
    "throws",
    "trait",
    "true",
    "try",
    "type",
    "typeof",
    "undefined",
    "unknown",
    "unless",
    "unsafe",
    "until",
    "use",
    "var",
    "void",
    "when",
    "where",
    "while",
    "window",
    "with",
    "yield",
];

/// A name token: starts with a letter, `_` or `$`, consists of letters, digits, `_` and `$`,
/// and is not a keyword.
pub(super) fn identifier(token: &str) -> bool {
    token
        .chars()
        .next()
        .is_some_and(|ch| ch.is_alphabetic() || matches!(ch, '_' | '$'))
        && token
            .chars()
            .all(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '$'))
        && KEYWORDS.binary_search(&token).is_err()
}

/// `-a,b +c,d @@ context` with Git's default length of one.
fn header_ranges(header: &str) -> Option<(usize, usize, usize, usize)> {
    let (ranges, _) = header.split_once(" @@")?;
    let (old, new) = ranges.split_once(' ')?;
    let parse = |value: &str| -> Option<(usize, usize)> {
        match value.split_once(',') {
            Some((start, lines)) => Some((start.parse().ok()?, lines.parse().ok()?)),
            None => Some((value.parse().ok()?, 1)),
        }
    };
    let (old_start, old_lines) = parse(old.strip_prefix('-')?)?;
    let (new_start, new_lines) = parse(new.strip_prefix('+')?)?;
    Some((old_start, old_lines, new_start, new_lines))
}

fn blocks(lines: &[Line<'_>]) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        if lines[index].tag == ' ' {
            index += 1;
            continue;
        }
        let removed_start = index;
        while index < lines.len() && lines[index].tag == '-' {
            index += 1;
        }
        let added_start = index;
        while index < lines.len() && lines[index].tag == '+' {
            index += 1;
        }
        blocks.push(Block {
            removed: removed_start..added_start,
            added: added_start..index,
        });
    }
    blocks
}

/// Token texts of `text` without whitespace.
pub(super) fn tokens(text: &str) -> Vec<String> {
    crate::dup::tokenize(text)
        .into_iter()
        .map(|token| token.text)
        .collect()
}

/// Edits of every block of `hunk`; blocks above the token cap get `None`.
pub(super) fn edits(hunk: &Hunk<'_>) -> Result<Vec<Option<Vec<Edit>>>> {
    hunk.blocks
        .iter()
        .map(|block| {
            let side = |range: &Range<usize>| {
                let text: Vec<_> = hunk.lines[range.clone()]
                    .iter()
                    .map(|line| line.text)
                    .collect();
                tokens(&text.join("\n"))
            };
            block_edits(&side(&block.removed), &side(&block.added))
        })
        .collect()
}

fn block_edits(old: &[String], new: &[String]) -> Result<Option<Vec<Edit>>> {
    Ok(aligned(old, new)?.map(|edits| edits.into_iter().map(|(edit, ..)| edit).collect()))
}

/// Token edits with the token range each covers in the old and new sequence.
pub(super) type Aligned = Vec<(Edit, Range<usize>, Range<usize>)>;

/// Token edits between two token sequences with the token range each covers on both sides, in
/// order; `None` above the token cap.
pub(super) fn aligned(old: &[String], new: &[String]) -> Result<Option<Aligned>> {
    if old.len() > MAX_BLOCK_TOKENS || new.len() > MAX_BLOCK_TOKENS {
        return Ok(None);
    }
    if old.is_empty() || new.is_empty() {
        return Ok(Some(vec![(sequence(old, new), 0..old.len(), 0..new.len())]));
    }
    // One token per line lets Git's line diff align token sequences.
    let lines = |tokens: &[String]| {
        let mut joined = String::new();
        for token in tokens {
            joined.push_str(&token.replace('\n', "\u{1}"));
            joined.push('\n');
        }
        joined
    };
    let changed = changed_hunks(&lines(old), &lines(new))?;
    if changed.omitted_hunks > 0 {
        return Ok(None);
    }
    let mut edits = Vec::new();
    for hunk in changed.hunks {
        // Git numbers an empty side by the line before it.
        let range = |start: usize, count: usize| {
            if count == 0 {
                start..start
            } else {
                start - 1..start - 1 + count
            }
        };
        let removed = range(hunk.old_start, hunk.old_lines);
        let added = range(hunk.new_start, hunk.new_lines);
        let edit = if let ([removed], [added]) = (&old[removed.clone()], &new[added.clone()]) {
            let mut edit = reduce(removed, added);
            edit.rename = identifier(removed) && identifier(added);
            edit
        } else {
            sequence(&old[removed.clone()], &new[added.clone()])
        };
        edits.push((edit, removed, added));
    }
    Ok(Some(edits))
}

/// Tokens of one line with their byte ranges in it.
pub(super) fn line_tokens(text: &str) -> (Vec<String>, Vec<Range<usize>>) {
    crate::dup::tokenize(text)
        .into_iter()
        .map(|token| (token.text, token.start_byte..token.end_byte))
        .unzip()
}

/// Bytes of a line that an edit of tokens `range` covers: the tokens themselves when both sides
/// of the edit have tokens, otherwise the whole whitespace gap around them, so an inserted or
/// deleted token takes its own spacing along.
fn edit_bytes(
    spans: &[Range<usize>],
    range: &Range<usize>,
    both: bool,
    len: usize,
) -> Range<usize> {
    if both {
        return spans[range.start].start..spans[range.end - 1].end;
    }
    let start = match range.start.checked_sub(1) {
        Some(previous) => spans[previous].end,
        None => spans.first().map_or(len, |span| span.start),
    };
    let end = match spans.get(range.end) {
        Some(next) => next.start,
        None => spans.last().map_or(len, |span| span.end),
    };
    start..end.max(start)
}

/// `old` with the tokens of every edit replaced by the text of its counterpart in `new`, all
/// other bytes kept.
pub(super) fn substitute(
    old: (&str, &[Range<usize>]),
    new: (&str, &[Range<usize>]),
    edits: &[(Edit, Range<usize>, Range<usize>)],
) -> String {
    let mut output = String::with_capacity(new.0.len());
    let mut cursor = 0;
    for (_, removed, added) in edits {
        let both = !removed.is_empty() && !added.is_empty();
        let from = edit_bytes(old.1, removed, both, old.0.len());
        let to = edit_bytes(new.1, added, both, new.0.len());
        output.push_str(&old.0[cursor..from.start.max(cursor)]);
        output.push_str(&new.0[to]);
        cursor = cursor.max(from.end);
    }
    output.push_str(&old.0[cursor..]);
    output
}

fn sequence(old: &[String], new: &[String]) -> Edit {
    Edit {
        old: old.join(" "),
        new: new.join(" "),
        inside: false,
        whole: None,
        rename: false,
    }
}

/// Reduce a single-token replacement to its differing part between word boundaries.
pub(super) fn reduce(old: &str, new: &str) -> Edit {
    let whole = Some((old.to_string(), new.to_string()));
    let plain = Edit {
        old: old.to_string(),
        new: new.to_string(),
        inside: false,
        whole: whole.clone(),
        rename: false,
    };
    if [old, new]
        .iter()
        .any(|token| token.chars().next().is_none_or(|ch| ch.is_ascii_digit()))
    {
        return plain;
    }
    let mut prefix = old
        .char_indices()
        .zip(new.chars())
        .take_while(|((_, left), right)| left == right)
        .last()
        .map_or(0, |((index, ch), _)| index + ch.len_utf8());
    while prefix > 0 && !(boundary(old, prefix) && boundary(new, prefix)) {
        prefix = old[..prefix]
            .char_indices()
            .next_back()
            .map_or(0, |(index, _)| index);
    }
    let old_rest = &old[prefix..];
    let new_rest = &new[prefix..];
    let mut suffix = old_rest
        .chars()
        .rev()
        .zip(new_rest.chars().rev())
        .take_while(|(left, right)| left == right)
        .map(|(ch, _)| ch.len_utf8())
        .sum::<usize>();
    while suffix > 0 && !(boundary(old, old.len() - suffix) && boundary(new, new.len() - suffix)) {
        let tail = &old[old.len() - suffix..];
        suffix -= tail.chars().next().map_or(suffix, char::len_utf8);
    }
    if prefix == 0 && suffix == 0 {
        return plain;
    }
    let old_part = &old[prefix..old.len() - suffix];
    let new_part = &new[prefix..new.len() - suffix];
    if old_part.is_empty() || new_part.is_empty() {
        return plain;
    }
    Edit {
        old: old_part.to_string(),
        new: new_part.to_string(),
        inside: true,
        whole,
        rename: false,
    }
}

/// Word boundary inside one token: its ends, next to a non-alphanumeric character, a
/// lower-to-upper case change or a letter/digit change.
pub(super) fn boundary(token: &str, index: usize) -> bool {
    if index == 0 || index >= token.len() || !token.is_char_boundary(index) {
        return index == 0 || index >= token.len();
    }
    let (Some(before), Some(after)) = (
        token[..index].chars().next_back(),
        token[index..].chars().next(),
    ) else {
        return true;
    };
    !before.is_alphanumeric()
        || !after.is_alphanumeric()
        || (before.is_lowercase() && after.is_uppercase())
        || (before.is_numeric() != after.is_numeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_token_edits_reduce_to_whole_words() {
        let edit = reduce("laborIncrementMinutes", "laborStepMinutes");
        assert_eq!(
            (edit.old.as_str(), edit.new.as_str(), edit.inside),
            ("Increment", "Step", true)
        );
        let edit = reduce("incrementsToCover", "stepsToCover");
        assert_eq!(
            (edit.old.as_str(), edit.new.as_str()),
            ("increments", "steps")
        );
        let edit = reduce("JOB_PRIORITIES", "WORK_ORDER_PRIORITIES");
        assert_eq!(
            (edit.old.as_str(), edit.new.as_str()),
            ("JOB", "WORK_ORDER")
        );
        let edit = reduce("roundToIncrement", "quantize");
        assert_eq!(
            (edit.old.as_str(), edit.inside),
            ("roundToIncrement", false)
        );
        let edit = reduce("100", "150");
        assert_eq!((edit.old.as_str(), edit.new.as_str()), ("100", "150"));
        let edit = reduce("fooBar", "fooBaz");
        assert_eq!((edit.old.as_str(), edit.new.as_str()), ("Bar", "Baz"));
        // An insertion inside a name keeps both whole tokens.
        let edit = reduce("total", "totalCents");
        assert_eq!(
            (edit.old.as_str(), edit.new.as_str()),
            ("total", "totalCents")
        );
    }

    #[test]
    fn hunks_parse_lines_blocks_and_token_edits() {
        let diff = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,3 +1,3 @@ fn main\n a\n-let v = f(x, true);\n+let v = g(x, { up: true });\n c\n";
        let hunks = parse(0, diff);
        assert_eq!(hunks.len(), 1);
        let hunk = &hunks[0];
        assert_eq!(
            (
                hunk.old_start,
                hunk.old_lines,
                hunk.new_start,
                hunk.new_lines
            ),
            (1, 3, 1, 3)
        );
        assert!(hunk.raw.starts_with("@@ -1,3 +1,3 @@ fn main\n"));
        assert!(hunk.compact());
        assert_eq!(hunk.lines[1].number, 2);
        let edits = edits(hunk).unwrap();
        let edits = edits[0].as_ref().unwrap();
        let keys: Vec<_> = edits.iter().map(Edit::key).collect();
        assert_eq!(keys, [("f", "g"), ("", "{ up :"), ("", "}")]);
    }

    #[test]
    fn only_identifier_replacements_are_renames() {
        let classes = |line_old: &str, line_new: &str| -> Vec<(String, String, bool)> {
            let diff = format!("diff --git a/x b/x\n@@ -1 +1 @@\n-{line_old}\n+{line_new}\n");
            let hunks = parse(0, &diff);
            edits(&hunks[0]).unwrap()[0]
                .as_ref()
                .unwrap()
                .iter()
                .map(|edit| (edit.old.clone(), edit.new.clone(), edit.rename))
                .collect()
        };
        let edit = |old: &str, new: &str, rename| (old.to_string(), new.to_string(), rename);
        assert_eq!(classes("a(x);", "b(x);"), [edit("a", "b", true)]);
        assert_eq!(
            classes("fetch_and_cache(x)", "fetch_or_cache(x)"),
            [edit("and", "or", true)]
        );
        for (old, new, from, to) in [
            ("if a and b:", "if a or b:", "and", "or"),
            ("  verify: true,", "  verify: false,", "true", "false"),
            ("  rounds: 12,", "  rounds: 4,", "12", "4"),
            ("  node('node12');", "  node('node4');", "12", "4"),
            ("  emit('held');", "  emit('paused');", "held", "paused"),
            ("  return false;", "  return true;", "false", "true"),
        ] {
            assert_eq!(classes(old, new), [edit(from, to, false)], "{old}");
        }
        assert_eq!(
            classes("if (user.ok) {", "if (!user.ok) {"),
            [edit("", "!", false)]
        );
    }

    #[test]
    fn reindented_hunks_change_leading_whitespace_of_paired_blocks() {
        fn hunk(diff: &'static str) -> Hunk<'static> {
            parse(0, diff).remove(0)
        }
        let shifted = hunk(
            "diff --git a/x b/x\n@@ -1,2 +1,2 @@\n-  a(x);\n-  b(x);\n+    a(y);\n+    b(y);\n",
        );
        assert!(shifted.reindented());
        let kept = hunk("diff --git a/x b/x\n@@ -1 +1 @@\n-    a(x);\n+    a(y);\n");
        assert!(!kept.reindented());
        // A blank line between blocks of unequal size never hides a dedent.
        let blank = hunk(
            "diff --git a/x b/x\n@@ -1,3 +1,4 @@\n-    a(x)\n-    b(x)\n-  c(x)\n+    a(y)\n+\n+  b(y)\n+  c(y)\n",
        );
        assert!(blank.reindented());
        let inserted = hunk("diff --git a/x b/x\n@@ -1 +1,2 @@\n a\n+    b\n");
        assert!(!inserted.reindented());
    }

    #[test]
    fn keywords_are_sorted_for_binary_search() {
        assert!(KEYWORDS.windows(2).all(|pair| pair[0] < pair[1]));
        for word in [
            "Ok",
            "Some",
            "globalThis",
            "unknown",
            "nullptr",
            "arguments",
        ] {
            assert!(!identifier(word), "{word}");
        }
    }

    #[test]
    fn substitution_replaces_edited_tokens_and_keeps_other_bytes() {
        let check = |old: &str, new: &str| {
            let (old_tokens, old_spans) = line_tokens(old);
            let (new_tokens, new_spans) = line_tokens(new);
            let edits = aligned(&old_tokens, &new_tokens).unwrap().unwrap();
            substitute((old, &old_spans), (new, &new_spans), &edits)
        };
        assert_eq!(check("  f(a, 1);", "  g(a, 1, b);"), "  g(a, 1, b);");
        assert_eq!(check("  f(a, 1);", "    g(a, 1);"), "  g(a, 1);");
        assert_eq!(check("x  =  f(a)", "x = g(a)"), "x  =  g(a)");
        assert_eq!(check("f(a, b)", "f(a)"), "f(a)");
        assert_eq!(
            check("g(m, b, true);", "g(m, { s: b, up: 1 });"),
            "g(m, { s: b, up: 1 });"
        );
        assert_eq!(check("  a b", "  b"), "  b");
    }

    #[test]
    fn site_line_is_the_first_changed_line_on_the_surviving_side() {
        let hunk = parse(
            0,
            "diff --git a/x b/x\n@@ -10,4 +20,4 @@\n a\n b\n-c\n+C\n d\n",
        )
        .remove(0);
        assert_eq!((hunk.site_line(true), hunk.site_line(false)), (22, 12));
        let removal = parse(0, "diff --git a/x b/x\n@@ -10,3 +20,2 @@\n a\n-b\n c\n").remove(0);
        assert_eq!(removal.site_line(true), 21);
    }
}
