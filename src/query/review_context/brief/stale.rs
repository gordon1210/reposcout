//! Distinctive old spellings that the change replaced but the head snapshot still contains.

use super::hunks::Edit;
use crate::model::{ReviewDocMention, ReviewRewrite, ReviewStaleName};
use crate::scan::ReviewCapture;
use anyhow::Result;
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::{BTreeMap, BTreeSet};

/// Head occurrences kept per spelling; the head line count stays exact.
const MAX_HITS: usize = 64;
const MIN_CHARS: usize = 4;

/// Old spellings of renames that head no longer declares, whose line count drops from base to
/// head but stays above zero, ordered by spelling. A rename is a single-token replacement of the
/// same kind (identifier or literal) that keeps part of the name or repeats as a rewrite.
pub(super) fn stale<'a>(
    capture: &ReviewCapture,
    edits: impl Iterator<Item = &'a Edit>,
    rewrites: &[ReviewRewrite],
    declared: [&BTreeSet<&str>; 2],
) -> Result<Vec<ReviewStaleName>> {
    let rewrites: BTreeSet<_> = rewrites
        .iter()
        .map(|rule| (rule.old.as_str(), rule.new.as_str()))
        .collect();
    let mut candidates = BTreeMap::<String, String>::new();
    for edit in edits {
        let Some((old, new)) = &edit.whole else {
            continue;
        };
        let (Some((old, literal)), Some((new, new_literal))) = (spelling(old), spelling(new))
        else {
            continue;
        };
        // Replacing a whole quoted word is no rename unless it repeats as a rewrite.
        let partial = edit.inside && edit.old != old;
        if literal != new_literal || !(partial || rewrites.contains(&edit.key())) {
            continue;
        }
        // A spelling still declared in head names a live definition, not a leftover.
        if old == new || declared[1].contains(old) || !distinctive(old, literal, declared) {
            continue;
        }
        candidates
            .entry(old.to_string())
            .or_insert_with(|| new.to_string());
    }
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    let names: FxHashSet<&str> = candidates.keys().map(String::as_str).collect();
    // Prose mentions a plain word in its ordinary sense unless it is quoted.
    let shaped: FxHashSet<&str> = names
        .iter()
        .copied()
        .filter(|name| code_shaped(name))
        .collect();
    let mut counts = FxHashMap::<&str, [usize; 2]>::default();
    let mut hits = FxHashMap::<&str, Vec<ReviewDocMention>>::default();
    let mut found = Vec::new();
    for (side, revision) in capture.revisions.iter().enumerate() {
        for (path, file) in revision.files() {
            super::super::check_deadline(capture.deadline)?;
            let prose = matches!(file.language.as_str(), "Markdown" | "Text");
            let words = if prose { &shaped } else { &names };
            for (index, line) in file.content.lines().enumerate() {
                found.clear();
                spellings(line, words, &names, &mut found);
                found.sort_unstable();
                found.dedup();
                for name in &found {
                    counts.entry(name).or_default()[side] += 1;
                    if side == 1 && hits.get(name).map_or(0, Vec::len) < MAX_HITS {
                        hits.entry(name).or_default().push(ReviewDocMention {
                            path: path.clone(),
                            line: index + 1,
                            text: super::clip(line.trim()),
                        });
                    }
                }
            }
        }
    }
    Ok(candidates
        .iter()
        .filter_map(|(old, new)| {
            let [base_lines, head_lines] = counts.get(old.as_str()).copied().unwrap_or_default();
            (head_lines > 0 && head_lines < base_lines).then(|| ReviewStaleName {
                old: old.clone(),
                new: new.clone(),
                base_lines,
                head_lines,
                hits: hits.remove(old.as_str()).unwrap_or_default(),
            })
        })
        .collect())
}

/// An identifier, or the inner text of a quoted literal (`true` for literals).
fn spelling(token: &str) -> Option<(&str, bool)> {
    let mut chars = token.chars();
    let first = chars.next()?;
    if matches!(first, '\'' | '"' | '`') {
        let inner = token.strip_prefix(first)?.strip_suffix(first)?;
        return (!inner.is_empty() && !inner.contains(char::is_whitespace))
            .then_some((inner, true));
    }
    ((first.is_alphabetic() || first == '_')
        && token.chars().all(|ch| ch.is_alphanumeric() || ch == '_'))
    .then_some((token, false))
}

/// Long enough and shaped like a code name, or a plain word declared in base.
fn distinctive(spelling: &str, literal: bool, declared: [&BTreeSet<&str>; 2]) -> bool {
    if spelling.chars().count() < MIN_CHARS {
        return false;
    }
    literal || code_shaped(spelling) || declared[0].contains(spelling)
}

/// A lower-to-upper case transition, a digit or a code separator.
fn code_shaped(spelling: &str) -> bool {
    spelling
        .chars()
        .zip(spelling.chars().skip(1))
        .any(|(left, right)| left.is_lowercase() && right.is_uppercase())
        || spelling
            .chars()
            .any(|ch| ch.is_ascii_digit() || matches!(ch, '_' | '-' | '.' | '/' | ':'))
}

/// Candidate names occurring on `line` as whole identifiers among `words`, or as whole quoted
/// segments among `names`.
fn spellings<'a>(
    line: &str,
    words: &FxHashSet<&'a str>,
    names: &FxHashSet<&'a str>,
    found: &mut Vec<&'a str>,
) {
    for word in line.split(|ch: char| !(ch.is_alphanumeric() || ch == '_')) {
        if let Some(name) = words.get(word) {
            found.push(name);
        }
    }
    // Every text between two neighbouring quotes is a candidate segment, so an apostrophe
    // earlier on the line cannot shift which segments count as quoted.
    for quote in ['\'', '"', '`'] {
        if !line.contains(quote) {
            continue;
        }
        let parts: Vec<&str> = line.split(quote).collect();
        for &inside in &parts[1..parts.len() - 1] {
            if let Some(name) = names.get(inside) {
                found.push(name);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinctive_spellings_have_code_shape_or_lost_declarations() {
        let none = BTreeSet::new();
        assert!(distinctive("laborIncrementMinutes", false, [&none, &none]));
        assert!(distinctive("job.held", true, [&none, &none]));
        assert!(!distinctive("increment", false, [&none, &none]));
        assert!(!distinctive("up", true, [&none, &none]));
        let base = BTreeSet::from(["increment"]);
        assert!(distinctive("increment", false, [&base, &none]));
        assert_eq!(spelling("'job.held'"), Some(("job.held", true)));
        assert_eq!(spelling("a.b"), None);
    }

    #[test]
    fn spellings_match_whole_identifiers_and_quoted_segments() {
        let names = FxHashSet::from_iter(["job.held", "laborIncrementMinutes", "increment"]);
        let mut found = Vec::new();
        spellings(
            "emit('job.held'); x.laborIncrementMinutes; laborIncrementMinutesX; job.held",
            &names,
            &names,
            &mut found,
        );
        assert_eq!(found, ["laborIncrementMinutes", "job.held"]);
        let shaped = FxHashSet::from_iter(["laborIncrementMinutes"]);
        found.clear();
        spellings(
            "an increment, then `increment`",
            &shaped,
            &names,
            &mut found,
        );
        assert_eq!(found, ["increment"]);
        found.clear();
        spellings("don't emit 'job.held' here", &shaped, &names, &mut found);
        assert_eq!(found, ["job.held"]);
    }
}
