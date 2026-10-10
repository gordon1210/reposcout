use crate::model::{
    ReviewBrief, ReviewContextChange, ReviewContextFile, ReviewContextRelation, ReviewDocMention,
    ReviewUsageLine,
};
use crate::scan::ReviewCapture;
use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const MAX_TERMS: usize = 32;
const MIN_TERM_CHARS: usize = 4;
const MAX_MENTIONS: usize = 12;
const MAX_MENTIONS_PER_FILE: usize = 3;
const MAX_MENTION_CHARS: usize = 160;
const MAX_USAGE: usize = 16;
const MAX_USAGE_PER_FILE: usize = 3;
const MAX_CHAIN_DISTANCE: usize = 8;
const SIDES: [&str; 2] = ["base", "head"];

/// Derive brief-only facts from captured revisions and keep only relations touching changed paths.
pub(super) fn prepare(
    capture: &ReviewCapture,
    changes: &[ReviewContextChange],
    relations: &mut Vec<ReviewContextRelation>,
    context: &[ReviewContextFile],
) -> Result<ReviewBrief> {
    let changed_paths: BTreeSet<_> = changes
        .iter()
        .flat_map(|change| {
            [
                (SIDES[0], change.base.as_ref()),
                (SIDES[1], change.head.as_ref()),
            ]
        })
        .filter_map(|(side, file)| Some((side, file?.path.clone())))
        .collect();
    let entry_chains = entry_chains(relations, context);
    relations.retain(|relation| {
        relation.kind == "symbol-reference"
            || [&relation.edge.source, &relation.edge.target]
                .into_iter()
                .any(|path| changed_paths.contains(&(relation.side.as_str(), PathBuf::from(path))))
    });
    let mut brief = ReviewBrief {
        direct_relations: relations.len(),
        entry_chains,
        ..ReviewBrief::default()
    };
    for (index, side) in SIDES.into_iter().enumerate() {
        let files = context.iter().filter(|file| file.side == side);
        brief.transitive_dependents[index] =
            files.clone().filter(|file| file.distance >= 2).count();
        brief.config_scope_files[index] = files
            .filter(|file| {
                file.roles
                    .iter()
                    .all(|role| role == "resolver-config-scope")
            })
            .count();
    }
    let declared = declared(capture);
    brief.terms = terms(changes, &declared);
    mentions(capture, &changed_paths, &mut brief)?;
    usage(capture, changes, relations, &declared, &mut brief)?;
    Ok(brief)
}

/// Shortest head-side chain from each changed file to its nearest dependent without non-test importers.
fn entry_chains(
    relations: &[ReviewContextRelation],
    context: &[ReviewContextFile],
) -> Vec<Vec<PathBuf>> {
    let head: BTreeMap<_, _> = context
        .iter()
        .filter(|file| file.side == SIDES[1])
        .map(|file| (file.path.as_path(), file))
        .collect();
    let is_test = |path: &Path| {
        head.get(path)
            .is_some_and(|file| !file.test_evidence.is_empty())
    };
    let imported: BTreeSet<_> = relations
        .iter()
        .filter(|relation| relation.side == SIDES[1] && relation.kind == "import")
        .filter(|relation| !is_test(Path::new(&relation.edge.source)))
        .map(|relation| PathBuf::from(&relation.edge.target))
        .collect();
    let mut nearest = BTreeMap::<PathBuf, Vec<PathBuf>>::new();
    for file in head.values() {
        if file.distance < 2
            || file.distance > MAX_CHAIN_DISTANCE
            || !file.test_evidence.is_empty()
            || imported.contains(&file.path)
            || !file.roles.iter().any(|role| role.ends_with("dependent"))
        {
            continue;
        }
        let mut chain = vec![file.path.clone()];
        let mut current = *file;
        while current.distance > 0 {
            let Some(next) = current
                .via
                .as_deref()
                .and_then(|via| head.get(via).copied())
            else {
                break;
            };
            if next.distance >= current.distance {
                break;
            }
            chain.push(next.path.clone());
            current = next;
        }
        if current.distance != 0 {
            continue;
        }
        let seed = chain.pop().unwrap_or_default();
        // Candidates iterate in path order, so the first chain of equal length wins.
        if nearest
            .get(&seed)
            .is_none_or(|existing| chain.len() < existing.len())
        {
            nearest.insert(seed, chain);
        }
    }
    nearest
        .into_iter()
        .map(|(seed, mut chain)| {
            chain.push(seed);
            chain
        })
        .collect()
}

/// Declaration names captured in either revision.
fn declared(capture: &ReviewCapture) -> BTreeSet<&str> {
    capture
        .revisions
        .iter()
        .flat_map(|revision| revision.files().values())
        .flat_map(|file| &file.definitions.definitions)
        .map(|definition| definition.symbol.name.as_str())
        .collect()
}

/// Changed declaration names, then declared names and identifier-like literals on changed lines.
fn terms(changes: &[ReviewContextChange], declared: &BTreeSet<&str>) -> Vec<String> {
    let mut ordered = Vec::new();
    for change in changes {
        for side in [change.base.as_ref(), change.head.as_ref()]
            .into_iter()
            .flatten()
        {
            ordered.extend(side.definitions.iter().map(|item| item.symbol.name.clone()));
        }
    }
    let changed_lines: Vec<&str> = changes
        .iter()
        .filter_map(|change| change.diff.as_deref())
        .flat_map(|diff| {
            diff.lines()
                .skip_while(|line| !line.starts_with("@@"))
                .filter(|line| line.starts_with('+') || line.starts_with('-'))
                .map(|line| &line[1..])
        })
        .collect();
    for line in &changed_lines {
        ordered.extend(
            identifiers(line)
                .filter(|word| declared.contains(word))
                .map(str::to_string),
        );
    }
    for line in &changed_lines {
        ordered.extend(literals(line));
    }
    let mut seen = BTreeSet::new();
    ordered
        .into_iter()
        .filter(|term| term.chars().count() >= MIN_TERM_CHARS && seen.insert(term.clone()))
        .take(MAX_TERMS)
        .collect()
}

fn identifiers(line: &str) -> impl Iterator<Item = &str> {
    line.split(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
        .filter(|word| word.chars().next().is_some_and(|ch| !ch.is_numeric()))
}

/// Quoted identifier-like values such as status names, codes and keys.
fn literals(line: &str) -> Vec<String> {
    let mut found = Vec::new();
    for quote in ['"', '\'', '`'] {
        let mut parts = line.split(quote);
        parts.next();
        while let (Some(inside), Some(_)) = (parts.next(), parts.clone().next()) {
            if inside.chars().next().is_some_and(char::is_alphabetic)
                && inside
                    .chars()
                    .all(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':' | '/'))
            {
                found.push(inside.to_string());
            }
            parts.next();
        }
    }
    found
}

/// Lowercase words of an identifier, split at separators and lower-to-upper case changes.
fn words(term: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut previous_lower = false;
    for ch in term.chars() {
        if !ch.is_alphanumeric() {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            previous_lower = false;
            continue;
        }
        if ch.is_uppercase() && previous_lower && !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
        previous_lower = ch.is_lowercase() || ch.is_numeric();
        current.extend(ch.to_lowercase());
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

fn normalized(line: &str) -> String {
    let mut output = String::from(" ");
    for word in words(line) {
        output.push_str(&word);
        output.push(' ');
    }
    output
}

/// Compound terms match as identifiers or word phrases. Single words match exactly as whole
/// identifiers; all-lowercase words are ordinary prose, so they must be quoted as code.
fn matches(term: &str, phrase: Option<&str>, line: &str, normalized_line: &str) -> bool {
    if let Some(phrase) = phrase {
        return normalized_line.contains(phrase);
    }
    whole(term, line, !term.chars().any(char::is_uppercase))
}

/// Case-sensitive occurrence delimited by quotes, or by non-identifier characters.
fn whole(term: &str, line: &str, quoted: bool) -> bool {
    line.match_indices(term).any(|(start, _)| {
        let before = line[..start].chars().next_back();
        let after = line[start + term.len()..].chars().next();
        if quoted {
            let quote = |ch: Option<char>| matches!(ch, Some('`' | '"' | '\''));
            quote(before) && quote(after)
        } else {
            let boundary =
                |ch: Option<char>| ch.is_none_or(|ch| !(ch.is_alphanumeric() || ch == '_'));
            boundary(before) && boundary(after)
        }
    })
}

fn mentions(
    capture: &ReviewCapture,
    changed: &BTreeSet<(&str, PathBuf)>,
    brief: &mut ReviewBrief,
) -> Result<()> {
    let patterns: Vec<_> = brief
        .terms
        .iter()
        .map(|term| {
            let words = words(term);
            let phrase = (words.len() > 1).then(|| format!(" {} ", words.join(" ")));
            (term.as_str(), phrase)
        })
        .collect();
    if patterns.is_empty() {
        return Ok(());
    }
    let mut documents = Vec::new();
    for (path, file) in capture.revisions[1].files() {
        super::check_deadline(capture.deadline)?;
        if !matches!(file.language.as_str(), "Markdown" | "Text")
            || changed.contains(&(SIDES[1], path.clone()))
        {
            continue;
        }
        let mut lines = Vec::new();
        let mut terms = BTreeSet::new();
        for (index, line) in file.content.lines().enumerate() {
            let normalized_line = normalized(line);
            let hits: BTreeSet<_> = patterns
                .iter()
                .enumerate()
                .filter(|(_, (term, phrase))| {
                    matches(term, phrase.as_deref(), line, &normalized_line)
                })
                .map(|(index, _)| index)
                .collect();
            if !hits.is_empty() {
                lines.push((hits.len(), index + 1, line.trim()));
                terms.extend(hits);
            }
        }
        if !lines.is_empty() {
            brief.mentions_total += lines.len();
            documents.push((terms.len(), path, lines));
        }
    }
    documents.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(right.1)));
    for (_, path, mut lines) in documents {
        lines.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.cmp(&right.1)));
        lines.truncate(MAX_MENTIONS_PER_FILE);
        lines.sort_by_key(|line| line.1);
        for (_, line, text) in lines {
            if brief.mentions.len() == MAX_MENTIONS {
                return Ok(());
            }
            brief.mentions.push(ReviewDocMention {
                path: path.clone(),
                line,
                text: clip(text),
            });
        }
    }
    Ok(())
}

/// One search term for code: `direct` terms come from the change itself rather than from an
/// enclosing declaration, and `quoted` terms only match inside string quotes.
struct CodeTerm {
    text: String,
    direct: bool,
    quoted: bool,
}

/// Literal-only lowercase single words are ordinary code words unless quoted, as in documentation.
fn code_term(text: String, direct: bool, declared: &BTreeSet<&str>) -> CodeTerm {
    let quoted = !declared.contains(text.as_str())
        && words(&text).len() == 1
        && !text.chars().any(char::is_uppercase);
    CodeTerm {
        text,
        direct,
        quoted,
    }
}

/// Change terms, then names of the changed file's declarations whose spans use one of them.
fn change_terms(
    capture: &ReviewCapture,
    change: &ReviewContextChange,
    declared: &BTreeSet<&str>,
) -> Vec<CodeTerm> {
    let mut terms: Vec<_> = terms(std::slice::from_ref(change), declared)
        .into_iter()
        .map(|term| code_term(term, true, declared))
        .collect();
    for (index, side) in [change.base.as_ref(), change.head.as_ref()]
        .into_iter()
        .enumerate()
    {
        let Some(file) = side.and_then(|side| capture.revisions[index].files().get(&side.path))
        else {
            continue;
        };
        let lines: Vec<_> = file.content.lines().collect();
        for definition in &file.definitions.definitions {
            let name = &definition.symbol.name;
            if terms.len() >= MAX_TERMS
                || name.chars().count() < MIN_TERM_CHARS
                || terms.iter().any(|term| &term.text == name)
            {
                continue;
            }
            let span = &definition.declaration_span;
            let body = lines
                .get(span.start_line.saturating_sub(1)..span.end_line.min(lines.len()))
                .unwrap_or_default();
            if body.iter().any(|line| {
                terms
                    .iter()
                    .filter(|term| term.direct)
                    .any(|term| whole(&term.text, line, term.quoted))
            }) {
                terms.push(code_term(name.clone(), false, declared));
            }
        }
    }
    terms
}

/// Import statements, including bracketed continuation lines.
fn import_lines(content: &str) -> Vec<bool> {
    let mut depth = 0_isize;
    content
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            let starts = depth > 0
                || ["import ", "import{", "from ", "use ", "pub use "]
                    .iter()
                    .any(|prefix| trimmed.starts_with(prefix))
                || (trimmed.starts_with("export ") && trimmed.contains(" from "))
                || trimmed.contains("require(");
            if starts {
                let opened = trimmed.matches(['(', '{']).count();
                let closed = trimmed.matches([')', '}']).count();
                depth = (depth + opened.cast_signed() - closed.cast_signed()).max(0);
            }
            starts
        })
        .collect()
}

/// Local names bound by `term as alias` on an import line.
fn alias<'a>(term: &str, line: &'a str) -> Option<&'a str> {
    line.match_indices(term).find_map(|(start, _)| {
        let rest = line[start + term.len()..].strip_prefix(char::is_whitespace)?;
        let rest = rest.trim_start().strip_prefix("as")?;
        let rest = rest.strip_prefix(char::is_whitespace)?.trim_start();
        let end = rest
            .find(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
            .unwrap_or(rest.len());
        (end > 0).then(|| &rest[..end])
    })
}

/// Ranked candidate lines of one importer: non-import lines unless only import lines match,
/// direct-term lines first, then more distinct terms, then line order.
fn importer_lines<'a>(
    content: &'a str,
    terms: &[CodeTerm],
    skip: impl Fn(usize) -> bool,
) -> Vec<(usize, &'a str)> {
    let imports = import_lines(content);
    let mut local: Vec<(&str, bool, bool)> = terms
        .iter()
        .map(|term| (term.text.as_str(), term.direct, term.quoted))
        .collect();
    for (line, _) in content.lines().zip(&imports).filter(|(_, import)| **import) {
        for term in terms {
            if whole(&term.text, line, term.quoted)
                && let Some(name) = alias(&term.text, line)
                && !local.iter().any(|(text, ..)| *text == name)
            {
                local.push((name, true, false));
            }
        }
    }
    let mut hits = Vec::new();
    for ((index, line), import) in content.lines().enumerate().zip(&imports) {
        if skip(index + 1) {
            continue;
        }
        let matched: Vec<_> = local
            .iter()
            .filter(|(text, _, quoted)| whole(text, line, *quoted))
            .collect();
        if !matched.is_empty() {
            let direct = matched.iter().any(|(_, direct, _)| *direct);
            hits.push((
                *import,
                !direct,
                usize::MAX - matched.len(),
                index + 1,
                line,
            ));
        }
    }
    if hits.iter().any(|hit| !hit.0) {
        hits.retain(|hit| !hit.0);
    }
    hits.sort_unstable();
    hits.into_iter()
        .map(|(_, _, _, line, text)| (line, text.trim()))
        .collect()
}

/// Lines of direct importers without resolved references to the changed file that name
/// identifiers of the change, allocated round-robin across importers.
fn usage(
    capture: &ReviewCapture,
    changes: &[ReviewContextChange],
    relations: &[ReviewContextRelation],
    declared: &BTreeSet<&str>,
    brief: &mut ReviewBrief,
) -> Result<()> {
    let reference_sites: BTreeSet<_> = relations
        .iter()
        .filter_map(|relation| Some((relation.side.as_str(), relation.symbol.as_ref()?)))
        .map(|(side, symbol)| (side, symbol.source.path.as_str(), symbol.site.start_line))
        .collect();
    let mut claimed = BTreeSet::new();
    let mut importers = Vec::new();
    for change in changes {
        let changed_sides = [change.base.as_ref(), change.head.as_ref()];
        let terms = change_terms(capture, change, declared);
        if terms.is_empty() {
            continue;
        }
        let mut sources = BTreeMap::<&str, usize>::new();
        let mut referenced = BTreeSet::new();
        for relation in relations {
            let index = usize::from(relation.side == SIDES[1]);
            let Some(changed_path) = changed_sides[index].map(|side| side.path.as_path()) else {
                continue;
            };
            if let Some(symbol) = &relation.symbol {
                if Path::new(&symbol.target.path) == changed_path {
                    referenced.insert(symbol.source.path.as_str());
                }
            } else if relation.kind == "import"
                && Path::new(&relation.edge.target) == changed_path
                && Path::new(&relation.edge.source) != changed_path
            {
                let side = sources
                    .entry(relation.edge.source.as_str())
                    .or_insert(index);
                *side = (*side).max(index);
            }
        }
        for (path, index) in sources {
            super::check_deadline(capture.deadline)?;
            let path = PathBuf::from(path);
            let side = SIDES[index];
            let Some(file) = capture.revisions[index].files().get(&path) else {
                continue;
            };
            let key = path.to_string_lossy().into_owned();
            if referenced.contains(key.as_str()) {
                continue;
            }
            // A changed importer's own hunks are already shown in its diff.
            let ranges: Vec<_> = changes
                .iter()
                .filter_map(|other| [other.base.as_ref(), other.head.as_ref()][index])
                .filter(|other| other.path == path)
                .flat_map(|other| other.ranges.iter().map(|range| range.start..=range.end))
                .collect();
            let lines: Vec<_> = importer_lines(&file.content, &terms, |line| {
                ranges.iter().any(|range| range.contains(&line))
                    || reference_sites.contains(&(side, key.as_str(), line))
                    || claimed.contains(&(side, key.clone(), line))
            })
            .into_iter()
            .map(|(line, text)| ReviewUsageLine {
                change: changed_sides.map(|side| side.map(|side| side.path.clone())),
                side: side.into(),
                path: path.clone(),
                line,
                text: clip(text),
            })
            .collect();
            claimed.extend(lines.iter().map(|line| (side, key.clone(), line.line)));
            brief.usage_total += lines.len();
            importers.push(lines);
        }
    }
    for round in 0..MAX_USAGE_PER_FILE {
        for lines in &importers {
            if let Some(line) = lines.get(round) {
                if brief.usage.len() == MAX_USAGE {
                    return Ok(());
                }
                brief.usage.push(line.clone());
            }
        }
    }
    Ok(())
}

fn clip(text: &str) -> String {
    if text.chars().count() <= MAX_MENTION_CHARS {
        return text.to_string();
    }
    let mut clipped: String = text.chars().take(MAX_MENTION_CHARS - 1).collect();
    clipped.push('…');
    clipped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_split_into_phrase_words() {
        assert_eq!(words("event_identity"), ["event", "identity"]);
        assert_eq!(words("providerReceiptKey"), ["provider", "receipt", "key"]);
        assert_eq!(words("HTTPServer"), ["httpserver"]);
        let line = "Event identity is the tuple";
        assert!(matches(
            "event_identity",
            Some(" event identity "),
            line,
            &normalized(line)
        ));
        assert!(!matches(
            "event_identity",
            Some(" event identity "),
            "eventidentity",
            &normalized("eventidentity")
        ));
        let line = "Use `eventIdentity` or EVENT_IDENTITY";
        assert_eq!(normalized(line), " use event identity or event identity ");
        assert!(matches(
            "event_identity",
            Some(" event identity "),
            line,
            &normalized(line)
        ));
    }

    #[test]
    fn single_words_match_whole_case_sensitive_identifiers() {
        let line = "The WELCOME promotion; WELCOMEX and welcome differ";
        assert!(matches("WELCOME", None, line, &normalized(line)));
        assert!(!matches("WELCOM", None, line, &normalized(line)));
        assert!(!matches("welcomes", None, line, &normalized(line)));
    }

    #[test]
    fn lowercase_single_words_must_be_quoted_as_code() {
        let prose = "IDs returned by one request";
        assert!(!matches("returned", None, prose, &normalized(prose)));
        let table = "| some shipped units returned | `returned` |";
        assert!(matches("returned", None, table, &normalized(table)));
    }

    #[test]
    fn import_statements_include_bracketed_continuations() {
        let source = "from a import (\n    b as c,\n)\nx = c()\nimport {\n  d,\n} from './d';\nuse e::f;\nconst g = require('./g');\nfrom_value = 1\n";
        assert_eq!(
            import_lines(source),
            [true, true, true, false, true, true, true, true, true, false]
        );
        assert_eq!(alias("b", "    b as c,"), Some("c"));
        assert_eq!(alias("b", "import { b } from './b'"), None);
        assert_eq!(alias("b", "b assert"), None);
    }

    #[test]
    fn code_terms_quote_only_undeclared_lowercase_words() {
        let declared = BTreeSet::from(["compute"]);
        assert!(!code_term("compute".into(), true, &declared).quoted);
        assert!(code_term("returned".into(), true, &declared).quoted);
        assert!(!code_term("partially_returned".into(), true, &declared).quoted);
        assert!(!code_term("WELCOME".into(), true, &declared).quoted);
        let terms = [code_term("returned".into(), true, &declared)];
        let lines = importer_lines("returned = 1\nkey = \"returned\"\n", &terms, |_| false);
        assert_eq!(lines, [(2, "key = \"returned\"")]);
    }

    #[test]
    fn literals_keep_identifier_like_quoted_values() {
        assert_eq!(
            literals(r#"x = "partially_returned" if y == 'WELCOME' else "two words""#),
            ["partially_returned", "WELCOME"]
        );
        assert!(literals("unterminated \"value").is_empty());
    }
}
