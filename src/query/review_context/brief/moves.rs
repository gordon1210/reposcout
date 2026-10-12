//! Changed lines that equal their in-place counterpart after rewrites, or recur in another hunk.

use super::hunks::{Hunk, boundary, identifier, indent, line_tokens};
use super::lexicon::{Rename, Site};
use crate::model::{ReviewBriefMove, ReviewRewrite};
use anyhow::Result;
use rustc_hash::FxHashMap;
use std::collections::{BTreeMap, VecDeque};
use std::time::Instant;

/// Lines recurring in another hunk count when this many consecutive candidates move.
const MIN_RUN: usize = 3;
/// A single line recurring in another hunk counts when it is unique and this long.
const MIN_UNIQUE_TOKENS: usize = 8;
/// Changed lines above this total are not matched; every hunk then shows its lines.
const MAX_MATCHED_LINES: usize = 200_000;

/// What the brief does with one changed line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Fate {
    /// Shown.
    Changed,
    /// The line at the same position of the same block on the other side equals it byte for
    /// byte after proven pure renames.
    Unchanged,
    /// Recurs in another hunk after rewrites inside a declaration moved as a whole, or is a
    /// blank line between such lines.
    Moved,
}

pub(super) struct Matches {
    /// Per hunk and line; context lines stay `Changed`.
    pub fates: Vec<Vec<Fate>>,
    pub moves: Vec<ReviewBriefMove>,
    /// Per hunk: rename rewrites applied to moved lines it receives, as rewrite index and head
    /// line.
    pub moved_sites: Vec<Vec<(usize, usize)>>,
}

/// A removed token rewritten by rename rewrites: its index on the line, both spellings and the
/// rewrites applied.
struct Applied {
    token: usize,
    old: String,
    new: String,
    rules: Vec<usize>,
}

struct Entry {
    hunk: usize,
    change: usize,
    line: usize,
    number: usize,
    removed: bool,
    qualifies: bool,
    blank: bool,
    tokens: usize,
    key: String,
    indent: isize,
    /// Byte offset of each token on the line.
    starts: Vec<usize>,
    applied: Vec<Applied>,
    /// A removed line's text with its applied renames.
    rewritten: Option<String>,
}

/// Base and head side of one change: file text, declaration line ranges and test evidence.
#[derive(Default, Clone)]
pub(super) struct Sides<'a> {
    pub text: [Option<&'a str>; 2],
    /// One-based, inclusive.
    pub spans: [Vec<(usize, usize)>; 2],
    pub test: [bool; 2],
}

/// Proofs a fold needs: that a rename is pure, and that a declaration moved from the base file
/// of one change to head lines of another keeps the meaning of its names outside the folded
/// line ranges of the target.
#[derive(Clone, Copy)]
pub(super) struct Checks<'c> {
    pub pure: &'c dyn Fn(&Rename<'_>) -> bool,
    pub keeps: &'c Keeps<'c>,
}

/// Source change, target change, moved base lines of the source, moved head lines of the
/// target, and the target's folded head line ranges.
pub(super) type Keeps<'c> =
    dyn Fn(usize, usize, &[usize], &[usize], &[(usize, usize)]) -> bool + 'c;

/// Changed-line fates and moves. Hunks marked in `skip` are explained by rewrites and take no
/// part; `sensitive` marks hunks of indentation-sensitive files, whose lines only match with
/// identical indentation; `sides` holds each change's declarations. A line only matches after
/// renames that `checks` proves pure, and a declaration only moves when it keeps its bindings.
pub(super) fn matches(
    hunks: &[Hunk<'_>],
    skip: &[bool],
    sensitive: &[bool],
    sides: &[Sides<'_>],
    rules: &[ReviewRewrite],
    checks: Checks<'_>,
    deadline: Instant,
) -> Result<Matches> {
    let pure = checks.pure;
    let mut fates: Vec<Vec<Fate>> = hunks
        .iter()
        .map(|hunk| vec![Fate::Changed; hunk.lines.len()])
        .collect();
    let changed: usize = hunks
        .iter()
        .zip(skip)
        .filter(|(_, skip)| !**skip)
        .map(|(hunk, _)| hunk.lines.iter().filter(|line| line.tag != ' ').count())
        .sum();
    let mut moved_sites = vec![Vec::new(); hunks.len()];
    if changed > MAX_MATCHED_LINES {
        return Ok(Matches {
            fates,
            moves: Vec::new(),
            moved_sites,
        });
    }
    let entries = collect(
        hunks,
        skip,
        sensitive,
        &Spellings::new(rules),
        pure,
        &mut fates,
        deadline,
    )?;
    super::super::check_deadline(deadline)?;
    let eligible = eligible(&entries);
    let mut groups = FxHashMap::<&str, [Vec<usize>; 2]>::default();
    for (index, entry) in entries.iter().enumerate() {
        if eligible[index] {
            groups.entry(entry.key.as_str()).or_default()[usize::from(entry.removed)].push(index);
        }
    }
    let mut groups: Vec<_> = groups.into_iter().collect();
    groups.sort_unstable_by(|left, right| left.0.cmp(right.0));
    let mut partner = vec![None; entries.len()];
    for (_, [added, removed]) in groups {
        pair(&entries, &removed, &added, &mut partner);
    }
    consistent_shifts(&entries, &mut partner);
    super::super::check_deadline(deadline)?;
    declarations(&entries, sides, checks, &mut partner);
    super::super::check_deadline(deadline)?;
    let mut relocated: Vec<bool> = partner.iter().map(Option::is_some).collect();
    sandwich(&entries, &mut relocated);
    for (index, entry) in entries.iter().enumerate() {
        if relocated[index] {
            fates[entry.hunk][entry.line] = Fate::Moved;
        }
    }
    let moves = spans(&entries, &partner);
    for (index, entry) in entries.iter().enumerate() {
        let Some(other) = partner[index].filter(|_| entry.removed) else {
            continue;
        };
        let other = &entries[other];
        for applied in &entry.applied {
            for &rule in &applied.rules {
                moved_sites[other.hunk].push((rule, other.number));
            }
        }
    }
    for sites in &mut moved_sites {
        sites.sort_unstable();
        sites.dedup();
    }
    Ok(Matches {
        fates,
        moves,
        moved_sites,
    })
}

/// Every rename applied to `left` is proven pure with its token at the same position of
/// `right`.
fn pure_pair(left: &Entry, right: &Entry, pure: &dyn Fn(&Rename<'_>) -> bool) -> bool {
    left.applied.iter().all(|applied| {
        right.starts.get(applied.token).is_some_and(|&byte| {
            pure(&Rename {
                base: Site {
                    change: left.change,
                    line: left.number,
                    byte: left.starts[applied.token],
                },
                head: Site {
                    change: right.change,
                    line: right.number,
                    byte,
                },
                old: &applied.old,
                new: &applied.new,
            })
        })
    })
}

/// Changed lines of non-skipped hunks that are not unchanged in place, marking in-place
/// counterparts in `fates`.
fn collect(
    hunks: &[Hunk<'_>],
    skip: &[bool],
    sensitive: &[bool],
    spellings: &Spellings,
    pure: &dyn Fn(&Rename<'_>) -> bool,
    fates: &mut [Vec<Fate>],
    deadline: Instant,
) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for (index, hunk) in hunks.iter().enumerate() {
        if skip[index] {
            continue;
        }
        super::super::check_deadline(deadline)?;
        let first = entries.len();
        let mut at = vec![None; hunk.lines.len()];
        for (line_index, line) in hunk.lines.iter().enumerate() {
            if line.tag == ' ' {
                continue;
            }
            let (mut tokens, spans) = line_tokens(line.text);
            let word = tokens.iter().any(|token| {
                token
                    .chars()
                    .next()
                    .is_some_and(|ch| ch.is_alphabetic() || ch == '_')
            });
            let mut applied = Vec::new();
            let mut rewritten = None;
            if line.tag == '-' {
                for (position, token) in tokens.iter_mut().enumerate() {
                    let (new, rules) = spellings.removed(token);
                    if !rules.is_empty() {
                        applied.push(Applied {
                            token: position,
                            old: std::mem::replace(token, new.clone()),
                            new,
                            rules,
                        });
                    }
                }
                let mut text = String::with_capacity(line.text.len());
                let mut cursor = 0;
                for applied in &applied {
                    let span = &spans[applied.token];
                    text.push_str(&line.text[cursor..span.start]);
                    text.push_str(&applied.new);
                    cursor = span.end;
                }
                text.push_str(&line.text[cursor..]);
                rewritten = Some(text);
            }
            let plain = tokens.join(" ");
            let whitespace = indent(line.text);
            at[line_index] = Some(entries.len());
            entries.push(Entry {
                hunk: index,
                change: hunk.change,
                line: line_index,
                number: line.number,
                removed: line.tag == '-',
                qualifies: (word && tokens.len() >= 2) || tokens.len() >= 4,
                blank: line.text.trim().is_empty(),
                tokens: tokens.len(),
                indent: whitespace.chars().count().cast_signed(),
                key: if sensitive[index] {
                    format!("{whitespace}\u{0}{plain}")
                } else {
                    plain
                },
                starts: spans.iter().map(|span| span.start).collect(),
                applied,
                rewritten,
            });
        }
        let mut unchanged = Vec::new();
        for block in &hunk.blocks {
            for (left, right) in block.removed.clone().zip(block.added.clone()) {
                let (Some(left_entry), Some(right_entry)) = (at[left], at[right]) else {
                    continue;
                };
                let (left_entry, right_entry) = (&entries[left_entry], &entries[right_entry]);
                if left_entry.key == right_entry.key
                    && left_entry.rewritten.as_deref() == Some(hunk.lines[right].text)
                    && pure_pair(left_entry, right_entry, pure)
                {
                    unchanged.push((left, right));
                }
            }
        }
        for (left, right) in unchanged {
            fates[index][left] = Fate::Unchanged;
            fates[index][right] = Fate::Unchanged;
        }
        let added = entries.split_off(first);
        entries.extend(
            added
                .into_iter()
                .filter(|entry| fates[index][entry.line] != Fate::Unchanged),
        );
    }
    Ok(entries)
}

/// First and last paired line per pair of source and destination hunk.
fn spans(entries: &[Entry], partner: &[Option<usize>]) -> Vec<ReviewBriefMove> {
    let mut spans = BTreeMap::<(usize, usize), ([usize; 2], [usize; 2])>::new();
    for (index, entry) in entries.iter().enumerate() {
        let Some(other) = partner[index].filter(|_| entry.removed) else {
            continue;
        };
        let other = &entries[other];
        let span = spans
            .entry((entry.hunk, other.hunk))
            .or_insert(([entry.number; 2], [other.number; 2]));
        span.0 = [span.0[0].min(entry.number), span.0[1].max(entry.number)];
        span.1 = [span.1[0].min(other.number), span.1[1].max(other.number)];
    }
    spans
        .into_iter()
        .map(|((from, to), (from_lines, to_lines))| ReviewBriefMove {
            from,
            to,
            from_lines,
            to_lines,
        })
        .collect()
}

/// Qualifying entries whose key occurs on the opposite side in another hunk, inside a run of
/// at least `MIN_RUN` such entries on one block side (single other qualifying lines between
/// them are tolerated), or unique on both sides and at least `MIN_UNIQUE_TOKENS` long.
fn eligible(entries: &[Entry]) -> Vec<bool> {
    let mut groups = FxHashMap::<&str, [Vec<usize>; 2]>::default();
    for (index, entry) in entries.iter().enumerate() {
        if entry.qualifies {
            groups.entry(entry.key.as_str()).or_default()[usize::from(entry.removed)].push(index);
        }
    }
    let mut candidate = vec![false; entries.len()];
    let mut unique = vec![false; entries.len()];
    for sides in groups.values() {
        for (side, opposite) in [(&sides[0], &sides[1]), (&sides[1], &sides[0])] {
            let Some(&first) = opposite.first() else {
                continue;
            };
            let first = entries[first].hunk;
            let several = opposite.iter().any(|&other| entries[other].hunk != first);
            for &index in side {
                let other_hunk = several || first != entries[index].hunk;
                candidate[index] = other_hunk;
                unique[index] = other_hunk
                    && side.len() == 1
                    && opposite.len() == 1
                    && entries[index].tokens >= MIN_UNIQUE_TOKENS;
            }
        }
    }
    let mut eligible = vec![false; entries.len()];
    let mut run: Vec<usize> = Vec::new();
    let mut gap = 0;
    let flush = |run: &mut Vec<usize>, eligible: &mut Vec<bool>| {
        let long = run.len() >= MIN_RUN;
        for &index in run.iter() {
            if long || unique[index] {
                eligible[index] = true;
            }
        }
        run.clear();
    };
    for (index, entry) in entries.iter().enumerate() {
        if index == 0 || !contiguous(&entries[index - 1], entry) {
            flush(&mut run, &mut eligible);
            gap = 0;
        }
        if !entry.qualifies {
            continue;
        }
        if candidate[index] {
            gap = 0;
            run.push(index);
        } else {
            gap += 1;
            if gap > 1 {
                flush(&mut run, &mut eligible);
            }
        }
    }
    flush(&mut run, &mut eligible);
    eligible
}

fn contiguous(previous: &Entry, entry: &Entry) -> bool {
    previous.hunk == entry.hunk
        && previous.removed == entry.removed
        && previous.line + 1 == entry.line
}

/// Pair removed entries in order with the earliest unpaired added entry of the lowest other
/// hunk; never within one hunk.
fn pair(entries: &[Entry], removed: &[usize], added: &[usize], partner: &mut [Option<usize>]) {
    let mut buckets = BTreeMap::<usize, VecDeque<usize>>::new();
    for &index in added {
        buckets
            .entry(entries[index].hunk)
            .or_default()
            .push_back(index);
    }
    for &left in removed {
        let hunk = entries[left].hunk;
        let Some(target) = buckets.keys().copied().find(|&other| other != hunk) else {
            continue;
        };
        let Some(queue) = buckets.get_mut(&target) else {
            continue;
        };
        if let Some(right) = queue.pop_front() {
            partner[left] = Some(right);
            partner[right] = Some(left);
        }
        if queue.is_empty() {
            buckets.remove(&target);
        }
    }
}

/// Pairs between two hunks stay only when their indentation shifts by one amount.
fn consistent_shifts(entries: &[Entry], partner: &mut [Option<usize>]) {
    let mut shifts = FxHashMap::<(usize, usize), Option<isize>>::default();
    for (index, entry) in entries.iter().enumerate() {
        let Some(other) = partner[index].filter(|_| entry.removed) else {
            continue;
        };
        let other = &entries[other];
        let shift = other.indent - entry.indent;
        shifts
            .entry((entry.hunk, other.hunk))
            .and_modify(|known| {
                if *known != Some(shift) {
                    *known = None;
                }
            })
            .or_insert(Some(shift));
    }
    for index in 0..entries.len() {
        let Some(other) = partner[index].filter(|_| entries[index].removed) else {
            continue;
        };
        if shifts[&(entries[index].hunk, entries[other].hunk)].is_none() {
            partner[index] = None;
            partner[other] = None;
        }
    }
}

/// Line numbers of one declaration's non-blank lines, all changed on one side, as entries.
struct Declaration {
    change: usize,
    removed: bool,
    lines: Vec<usize>,
}

/// Keeps pairs only inside declarations moved as a whole: a declaration whose non-blank lines
/// are all removed in base pairs with a declaration whose non-blank lines are all added in head
/// when both have as many lines and every line at the same position equals its counterpart
/// after renames, with the same indentation, every rename applied is proven pure and the
/// declaration keeps its bindings in the target file. Lines too short to pair on their own
/// pair there as well. A declaration never moves between a test file and a non-test file.
fn declarations(
    entries: &[Entry],
    sides: &[Sides<'_>],
    checks: Checks<'_>,
    partner: &mut [Option<usize>],
) {
    let found = changed_declarations(entries, sides, partner);
    // Head declarations by member entry, with the member's position.
    let mut heads = FxHashMap::<usize, Vec<(usize, usize)>>::default();
    for (index, declaration) in found.iter().enumerate() {
        if !declaration.removed {
            for (position, &entry) in declaration.lines.iter().enumerate() {
                heads.entry(entry).or_default().push((index, position));
            }
        }
    }
    let mut accepted = Vec::new();
    for (index, source) in found.iter().enumerate() {
        if !source.removed {
            continue;
        }
        let Some((position, target)) = source
            .lines
            .iter()
            .enumerate()
            .find_map(|(position, &entry)| Some((position, partner[entry]?)))
        else {
            continue;
        };
        for &(candidate, offset) in heads.get(&target).into_iter().flatten() {
            let destination = &found[candidate];
            if offset != position
                || destination.lines.len() != source.lines.len()
                || sides[source.change].test[0] != sides[destination.change].test[1]
            {
                continue;
            }
            let mut pairs = source.lines.iter().zip(&destination.lines);
            if !pairs.all(|(&left, &right)| {
                entries[left].indent == entries[right].indent
                    && (partner[left] == Some(right)
                        || (partner[left].is_none()
                            && partner[right].is_none()
                            && entries[left].key == entries[right].key))
                    && pure_pair(&entries[left], &entries[right], checks.pure)
            }) {
                continue;
            }
            accepted.push((index, candidate));
        }
    }
    // Head line ranges of every accepted destination, per change.
    let mut folded = FxHashMap::<usize, Vec<(usize, usize)>>::default();
    let numbers = |declaration: &Declaration| -> Vec<usize> {
        declaration
            .lines
            .iter()
            .map(|&entry| entries[entry].number)
            .collect()
    };
    for &(_, candidate) in &accepted {
        let lines = numbers(&found[candidate]);
        if let (Some(&first), Some(&last)) = (lines.iter().min(), lines.iter().max()) {
            folded
                .entry(found[candidate].change)
                .or_default()
                .push((first, last));
        }
    }
    let mut kept = vec![false; entries.len()];
    let mut extra = Vec::new();
    for (source, candidate) in accepted {
        let (source, destination) = (&found[source], &found[candidate]);
        let ranges = folded
            .get(&destination.change)
            .map_or(&[][..], Vec::as_slice);
        if !(checks.keeps)(
            source.change,
            destination.change,
            &numbers(source),
            &numbers(destination),
            ranges,
        ) {
            continue;
        }
        for (&left, &right) in source.lines.iter().zip(&destination.lines) {
            kept[left] = true;
            kept[right] = true;
            if partner[left].is_none() {
                extra.push((left, right));
            }
        }
    }
    for index in 0..entries.len() {
        if !kept[index] {
            partner[index] = None;
        }
    }
    for (left, right) in extra {
        if partner[left].is_none() && partner[right].is_none() {
            partner[left] = Some(right);
            partner[right] = Some(left);
        }
    }
}

/// Declarations with at least one paired line whose non-blank lines are all changed lines.
fn changed_declarations(
    entries: &[Entry],
    sides: &[Sides<'_>],
    partner: &[Option<usize>],
) -> Vec<Declaration> {
    let mut at = FxHashMap::<(usize, bool, usize), usize>::default();
    let mut paired = BTreeMap::<(usize, bool), Vec<usize>>::new();
    for (index, entry) in entries.iter().enumerate() {
        at.insert((entry.change, entry.removed, entry.number), index);
        if partner[index].is_some() {
            paired
                .entry((entry.change, entry.removed))
                .or_default()
                .push(entry.number);
        }
    }
    let mut found: Vec<Declaration> = Vec::new();
    for ((change, removed), mut numbers) in paired {
        numbers.sort_unstable();
        let side = usize::from(!removed);
        let Some(text) = sides[change].text[side] else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        for &(start, end) in &sides[change].spans[side] {
            let first = numbers.partition_point(|&number| number < start);
            if numbers.get(first).is_none_or(|&number| number > end) {
                continue;
            }
            let mut members = Vec::new();
            let whole = (start..=end).all(|number| {
                if lines
                    .get(number.wrapping_sub(1))
                    .is_none_or(|line| line.trim().is_empty())
                {
                    return true;
                }
                at.get(&(change, removed, number))
                    .map(|&index| members.push(index))
                    .is_some()
            });
            if whole && !members.is_empty() {
                found.push(Declaration {
                    change,
                    removed,
                    lines: members,
                });
            }
        }
    }
    found
}

/// Blank lines move when every nearest qualifying neighbour on their block side does.
fn sandwich(entries: &[Entry], moved: &mut [bool]) {
    let nearest = |order: &mut dyn Iterator<Item = usize>| {
        let mut nearest = vec![None; entries.len()];
        let mut previous: Option<usize> = None;
        let mut last = None;
        for index in order {
            let current = &entries[index];
            let joined = previous.is_some_and(|previous: usize| {
                let (first, second) = if previous < index {
                    (&entries[previous], current)
                } else {
                    (current, &entries[previous])
                };
                contiguous(first, second)
            });
            if !joined {
                last = None;
            }
            nearest[index] = last;
            if current.qualifies {
                last = Some(index);
            }
            previous = Some(index);
        }
        nearest
    };
    let before = nearest(&mut (0..entries.len()));
    let after = nearest(&mut (0..entries.len()).rev());
    let snapshot = moved.to_vec();
    for (index, entry) in entries.iter().enumerate() {
        if !entry.blank || moved[index] {
            continue;
        }
        let neighbours: Vec<bool> = [before[index], after[index]]
            .into_iter()
            .flatten()
            .map(|neighbour| snapshot[neighbour])
            .collect();
        if !neighbours.is_empty() && neighbours.iter().all(|moved| *moved) {
            moved[index] = true;
        }
    }
}

fn single(text: &str) -> bool {
    !text.is_empty() && !text.contains(' ')
}

/// Rename rewrites applied to identifier tokens of removed lines only, so a line matches its
/// counterpart when it applies the renames in their direction, never when it reverses them.
struct Spellings {
    /// Old spelling, new spelling, whether it also applies inside longer tokens and its
    /// rewrite index; longest old spelling first.
    list: Vec<(String, String, bool, usize)>,
}

impl Spellings {
    fn new(rules: &[ReviewRewrite]) -> Self {
        let mut list: Vec<(String, String, bool, usize)> = Vec::new();
        for (index, rule) in rules.iter().enumerate() {
            if rule.rename
                && single(&rule.old)
                && single(&rule.new)
                && !list.iter().any(|(old, ..)| *old == rule.old)
            {
                list.push((rule.old.clone(), rule.new.clone(), rule.inside, index));
            }
        }
        list.sort_by(|left, right| {
            right
                .0
                .len()
                .cmp(&left.0.len())
                .then_with(|| left.0.cmp(&right.0))
        });
        Self { list }
    }

    /// A removed identifier token with its old spellings replaced by the new ones: whole tokens
    /// always, and parts between word boundaries for `inside` rewrites. Replaced text is not
    /// rewritten again; other tokens stay as they are. Also returns the rewrites applied.
    fn removed(&self, token: &str) -> (String, Vec<usize>) {
        if !identifier(token) {
            return (token.to_string(), Vec::new());
        }
        let placeholder = |index: usize| {
            char::from_u32(0xE000 + u32::try_from(index).unwrap_or(0)).unwrap_or('\u{E000}')
        };
        let mut current = token.to_string();
        let mut applied = Vec::new();
        for (index, (spelling, _, inside, rule)) in self.list.iter().enumerate() {
            if current == *spelling {
                current = placeholder(index).to_string();
                applied.push(*rule);
                break;
            }
            if !*inside || !current.contains(spelling.as_str()) {
                continue;
            }
            let mut output = String::with_capacity(current.len());
            let mut last = 0;
            for (start, _) in current.match_indices(spelling.as_str()) {
                let end = start + spelling.len();
                if start >= last && boundary(&current, start) && boundary(&current, end) {
                    output.push_str(&current[last..start]);
                    output.push(placeholder(index));
                    last = end;
                    if !applied.contains(rule) {
                        applied.push(*rule);
                    }
                }
            }
            output.push_str(&current[last..]);
            current = output;
        }
        if applied.is_empty() {
            return (current, applied);
        }
        let mut output = String::with_capacity(current.len());
        for ch in current.chars() {
            let index = (ch as usize).wrapping_sub(0xE000);
            match self.list.get(index) {
                Some((_, new, ..)) => output.push_str(new),
                None => output.push(ch),
            }
        }
        applied.sort_unstable();
        (output, applied)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(old: &str, new: &str, inside: bool, rename: bool) -> ReviewRewrite {
        ReviewRewrite {
            old: old.into(),
            new: new.into(),
            inside,
            hunks: 3,
            rename,
        }
    }

    #[test]
    fn spellings_rewrite_removed_tokens_in_the_rewrite_direction() {
        let spellings = Spellings::new(&[
            rule("Job", "WorkOrder", true, true),
            rule("job", "workOrder", true, true),
            rule("roundToIncrement", "quantize", false, true),
            rule("floorToIncrement", "quantize", false, true),
            rule("true", "false", false, false),
            rule("held", "paused", true, false),
        ]);
        let removed = |token: &str| spellings.removed(token).0;
        assert_eq!(removed("assignJob"), "assignWorkOrder");
        assert_eq!(removed("jobId"), "workOrderId");
        assert_eq!(removed("Jobs"), "Jobs");
        assert_eq!(removed("floorToIncrement"), "quantize");
        assert_eq!(removed("roundToIncrement"), "quantize");
        assert_eq!(removed("quantizer"), "quantizer");
        // The reverse direction is never folded.
        assert_eq!(removed("quantize"), "quantize");
        // Only renames apply, and only to identifier tokens.
        assert_eq!(removed("true"), "true");
        assert_eq!(removed("'held'"), "'held'");
        assert_eq!(removed("'job'"), "'job'");
    }

    fn parse(diffs: &[&'static str]) -> Vec<Hunk<'static>> {
        diffs
            .iter()
            .enumerate()
            .flat_map(|(change, diff)| super::super::hunks::parse(change, diff))
            .collect()
    }

    fn moved(hunks: &[Hunk<'_>]) -> Matches {
        moved_in(hunks, &[])
    }

    fn moved_in(hunks: &[Hunk<'_>], sides: &[Sides<'static>]) -> Matches {
        let none = vec![false; hunks.len()];
        let deadline = Instant::now() + std::time::Duration::from_secs(60);
        let mut sides = sides.to_vec();
        let changes = hunks.iter().map(|hunk| hunk.change + 1).max().unwrap_or(0);
        sides.resize_with(changes.max(sides.len()), Sides::default);
        let checks = Checks {
            pure: &|_| true,
            keeps: &|_, _, _, _, _| true,
        };
        matches(hunks, &none, &none, &sides, &[], checks, deadline).unwrap()
    }

    /// One change's base and head text, each declaring one span.
    fn side(
        base: Option<(&'static str, (usize, usize))>,
        head: Option<(&'static str, (usize, usize))>,
    ) -> Sides<'static> {
        Sides {
            text: [base.map(|base| base.0), head.map(|head| head.0)],
            spans: [
                base.map(|base| vec![base.1]).unwrap_or_default(),
                head.map(|head| vec![head.1]).unwrap_or_default(),
            ],
            test: [false; 2],
        }
    }

    const TOTAL_BASE: &str = "export function total(items) {\n  const sum = items.reduce(add, 0);\n  const tax = sum * rate;\n  return sum + tax;\n}\n\nexport const unit = 'cent';\n";
    const TOTAL_UNCHANGED: &str = "export function total(items) {\n  const sum = items.reduce(add, 0);\n  const tax = sum * rate;\n  return sum + tax;\n}\n";
    const TOTAL_EDITED: &str = "export function total(items) {\n  const sum = items.reduce(add, 0);\n  const tax = sum * rateFor(items);\n  return sum + tax;\n}\n";
    const TOTAL_REMOVED: &str = "diff --git a/a.ts b/a.ts\n@@ -1,7 +0,0 @@\n-export function total(items) {\n-  const sum = items.reduce(add, 0);\n-  const tax = sum * rate;\n-  return sum + tax;\n-}\n-\n-export const unit = 'cent';\n";

    fn total_hunks() -> Vec<Hunk<'static>> {
        parse(&[
            TOTAL_REMOVED,
            "diff --git a/b.ts b/b.ts\n@@ -0,0 +1,5 @@\n+export function total(items) {\n+  const sum = items.reduce(add, 0);\n+  const tax = sum * rate;\n+  return sum + tax;\n+}\n",
        ])
    }

    #[test]
    fn only_unchanged_declarations_move_and_never_across_the_test_boundary() {
        let hunks = total_hunks();
        let sides = |base_test: bool, head_test: bool| {
            let mut base = side(Some((TOTAL_BASE, (1, 5))), None);
            base.test[0] = base_test;
            let mut head = side(None, Some((TOTAL_UNCHANGED, (1, 5))));
            head.test[1] = head_test;
            [base, head]
        };
        let matches = moved_in(&hunks, &sides(false, false));
        let fates = |hunk: usize| -> Vec<bool> {
            matches.fates[hunk]
                .iter()
                .map(|fate| *fate == Fate::Moved)
                .collect()
        };
        // The closing braces pair by position inside the matched declarations.
        assert_eq!(fates(0), [true, true, true, true, true, false, false]);
        assert_eq!(fates(1), [true; 5]);
        assert_eq!(
            matches.moves,
            [ReviewBriefMove {
                from: 0,
                to: 1,
                from_lines: [1, 5],
                to_lines: [1, 5],
            }]
        );
        assert_eq!(moved_in(&hunks, &sides(true, true)).moves.len(), 1);
        // Without declarations, or between a test and a non-test file, nothing moves.
        assert_eq!(moved(&hunks).moves, Vec::<ReviewBriefMove>::new());
        assert_eq!(
            moved_in(&hunks, &sides(false, true)).moves,
            Vec::<ReviewBriefMove>::new()
        );
        assert_eq!(
            moved_in(&hunks, &sides(true, false)).moves,
            Vec::<ReviewBriefMove>::new()
        );
        // One edited line keeps the whole declaration shown.
        let edited = parse(&[
            TOTAL_REMOVED,
            "diff --git a/b.ts b/b.ts\n@@ -0,0 +1,5 @@\n+export function total(items) {\n+  const sum = items.reduce(add, 0);\n+  const tax = sum * rateFor(items);\n+  return sum + tax;\n+}\n",
        ]);
        let head = side(None, Some((TOTAL_EDITED, (1, 5))));
        let sides = [side(Some((TOTAL_BASE, (1, 5))), None), head];
        let matches = moved_in(&edited, &sides);
        assert_eq!(matches.moves, Vec::<ReviewBriefMove>::new());
        assert!(
            matches
                .fates
                .iter()
                .flatten()
                .all(|fate| *fate == Fate::Changed)
        );
    }

    #[test]
    fn lines_never_move_within_their_hunk_and_short_lines_need_a_run() {
        // A swap inside one hunk and a short line moving to another hunk both stay shown.
        let hunks = parse(&[
            "diff --git a/a.ts b/a.ts\n@@ -1,3 +1,2 @@\n-  debit(from, amount);\n   credit(to, amount);\n+  debit(from, amount);\n-  requireAdmin(user);\n@@ -20 +19,2 @@\n   list(users);\n+  requireAdmin(user);\n",
        ]);
        let matches = moved(&hunks);
        assert!(
            matches
                .fates
                .iter()
                .flatten()
                .all(|fate| *fate == Fate::Changed)
        );
        assert_eq!(matches.moves, Vec::<ReviewBriefMove>::new());
    }

    #[test]
    fn declarations_with_other_lines_or_line_counts_stay_shown() {
        // A removed keyword changes the declaration's line count.
        let hunks = parse(&[
            "diff --git a/a.py b/a.py\n@@ -1,5 +0,0 @@\n-def handle(item):\n-    if item.poison:\n-        log_poison(item)\n-        return\n-    process(item)\n@@ -20,0 +16,4 @@\n+def handle(item):\n+    if item.poison:\n+        log_poison(item)\n+    process(item)\n",
        ]);
        let base = "def handle(item):\n    if item.poison:\n        log_poison(item)\n        return\n    process(item)\n";
        let head = "\n\n\n\n\n\n\n\n\n\n\n\n\n\n\ndef handle(item):\n    if item.poison:\n        log_poison(item)\n    process(item)\n";
        let matches = moved_in(
            &hunks,
            &[side(Some((base, (1, 5))), Some((head, (16, 19))))],
        );
        assert!(
            matches
                .fates
                .iter()
                .flatten()
                .all(|fate| *fate == Fate::Changed)
        );
        // A closing brace moved inside a declaration of the same size.
        let hunks = parse(&[
            "diff --git a/a.ts b/a.ts\n@@ -1,6 +0,0 @@\n-function handle(req) {\n-  if (!allowed(req)) {\n-    return deny(req, 403);\n-  }\n-  serve(req, res, next);\n-}\n",
            "diff --git a/b.ts b/b.ts\n@@ -0,0 +1,6 @@\n+function handle(req) {\n+  if (!allowed(req)) {\n+    return deny(req, 403);\n+  serve(req, res, next);\n+  }\n+}\n",
        ]);
        let base = "function handle(req) {\n  if (!allowed(req)) {\n    return deny(req, 403);\n  }\n  serve(req, res, next);\n}\n";
        let head = "function handle(req) {\n  if (!allowed(req)) {\n    return deny(req, 403);\n  serve(req, res, next);\n  }\n}\n";
        let sides = [
            side(Some((base, (1, 6))), None),
            side(None, Some((head, (1, 6)))),
        ];
        assert!(
            moved_in(&hunks, &sides)
                .fates
                .iter()
                .flatten()
                .all(|fate| *fate == Fate::Changed)
        );
    }

    #[test]
    fn reindented_declarations_never_move() {
        let base = "  alpha(one, two);\n  beta(one, two);\n  gamma(one, two);\n";
        let sides = |head: &'static str| {
            [
                side(Some((base, (1, 3))), None),
                side(None, Some((head, (1, 3)))),
            ]
        };
        let hunks = parse(&[
            "diff --git a/a.ts b/a.ts\n@@ -1,3 +0,0 @@\n-  alpha(one, two);\n-  beta(one, two);\n-  gamma(one, two);\n",
            "diff --git a/b.ts b/b.ts\n@@ -0,0 +1,3 @@\n+alpha(one, two);\n+    beta(one, two);\n+gamma(one, two);\n",
        ]);
        let head = "alpha(one, two);\n    beta(one, two);\ngamma(one, two);\n";
        assert_eq!(
            moved_in(&hunks, &sides(head)).moves,
            Vec::<ReviewBriefMove>::new()
        );
        let hunks = parse(&[
            "diff --git a/a.ts b/a.ts\n@@ -1,3 +0,0 @@\n-  alpha(one, two);\n-  beta(one, two);\n-  gamma(one, two);\n",
            "diff --git a/b.ts b/b.ts\n@@ -0,0 +1,3 @@\n+alpha(one, two);\n+beta(one, two);\n+gamma(one, two);\n",
        ]);
        let head = "alpha(one, two);\nbeta(one, two);\ngamma(one, two);\n";
        assert_eq!(
            moved_in(&hunks, &sides(head)).moves,
            Vec::<ReviewBriefMove>::new()
        );
        let hunks = parse(&[
            "diff --git a/a.ts b/a.ts\n@@ -1,3 +0,0 @@\n-  alpha(one, two);\n-  beta(one, two);\n-  gamma(one, two);\n",
            "diff --git a/b.ts b/b.ts\n@@ -0,0 +1,3 @@\n+  alpha(one, two);\n+  beta(one, two);\n+  gamma(one, two);\n",
        ]);
        assert_eq!(moved_in(&hunks, &sides(base)).moves.len(), 1);
    }
}
