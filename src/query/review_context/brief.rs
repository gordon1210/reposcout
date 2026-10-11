mod bindings;
mod hunks;
mod lexicon;
mod moves;
mod rewrites;
mod stale;

use crate::model::{
    CallReferenceStatus, DefinitionStatus, ReviewBrief, ReviewBriefHunk, ReviewContextChange,
    ReviewContextFile, ReviewContextRelation, ReviewDocMention, ReviewHunkRole, ReviewUsageLine,
};
use crate::scan::ReviewCapture;
use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
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
/// Definition kinds whose names identify call or construction sites in importers.
const CALLABLE_KINDS: [&str; 4] = ["function", "method", "class", "constant"];

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
    brief.call_gaps = call_gaps(capture, changes);
    let declared_sides = [declared(capture, 0), declared(capture, 1)];
    let declared: BTreeSet<&str> = declared_sides.iter().flatten().copied().collect();
    diff_facts(
        capture,
        changes,
        context,
        [&declared_sides[0], &declared_sides[1]],
        &mut brief,
    )?;
    brief.terms = terms(changes);
    mentions(capture, &changed_paths, &mut brief)?;
    usage(capture, changes, relations, &declared, &mut brief)?;
    // Lines already listed as old spellings still in head are not repeated as mentions or usage.
    let stale: BTreeSet<_> = brief
        .stale
        .iter()
        .flat_map(|name| &name.hits)
        .map(|hit| (hit.path.clone(), hit.line))
        .collect();
    brief
        .mentions
        .retain(|mention| !stale.contains(&(mention.path.clone(), mention.line)));
    brief
        .usage
        .retain(|line| line.side != SIDES[1] || !stale.contains(&(line.path.clone(), line.line)));
    Ok(brief)
}

/// Per revision: changed code files without call-reference support, and with incomplete references.
fn call_gaps(capture: &ReviewCapture, changes: &[ReviewContextChange]) -> [[usize; 2]; 2] {
    let mut gaps = [[0; 2]; 2];
    for change in changes {
        for (index, side) in [change.base.as_ref(), change.head.as_ref()]
            .into_iter()
            .enumerate()
        {
            let Some(side) = side.filter(|side| {
                matches!(
                    side.extraction,
                    DefinitionStatus::Available | DefinitionStatus::ParseErrors
                )
            }) else {
                continue;
            };
            let status = capture.revisions[index]
                .files()
                .get(&side.path)
                .and_then(|file| file.graph_facts.as_ref())
                .and_then(|facts| facts.call_references.as_ref())
                .map(|facts| facts.status);
            match status {
                Some(CallReferenceStatus::Available) => {}
                Some(CallReferenceStatus::Unsupported) | None => gaps[index][0] += 1,
                Some(_) => gaps[index][1] += 1,
            }
        }
    }
    gaps
}

/// Languages where leading whitespace never changes meaning; every other language counts as
/// indentation-sensitive.
const INDENTATION_FREE: [&str; 26] = [
    "Rust",
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
    "Lua",
    "SQL",
    "HTML",
    "CSS",
    "SCSS",
    "JSON",
    "TOML",
    "XML",
    "Godot Shader",
    "Godot Scene",
    "Godot Resource",
    "Godot Project",
];

/// Leading whitespace may change the meaning of the change's file: its captured language is
/// not known to ignore indentation.
fn indentation_sensitive(capture: &ReviewCapture, change: &ReviewContextChange) -> bool {
    let language = [(1, change.head.as_ref()), (0, change.base.as_ref())]
        .into_iter()
        .find_map(|(index, side)| {
            capture.revisions[index]
                .files()
                .get(&side?.path)
                .map(|file| file.language.as_str())
        });
    !language.is_some_and(|language| INDENTATION_FREE.contains(&language))
}

/// Text, declaration spans and test evidence of each change's base and head file.
fn move_sides<'a>(
    capture: &'a ReviewCapture,
    changes: &[ReviewContextChange],
    tests: &BTreeSet<(&str, &Path)>,
) -> Vec<moves::Sides<'a>> {
    changes
        .iter()
        .map(|change| {
            let mut sides = moves::Sides::default();
            for (index, side) in [change.base.as_ref(), change.head.as_ref()]
                .into_iter()
                .enumerate()
            {
                let Some(side) = side else {
                    continue;
                };
                sides.test[index] = tests.contains(&(SIDES[index], side.path.as_path()));
                let Some(file) = capture.revisions[index].files().get(&side.path) else {
                    continue;
                };
                sides.text[index] = Some(file.content.as_str());
                sides.spans[index] = file
                    .definitions
                    .definitions
                    .iter()
                    .map(|definition| {
                        let span = &definition.declaration_span;
                        (span.start_line, span.end_line)
                    })
                    .collect();
            }
            sides
        })
        .collect()
}

/// Hunks with their rewrite, move and display facts, moves, change entries, renames and stale
/// names.
/// Per change, every whole-identifier rename its hunks apply.
fn file_renames(parsed: &[hunks::Hunk<'_>], changes: usize) -> Vec<Vec<(String, String)>> {
    let mut renames = vec![Vec::new(); changes];
    for hunk in parsed {
        let whole = hunk
            .edits
            .iter()
            .flatten()
            .flatten()
            .filter(|edit| edit.rename)
            .filter_map(|edit| edit.whole.clone());
        if let Some(file) = renames.get_mut(hunk.change) {
            file.extend(whole);
        }
    }
    renames
}

fn diff_facts(
    capture: &ReviewCapture,
    changes: &[ReviewContextChange],
    context: &[ReviewContextFile],
    declared: [&BTreeSet<&str>; 2],
    brief: &mut ReviewBrief,
) -> Result<()> {
    let mut parsed = Vec::new();
    for (index, change) in changes.iter().enumerate() {
        let Some(diff) = change.diff.as_deref() else {
            continue;
        };
        let both = change.base.is_some() && change.head.is_some();
        for mut hunk in hunks::parse(index, diff) {
            super::check_deadline(capture.deadline)?;
            if both {
                hunk.edits = hunks::edits(&hunk)?;
            }
            parsed.push(hunk);
        }
    }
    let sensitive: Vec<bool> = changes
        .iter()
        .map(|change| indentation_sensitive(capture, change))
        .collect();
    let sensitive: Vec<bool> = parsed.iter().map(|hunk| sensitive[hunk.change]).collect();
    let tests: BTreeSet<(&str, &Path)> = context
        .iter()
        .filter(|file| !file.test_evidence.is_empty())
        .map(|file| (file.side.as_str(), file.path.as_path()))
        .collect();
    let sides = move_sides(capture, changes, &tests);
    brief.rewrites = rewrites::rewrites(&parsed);
    let rules = rewrites::Rules::new(&brief.rewrites);
    let tested: Vec<bool> = sides
        .iter()
        .map(|side| side.test[0] || side.test[1])
        .collect();
    let renames = file_renames(&parsed, changes.len());
    let lexicon = lexicon::Lexicon::new(capture, changes, &tested, &renames);
    let pure = |rename: &lexicon::Rename<'_>| lexicon.pure(rename);
    let mut explained = Vec::with_capacity(parsed.len());
    for (hunk, sensitive) in parsed.iter().zip(&sensitive) {
        super::check_deadline(capture.deadline)?;
        explained.push(rules.explains(hunk, *sensitive, &pure)?);
    }
    let skip: Vec<bool> = explained
        .iter()
        .map(|explained| *explained != rewrites::Explained::No)
        .collect();
    let matches = moves::matches(
        &parsed,
        &skip,
        &sensitive,
        &sides,
        &brief.rewrites,
        moves::Checks {
            pure: &pure,
            keeps: &|source, target, moved, lines, folded| {
                lexicon.keeps_bindings(source, target, moved, lines, folded)
            },
        },
        capture.deadline,
    )?;
    super::check_deadline(capture.deadline)?;
    for (index, hunk) in parsed.iter().enumerate() {
        let change = &changes[hunk.change];
        let test = match (&change.head, &change.base) {
            (Some(side), _) => tests.contains(&(SIDES[1], side.path.as_path())),
            (None, Some(side)) => tests.contains(&(SIDES[0], side.path.as_path())),
            (None, None) => false,
        };
        let mut brief_hunk = brief_hunk(
            hunk,
            change,
            explained[index],
            &matches.fates[index],
            &rules,
            test,
            sensitive[index] && hunk.reindented(),
        );
        brief_hunk
            .moved_sites
            .clone_from(&matches.moved_sites[index]);
        brief.hunks.push(brief_hunk);
    }
    // A hunk receiving moved lines with renames is one more site of each such rewrite.
    for hunk in &brief.hunks {
        let rules: BTreeSet<usize> = hunk
            .moved_sites
            .iter()
            .map(|(rule, _)| *rule)
            .filter(|rule| !hunk.rewrites.contains(rule))
            .collect();
        for rule in rules {
            brief.rewrites[rule].hunks += 1;
        }
    }
    brief.moves = matches.moves;
    brief.examples = rewrites::examples(&brief.hunks, brief.rewrites.len());
    classify(changes, brief);
    let edits = parsed
        .iter()
        .flat_map(|hunk| hunk.edits.iter().flatten().flatten());
    brief.stale = stale::stale(capture, edits, &brief.rewrites, declared)?;
    Ok(())
}

/// Changes rendering as their own entry (with a shown or moved hunk, without a diff or hunks,
/// added or deleted), and renamed, copied or mode-changed files whose hunks are all rewrite-only
/// or absent.
fn classify(changes: &[ReviewContextChange], brief: &mut ReviewBrief) {
    let mut parsed = vec![0; changes.len()];
    let mut shown = vec![false; changes.len()];
    for hunk in &brief.hunks {
        parsed[hunk.change] += 1;
        shown[hunk.change] |= hunk.role != ReviewHunkRole::Rewrite;
    }
    for (index, change) in changes.iter().enumerate() {
        let renamed = match (&change.base, &change.head) {
            (Some(base), Some(head)) => base.path != head.path || base.mode != head.mode,
            _ => false,
        };
        let added_or_deleted = change.base.is_none() || change.head.is_none();
        if shown[index] || change.diff.is_none() || added_or_deleted {
            brief.entry_changes.push(index);
        } else if renamed {
            brief.renames.push(index);
        } else if parsed[index] == 0 {
            brief.entry_changes.push(index);
        }
    }
}

/// One hunk's role, hidden lines, deviation, priority and displayed text. A `reindented` hunk
/// of an indentation-sensitive file shows in full with its context and without rewrites.
fn brief_hunk(
    hunk: &hunks::Hunk<'_>,
    change: &ReviewContextChange,
    explained: rewrites::Explained,
    fates: &[moves::Fate],
    rules: &rewrites::Rules<'_>,
    test: bool,
    reindented: bool,
) -> ReviewBriefHunk {
    let edited: Vec<String> = hunk
        .lines
        .iter()
        .filter(|line| line.tag != ' ')
        .map(|line| format!("{}{}", line.tag, line.text))
        .collect();
    let count = |fate| {
        hunk.lines
            .iter()
            .zip(fates)
            .filter(|(line, current)| line.tag != ' ' && **current == fate)
            .count()
    };
    let moved_lines = count(moves::Fate::Moved);
    let mut unchanged_lines = count(moves::Fate::Unchanged);
    // A hunk whose lines all equal their counterparts after rewrites shows them.
    if reindented || (moved_lines == 0 && unchanged_lines == edited.len()) {
        unchanged_lines = 0;
    }
    let hidden: Vec<bool> = fates
        .iter()
        .map(|fate| match fate {
            moves::Fate::Changed => false,
            moves::Fate::Unchanged => unchanged_lines > 0,
            moves::Fate::Moved => !reindented,
        })
        .collect();
    let role = if explained == rewrites::Explained::Renames {
        ReviewHunkRole::Rewrite
    } else if !edited.is_empty() && moved_lines == edited.len() {
        ReviewHunkRole::Moved
    } else {
        ReviewHunkRole::Shown
    };
    let shown = role == ReviewHunkRole::Shown;
    let any_hidden = hunk
        .lines
        .iter()
        .zip(&hidden)
        .any(|(line, hidden)| line.tag != ' ' && *hidden);
    let variant_of = (shown && !reindented)
        .then(|| rules.variant_of(hunk))
        .flatten();
    let compact = explained == rewrites::Explained::Rewrites;
    // Rule-explained hunks after every other hunk. Before them, every non-test hunk precedes
    // every test hunk, each group ordered deviating, structural, line-for-line, deletion.
    let priority = if compact {
        8 + u8::from(test)
    } else {
        let kind = if variant_of.is_some() {
            0
        } else if change.head.is_none() {
            3
        } else {
            1 + u8::from(hunk.compact())
        };
        kind + 4 * u8::from(test)
    };
    ReviewBriefHunk {
        change: hunk.change,
        old_start: hunk.old_start,
        old_lines: hunk.old_lines,
        new_start: hunk.new_start,
        new_lines: hunk.new_lines,
        role,
        rewrites: if reindented {
            Vec::new()
        } else {
            rules.used(hunk)
        },
        variant_of,
        moved_lines: if reindented && shown { 0 } else { moved_lines },
        unchanged_lines,
        site_line: hunk.site_line(change.head.is_some()),
        // Lines whose block edits are all rewrites but that are no rewrite of their
        // counterpart show with their context.
        text: if shown && (reindented || (!compact && rules.ruled(hunk) && !any_hidden)) {
            raw(hunk)
        } else if shown {
            body(hunk, &hidden, compact)
        } else {
            String::new()
        },
        changed: edited,
        priority,
        test,
        explained: compact,
        moved_sites: Vec::new(),
    }
}

/// Displayed hunk body: the whole hunk, only the changed lines of a line-for-line hunk, or the
/// changed lines not `hidden`, numbered where a run of consecutive lines starts. A `numbered`
/// hunk shows its changed lines numbered that way.
fn body(hunk: &hunks::Hunk<'_>, hidden: &[bool], numbered: bool) -> String {
    let mut text = String::new();
    let any_hidden = numbered
        || hunk
            .lines
            .iter()
            .zip(hidden)
            .any(|(line, hidden)| line.tag != ' ' && *hidden);
    if !any_hidden && !hunk.compact() {
        return raw(hunk);
    }
    text.push_str(&hunk.range_header());
    text.push('\n');
    let mut previous = None;
    for (line, hidden) in hunk.lines.iter().zip(hidden) {
        if line.tag == ' ' || *hidden {
            continue;
        }
        if !any_hidden || previous == Some((line.tag, line.number.wrapping_sub(1))) {
            text.push(line.tag);
            text.push_str(line.text);
        } else {
            let _ = write!(text, "{}{}: {}", line.tag, line.number, line.text);
        }
        previous = Some((line.tag, line.number));
        text.push('\n');
    }
    text
}

/// The whole hunk with its context lines.
fn raw(hunk: &hunks::Hunk<'_>) -> String {
    let mut text = hunk.raw.to_string();
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
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

/// Declaration names captured in one revision.
fn declared(capture: &ReviewCapture, side: usize) -> BTreeSet<&str> {
    capture.revisions[side]
        .files()
        .values()
        .flat_map(|file| &file.definitions.definitions)
        .map(|definition| definition.symbol.name.as_str())
        .collect()
}

/// Changed declaration names of at least two words, searched in documentation.
fn terms(changes: &[ReviewContextChange]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    changes
        .iter()
        .flat_map(|change| [change.base.as_ref(), change.head.as_ref()])
        .flatten()
        .flat_map(|side| &side.definitions)
        .map(|definition| definition.symbol.name.clone())
        .filter(|term| {
            term.chars().count() >= MIN_TERM_CHARS
                && words(term).len() >= 2
                && seen.insert(term.clone())
        })
        .take(MAX_TERMS)
        .collect()
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

/// Indices of terms that occur on `line` as one identifier token in any case style, such as
/// `eventIdentity`, `event_identity` or `EVENT_IDENTITY` for `event_identity`.
fn identifier_hits(line: &str, phrases: &BTreeMap<String, Vec<usize>>) -> BTreeSet<usize> {
    line.split(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
        .filter(|token| token.chars().next().is_some_and(|ch| !ch.is_numeric()))
        .filter_map(|token| phrases.get(&words(token).join(" ")))
        .flatten()
        .copied()
        .collect()
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
    let mut phrases = BTreeMap::<String, Vec<usize>>::new();
    for (index, term) in brief.terms.iter().enumerate() {
        phrases
            .entry(words(term).join(" "))
            .or_default()
            .push(index);
    }
    if phrases.is_empty() {
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
            let hits = identifier_hits(line, &phrases);
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

/// Changed callable declarations, then callable declarations of the changed file whose spans
/// use one of them.
fn change_terms(
    capture: &ReviewCapture,
    change: &ReviewContextChange,
    declared: &BTreeSet<&str>,
) -> Vec<CodeTerm> {
    let mut terms: Vec<CodeTerm> = Vec::new();
    for definition in [change.base.as_ref(), change.head.as_ref()]
        .into_iter()
        .flatten()
        .flat_map(|side| &side.definitions)
    {
        let name = &definition.symbol.name;
        if CALLABLE_KINDS.contains(&definition.symbol.kind.as_str())
            && name.chars().count() >= MIN_TERM_CHARS
            && terms.len() < MAX_TERMS
            && !terms.iter().any(|term| &term.text == name)
        {
            terms.push(code_term(name.clone(), true, declared));
        }
    }
    if terms.is_empty() {
        return terms;
    }
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
                || !CALLABLE_KINDS.contains(&definition.symbol.kind.as_str())
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
    fn doc_terms_match_identifier_tokens_in_any_case_style() {
        assert_eq!(words("event_identity"), ["event", "identity"]);
        assert_eq!(words("providerReceiptKey"), ["provider", "receipt", "key"]);
        assert_eq!(words("HTTPServer"), ["httpserver"]);
        let phrases = BTreeMap::from([("event identity".to_string(), vec![0])]);
        assert_eq!(
            identifier_hits("Use `eventIdentity` or EVENT_IDENTITY", &phrases),
            BTreeSet::from([0])
        );
        assert!(identifier_hits("Event identity is the tuple", &phrases).is_empty());
        assert!(identifier_hits("eventidentity", &phrases).is_empty());
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
}
