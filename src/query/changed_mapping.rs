use crate::model::{DefinitionFact, DefinitionFacts, DefinitionStatus, LineRange, SourceSpan};
use anyhow::Result;
use std::collections::BTreeMap;

const MAX_HUNKS: usize = 4_096;
const MAX_MAPPING_WORK: usize = 1_000_000;

/// Old-side and new-side zero-context line ranges, including zero-length insertion or deletion anchors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChangeHunk {
    pub old_start: usize,
    pub old_lines: usize,
    pub new_start: usize,
    pub new_lines: usize,
}

/// Buffer-derived change hunks and explicit accounting for hunks omitted by the work limit.
pub(crate) struct HunkChanges {
    pub hunks: Vec<ChangeHunk>,
    pub omitted_hunks: usize,
}

pub(crate) struct ChangedDefinition {
    pub definition_index: usize,
    pub ranges: Vec<LineRange>,
    pub wrapper_ranges: Vec<LineRange>,
    pub ambiguous: bool,
}

pub(crate) struct MappedChanges {
    pub status: DefinitionStatus,
    pub definitions: Vec<ChangedDefinition>,
    pub uncovered: Vec<LineRange>,
    pub unprocessed: Vec<LineRange>,
}

/// Compute zero-context hunks from the supplied buffers, retaining zero-length anchors and reporting hunks omitted beyond the 4,096-hunk limit.
pub(crate) fn changed_hunks(old: &str, new: &str) -> Result<HunkChanges> {
    let mut options = git2::DiffOptions::new();
    options.context_lines(0);
    let patch = git2::Patch::from_buffers(
        old.as_bytes(),
        None,
        new.as_bytes(),
        None,
        Some(&mut options),
    )?;
    let count = patch.num_hunks();
    let hunks = (0..count.min(MAX_HUNKS))
        .map(|index| {
            let (hunk, _) = patch.hunk(index)?;
            Ok(ChangeHunk {
                old_start: hunk.old_start() as usize,
                old_lines: hunk.old_lines() as usize,
                new_start: hunk.new_start() as usize,
                new_lines: hunk.new_lines() as usize,
            })
        })
        .collect::<Result<Vec<_>, git2::Error>>()?;
    Ok(HunkChanges {
        hunks,
        omitted_hunks: count.saturating_sub(MAX_HUNKS),
    })
}

/// Map changed line segments to innermost declarations, separating wrapper-only, uncovered, ambiguous and work-limit-unprocessed segments without I/O.
pub(crate) fn map_changed_definitions(
    facts: &DefinitionFacts,
    ranges: &[LineRange],
) -> MappedChanges {
    map_with_limit(facts, ranges, MAX_MAPPING_WORK)
}

fn map_with_limit(facts: &DefinitionFacts, ranges: &[LineRange], mut work: usize) -> MappedChanges {
    let ranges = normalized_ranges(ranges);
    let mut mapped = BTreeMap::<usize, ChangedDefinition>::new();
    let mut result = MappedChanges {
        status: facts.status,
        definitions: Vec::new(),
        uncovered: Vec::new(),
        unprocessed: Vec::new(),
    };
    for (position, range) in ranges.iter().enumerate() {
        let Some((matches, uncovered)) = map_range(&facts.definitions, range, &mut work) else {
            result.unprocessed.extend_from_slice(&ranges[position..]);
            break;
        };
        result.uncovered.extend(uncovered);
        for change in matches {
            let entry =
                mapped
                    .entry(change.definition_index)
                    .or_insert_with(|| ChangedDefinition {
                        definition_index: change.definition_index,
                        ranges: Vec::new(),
                        wrapper_ranges: Vec::new(),
                        ambiguous: false,
                    });
            entry.ranges.extend(change.ranges);
            entry.wrapper_ranges.extend(change.wrapper_ranges);
            entry.ambiguous |= change.ambiguous;
        }
    }
    result.definitions = mapped
        .into_values()
        .map(|mut change| {
            change.ranges = normalized_ranges(&change.ranges);
            change.wrapper_ranges = normalized_ranges(&change.wrapper_ranges);
            change
        })
        .collect();
    result.uncovered = normalized_ranges(&result.uncovered);
    result
}

fn map_range(
    definitions: &[DefinitionFact],
    range: &LineRange,
    work: &mut usize,
) -> Option<(Vec<ChangedDefinition>, Vec<LineRange>)> {
    let mut relevant = Vec::new();
    let mut boundaries = vec![range.start, range.end.checked_add(1)?];
    for (index, definition) in definitions.iter().enumerate() {
        spend(work, 1)?;
        let spans = [Some(definition.declaration_span), definition.source_span];
        let mut intersects = false;
        for span in spans.into_iter().flatten() {
            if span.start_line <= range.end && span.end_line >= range.start {
                intersects = true;
                boundaries.push(span.start_line.max(range.start));
                boundaries.push(span.end_line.min(range.end).checked_add(1)?);
            }
        }
        if intersects {
            relevant.push((index, definition));
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    let mut mapped = BTreeMap::<usize, ChangedDefinition>::new();
    let mut uncovered = Vec::new();
    for pair in boundaries.windows(2) {
        spend(work, relevant.len().saturating_mul(2).saturating_add(1))?;
        let segment = LineRange {
            start: pair[0],
            end: pair[1] - 1,
        };
        let candidates = relevant
            .iter()
            .filter_map(|(index, definition)| {
                if contains_line(definition.declaration_span, segment.start) {
                    Some((*index, definition.declaration_span, false))
                } else {
                    definition
                        .source_span
                        .filter(|span| contains_line(*span, segment.start))
                        .map(|span| (*index, span, true))
                }
            })
            .collect::<Vec<_>>();
        let selected = innermost(candidates);
        if selected.is_empty() {
            uncovered.push(segment);
            continue;
        }
        let ambiguous = selected.len() > 1;
        for (index, wrapper) in selected {
            let entry = mapped.entry(index).or_insert_with(|| ChangedDefinition {
                definition_index: index,
                ranges: Vec::new(),
                wrapper_ranges: Vec::new(),
                ambiguous: false,
            });
            if wrapper {
                entry.wrapper_ranges.push(segment.clone());
            } else {
                entry.ranges.push(segment.clone());
            }
            entry.ambiguous |= ambiguous;
        }
    }
    Some((mapped.into_values().collect(), uncovered))
}

fn innermost(mut candidates: Vec<(usize, SourceSpan, bool)>) -> Vec<(usize, bool)> {
    candidates.sort_by_key(|(_, span, _)| (std::cmp::Reverse(span.start_byte), span.end_byte));
    let mut retained = Vec::new();
    let mut min_end: Option<usize> = None;
    for group in candidates.chunk_by(|(_, left, _), (_, right, _)| {
        left.start_byte == right.start_byte && left.end_byte == right.end_byte
    }) {
        let end = group[0].1.end_byte;
        if min_end.is_none_or(|previous| previous > end) {
            retained.extend(group.iter().map(|(index, _, wrapper)| (*index, *wrapper)));
        }
        min_end = Some(min_end.map_or(end, |previous| previous.min(end)));
    }
    retained.sort_unstable();
    retained
}

fn contains_line(span: SourceSpan, line: usize) -> bool {
    span.start_line <= line && span.end_line >= line
}

fn spend(work: &mut usize, amount: usize) -> Option<()> {
    *work = work.checked_sub(amount)?;
    Some(())
}

fn normalized_ranges(ranges: &[LineRange]) -> Vec<LineRange> {
    let mut ranges = ranges.to_vec();
    ranges.sort_by_key(|range| (range.start, range.end));
    let mut merged: Vec<LineRange> = Vec::new();
    for range in ranges {
        if let Some(previous) = merged.last_mut()
            && range.start <= previous.end.saturating_add(1)
        {
            previous.end = previous.end.max(range.end);
        } else {
            merged.push(range);
        }
    }
    merged
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "bounded fixtures must fail immediately on invalid setup"
)]
mod tests {
    use super::*;
    use crate::lang::FirstClass;
    use std::fmt::Write as _;

    fn facts(language: FirstClass, source: &str) -> DefinitionFacts {
        let tree = crate::parse::parse(language, source).unwrap();
        crate::metrics::symbols::analyze(language, source, &tree).definitions
    }

    fn pairs(ranges: &[LineRange]) -> Vec<(usize, usize)> {
        ranges
            .iter()
            .map(|range| (range.start, range.end))
            .collect()
    }

    #[test]
    fn captured_buffer_hunks_preserve_empty_insertion_and_deletion_sides() {
        let added = changed_hunks("", "fn added() {}\n").unwrap();
        assert_eq!(added.omitted_hunks, 0);
        assert_eq!(
            added.hunks,
            [ChangeHunk {
                old_start: 0,
                old_lines: 0,
                new_start: 1,
                new_lines: 1,
            }]
        );
        let deleted = changed_hunks("fn removed() {}\n", "").unwrap();
        assert_eq!(
            deleted.hunks,
            [ChangeHunk {
                old_start: 1,
                old_lines: 1,
                new_start: 0,
                new_lines: 0,
            }]
        );
        let middle = changed_hunks("one\nthree\n", "one\ntwo\nthree\n").unwrap();
        assert_eq!(
            middle.hunks,
            [ChangeHunk {
                old_start: 1,
                old_lines: 0,
                new_start: 2,
                new_lines: 1,
            }]
        );
    }

    #[test]
    fn captured_buffer_hunks_report_final_newline_changes_and_no_changes() {
        let newline = changed_hunks("fn unchanged() {}", "fn unchanged() {}\n").unwrap();
        assert_eq!(newline.hunks.len(), 1);
        assert_eq!(
            (newline.hunks[0].old_lines, newline.hunks[0].new_lines),
            (1, 1)
        );
        assert!(changed_hunks("same\n", "same\n").unwrap().hunks.is_empty());
    }

    #[test]
    fn hunk_limit_retains_exact_omitted_count() {
        let mut old = String::new();
        for index in 0..=MAX_HUNKS {
            writeln!(old, "old_{index}\nkeep_{index}").unwrap();
        }
        let new = old.replace("old_", "new_");
        let changes = changed_hunks(&old, &new).unwrap();
        assert_eq!(changes.hunks.len(), MAX_HUNKS);
        assert_eq!(changes.omitted_hunks, 1);
    }

    #[test]
    fn nesting_splits_touched_ranges_without_marking_ancestors_for_inner_only_edits() {
        let facts = facts(
            FirstClass::Rust,
            "fn outer() {\n    let first = 1;\n    fn inner() {}\n    let last = 2;\n}\n",
        );
        let inner_only = map_changed_definitions(&facts, &[LineRange { start: 3, end: 3 }]);
        assert_eq!(inner_only.definitions.len(), 1);
        assert_eq!(
            facts.definitions[inner_only.definitions[0].definition_index]
                .symbol
                .name,
            "inner"
        );
        let all = map_changed_definitions(&facts, &[LineRange { start: 1, end: 5 }]);
        assert_eq!(all.definitions.len(), 2);
        let outer = all
            .definitions
            .iter()
            .find(|change| facts.definitions[change.definition_index].symbol.name == "outer")
            .unwrap();
        assert_eq!(pairs(&outer.ranges), [(1, 2), (4, 5)]);
        assert!(!outer.ambiguous);
        assert!(all.uncovered.is_empty());
    }

    #[test]
    fn same_line_siblings_keep_ambiguous_touched_evidence() {
        let facts = facts(FirstClass::Rust, "fn first() {} fn second() {}\n");
        let changes = map_changed_definitions(&facts, &[LineRange { start: 1, end: 1 }]);
        assert_eq!(changes.definitions.len(), 2);
        for definition in changes.definitions {
            assert!(definition.ambiguous);
            assert_eq!(pairs(&definition.ranges), [(1, 1)]);
        }
    }

    #[test]
    fn shared_type_wrapper_has_explicit_wrapper_evidence() {
        let facts = facts(
            FirstClass::Go,
            "package sample\ntype (\nOne int\nTwo string\n)\n",
        );
        let changes = map_changed_definitions(&facts, &[LineRange { start: 2, end: 2 }]);
        assert_eq!(changes.definitions.len(), 2);
        for definition in changes.definitions {
            assert!(definition.ambiguous);
            assert!(definition.ranges.is_empty());
            assert_eq!(pairs(&definition.wrapper_ranges), [(2, 2)]);
        }
        assert!(changes.uncovered.is_empty());
    }

    #[test]
    fn grouped_go_type_edit_does_not_mark_the_sibling_wrapper() {
        let facts = facts(
            FirstClass::Go,
            "package sample\ntype (\nOne int\nTwo string\n)\n",
        );
        let changes = map_changed_definitions(&facts, &[LineRange { start: 3, end: 3 }]);
        assert_eq!(changes.definitions.len(), 1);
        let selected = &changes.definitions[0];
        assert_eq!(
            facts.definitions[selected.definition_index].symbol.name,
            "One"
        );
        assert_eq!(pairs(&selected.ranges), [(3, 3)]);
        assert!(selected.wrapper_ranges.is_empty());
        assert!(!selected.ambiguous);
    }

    #[test]
    fn grouped_javascript_body_edit_does_not_mark_the_sibling_wrapper() {
        let facts = facts(
            FirstClass::JavaScript,
            "export const one = () => {\n  return 1;\n},\ntwo = () => {\n  return 2;\n};\n",
        );
        let changes = map_changed_definitions(&facts, &[LineRange { start: 2, end: 2 }]);
        assert_eq!(changes.definitions.len(), 1);
        let selected = &changes.definitions[0];
        assert_eq!(
            facts.definitions[selected.definition_index].symbol.name,
            "one"
        );
        assert_eq!(pairs(&selected.ranges), [(2, 2)]);
        assert!(selected.wrapper_ranges.is_empty());
        assert!(!selected.ambiguous);
    }

    #[test]
    fn nested_group_header_maps_shared_wrappers_instead_of_outer_function() {
        let facts = facts(
            FirstClass::Go,
            "package sample\nfunc outer() {\ntype (\nOne int\nTwo string\n)\n}\n",
        );
        let changes = map_changed_definitions(&facts, &[LineRange { start: 3, end: 3 }]);
        assert_eq!(changes.definitions.len(), 2);
        for selected in changes.definitions {
            assert_ne!(
                facts.definitions[selected.definition_index].symbol.name,
                "outer"
            );
            assert!(selected.ranges.is_empty());
            assert_eq!(pairs(&selected.wrapper_ranges), [(3, 3)]);
            assert!(selected.ambiguous);
        }
        assert!(changes.uncovered.is_empty());
    }

    #[test]
    fn unmapped_top_level_and_parse_status_remain_explicit() {
        let facts = facts(FirstClass::Rust, "use std::fmt;\n\nfn value() {}\n");
        let changes = map_changed_definitions(&facts, &[LineRange { start: 1, end: 3 }]);
        assert_eq!(changes.status, DefinitionStatus::Available);
        assert_eq!(pairs(&changes.uncovered), [(1, 2)]);
        assert_eq!(pairs(&changes.definitions[0].ranges), [(3, 3)]);
        let unavailable = DefinitionFacts {
            status: DefinitionStatus::Unavailable,
            definitions: Vec::new(),
        };
        let changes = map_changed_definitions(&unavailable, &[LineRange { start: 1, end: 3 }]);
        assert_eq!(changes.status, DefinitionStatus::Unavailable);
        assert_eq!(pairs(&changes.uncovered), [(1, 3)]);
    }

    #[test]
    fn work_limit_does_not_label_unprocessed_lines_uncovered() {
        let facts = facts(FirstClass::Rust, "fn first() {}\nfn second() {}\n");
        let changes = map_with_limit(&facts, &[LineRange { start: 1, end: 2 }], 0);
        assert!(changes.definitions.is_empty());
        assert!(changes.uncovered.is_empty());
        assert_eq!(pairs(&changes.unprocessed), [(1, 2)]);
    }
}
