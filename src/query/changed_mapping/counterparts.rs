use super::{HunkChanges, MAX_MAPPING_WORK, spend};
use crate::model::DefinitionFact;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct CounterpartMapping {
    pub definitions: Vec<usize>,
    pub ambiguous: usize,
    pub unprocessed: usize,
}

/// Match existing declarations only through an unchanged header-start line in the captured diff.
/// Names alone cannot establish a counterpart for an added or removed declaration.
pub(crate) fn map_counterparts(
    changed: &[DefinitionFact],
    opposite: &[DefinitionFact],
    hunks: &HunkChanges,
    old_side: bool,
) -> CounterpartMapping {
    map_with_limit(changed, opposite, hunks, old_side, MAX_MAPPING_WORK)
}

fn map_with_limit(
    changed: &[DefinitionFact],
    opposite: &[DefinitionFact],
    hunks: &HunkChanges,
    old_side: bool,
    mut work: usize,
) -> CounterpartMapping {
    let mut result = CounterpartMapping {
        definitions: Vec::new(),
        ambiguous: 0,
        unprocessed: 0,
    };
    if hunks.omitted_hunks > 0 || spend(&mut work, opposite.len()).is_none() {
        result.unprocessed = changed.len();
        return result;
    }
    let mut by_header = BTreeMap::<_, Vec<usize>>::new();
    for (index, definition) in opposite.iter().enumerate() {
        by_header
            .entry((
                definition.symbol.line,
                &definition.symbol.name,
                &definition.symbol.kind,
            ))
            .or_default()
            .push(index);
    }
    let mut selected = BTreeSet::new();
    for (position, definition) in changed.iter().enumerate() {
        if spend(&mut work, hunks.hunks.len().saturating_add(1)).is_none() {
            result.unprocessed = changed.len() - position;
            break;
        }
        let Some(line) = unchanged_line(definition.symbol.line, hunks, old_side) else {
            continue;
        };
        let Some(matches) =
            by_header.get(&(line, &definition.symbol.name, &definition.symbol.kind))
        else {
            continue;
        };
        if let [index] = matches.as_slice() {
            selected.insert(*index);
        } else {
            result.ambiguous += 1;
        }
    }
    result.definitions = selected.into_iter().collect();
    result
}

fn unchanged_line(line: usize, hunks: &HunkChanges, old_side: bool) -> Option<usize> {
    let line = line.checked_sub(1)?;
    let (mut source_end, mut target_end) = (0, 0);
    for hunk in &hunks.hunks {
        let (start, count, other_start, other_count) = if old_side {
            (
                hunk.old_start,
                hunk.old_lines,
                hunk.new_start,
                hunk.new_lines,
            )
        } else {
            (
                hunk.new_start,
                hunk.new_lines,
                hunk.old_start,
                hunk.old_lines,
            )
        };
        // Zero-length starts are gaps after N lines; nonempty starts are one-based lines.
        let start = start.checked_sub(usize::from(count > 0))?;
        let other_start = other_start.checked_sub(usize::from(other_count > 0))?;
        if line < start {
            break;
        }
        source_end = start.checked_add(count)?;
        if line < source_end {
            return None;
        }
        target_end = other_start.checked_add(other_count)?;
    }
    line.checked_sub(source_end)?
        .checked_add(target_end)?
        .checked_add(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::changed_mapping::ChangeHunk;

    #[test]
    fn unchanged_header_translation_handles_zero_anchors_and_offsets() {
        let hunks = HunkChanges {
            hunks: vec![
                ChangeHunk {
                    old_start: 0,
                    old_lines: 0,
                    new_start: 1,
                    new_lines: 2,
                },
                ChangeHunk {
                    old_start: 3,
                    old_lines: 2,
                    new_start: 4,
                    new_lines: 0,
                },
            ],
            omitted_hunks: 0,
        };
        assert_eq!(unchanged_line(1, &hunks, true), Some(3));
        assert_eq!(unchanged_line(2, &hunks, true), Some(4));
        assert_eq!(unchanged_line(3, &hunks, true), None);
        assert_eq!(unchanged_line(4, &hunks, true), None);
        assert_eq!(unchanged_line(5, &hunks, true), Some(5));
        assert_eq!(unchanged_line(1, &hunks, false), None);
        assert_eq!(unchanged_line(2, &hunks, false), None);
        assert_eq!(unchanged_line(3, &hunks, false), Some(1));
        assert_eq!(unchanged_line(4, &hunks, false), Some(2));
        assert_eq!(unchanged_line(5, &hunks, false), Some(5));
    }

    #[test]
    fn incomplete_diff_or_work_limit_never_guesses_a_counterpart() {
        let definitions = [DefinitionFact::default()];
        for (omitted_hunks, work) in [(1, MAX_MAPPING_WORK), (0, 0)] {
            let mapped = map_with_limit(
                &definitions,
                &definitions,
                &HunkChanges {
                    hunks: Vec::new(),
                    omitted_hunks,
                },
                true,
                work,
            );
            assert_eq!(mapped.definitions, Vec::<usize>::new());
            assert_eq!(mapped.unprocessed, 1);
        }
    }
}
