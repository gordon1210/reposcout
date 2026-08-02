use super::{
    BTreeMap, CloneGroup, CloneInstance, DuplicateBlock, DuplicateFindingSummary, Duplication,
    HealthPolicy, LineRange, PathBuf, Range, lang, testcov,
};

/// Rank the highest-impact duplicate blocks across both detectors. Impact is
/// the number of lines removable by de-duplicating a block,
/// `lines * (copies - 1)`; ties break toward larger blocks and more copies. Locations are capped
/// so the list stays compact even in `--summary` output.
#[cfg(test)]
pub(super) fn top_duplicate_blocks(
    dup: &Duplication,
    top: usize,
    min_new_lines: usize,
) -> Vec<DuplicateBlock> {
    top_duplicate_blocks_where(
        &ranked_duplicate_candidates(dup),
        top,
        min_new_lines,
        |_| true,
    )
}

#[cfg(test)]
pub(super) fn top_production_duplicate_blocks(
    dup: &Duplication,
    top: usize,
    min_new_lines: usize,
    test_regions: &BTreeMap<PathBuf, Vec<LineRange>>,
    health_policy: &HealthPolicy,
) -> Vec<DuplicateBlock> {
    top_duplicate_blocks_where(
        &ranked_duplicate_candidates(dup),
        top,
        min_new_lines,
        |group| {
            group.instances.iter().any(|instance| {
                instance_has_production_lines(instance, test_regions, health_policy, min_new_lines)
            })
        },
    )
}

pub(super) fn ranked_duplicate_candidates(dup: &Duplication) -> Vec<DuplicateCandidate<'_>> {
    let mut candidates = dup
        .exact
        .iter()
        .chain(dup.near.iter())
        .filter(|group| group.instances.len() >= 2)
        .map(|group| {
            let copies = group.instances.len();
            let mut key = group
                .instances
                .iter()
                .map(|instance| {
                    format!(
                        "{}:{}-{}",
                        instance.path.display(),
                        instance.start_line,
                        occupied_end_line(instance)
                    )
                })
                .collect::<Vec<_>>();
            key.sort();
            DuplicateCandidate {
                group,
                duplicated_lines: group.lines.saturating_mul(copies.saturating_sub(1)),
                key,
            }
        })
        .collect::<Vec<_>>();

    candidates.sort_by(|a, b| {
        b.duplicated_lines
            .cmp(&a.duplicated_lines)
            .then_with(|| b.group.lines.cmp(&a.group.lines))
            .then_with(|| b.group.instances.len().cmp(&a.group.instances.len()))
            .then_with(|| a.key.cmp(&b.key))
    });
    candidates
}

pub(super) fn top_duplicate_blocks_where(
    candidates: &[DuplicateCandidate<'_>],
    top: usize,
    min_new_lines: usize,
    mut include: impl FnMut(&CloneGroup) -> bool,
) -> Vec<DuplicateBlock> {
    const MAX_LOCATIONS: usize = 10;
    let mut selected_coverage = BTreeMap::<PathBuf, Vec<Range<usize>>>::new();
    let mut blocks = Vec::with_capacity(top.min(candidates.len()));
    for candidate in candidates {
        if blocks.len() >= top {
            break;
        }
        if !include(candidate.group) {
            continue;
        }
        if !blocks.is_empty()
            && !contributes_new_duplicate_lines(candidate.group, &selected_coverage, min_new_lines)
        {
            continue;
        }
        for instance in &candidate.group.instances {
            if let Some(range) = instance_line_range(instance) {
                insert_line_range(
                    selected_coverage.entry(instance.path.clone()).or_default(),
                    range,
                );
            }
        }

        let copies = candidate.group.instances.len();
        let locations = candidate
            .key
            .iter()
            .take(MAX_LOCATIONS)
            .cloned()
            .collect::<Vec<_>>();
        blocks.push(DuplicateBlock {
            lines: candidate.group.lines,
            tokens: candidate.group.tokens,
            similarity: candidate.group.similarity,
            copies,
            duplicated_lines: candidate.duplicated_lines,
            locations,
        });
    }
    blocks
}

pub(super) struct DuplicateCandidate<'a> {
    group: &'a CloneGroup,
    duplicated_lines: usize,
    key: Vec<String>,
}

pub(super) fn contributes_new_duplicate_lines(
    group: &CloneGroup,
    selected: &BTreeMap<PathBuf, Vec<Range<usize>>>,
    min_new_lines: usize,
) -> bool {
    group
        .instances
        .iter()
        .filter(|instance| {
            instance_line_range(instance).is_some_and(|range| {
                longest_uncovered_run(
                    &range,
                    selected
                        .get(&instance.path)
                        .map(Vec::as_slice)
                        .unwrap_or_default(),
                ) >= min_new_lines
            })
        })
        .take(2)
        .count()
        >= 2
}

pub(super) fn instance_line_range(instance: &CloneInstance) -> Option<Range<usize>> {
    let end_line = occupied_end_line(instance);
    (instance.start_line > 0 && instance.start_line <= end_line)
        .then_some(instance.start_line - 1..end_line)
}

pub(super) fn occupied_end_line(instance: &CloneInstance) -> usize {
    if instance.end_column == 1
        && instance.end_line > instance.start_line
        && instance.end_byte > instance.start_byte
    {
        instance.end_line - 1
    } else {
        instance.end_line
    }
}

pub(super) fn longest_uncovered_run(range: &Range<usize>, covered: &[Range<usize>]) -> usize {
    debug_assert!(
        covered
            .windows(2)
            .all(|pair| pair[0].start <= pair[1].start),
        "covered line ranges must be sorted by start"
    );
    let mut cursor = range.start;
    let mut longest = 0usize;
    for existing in covered {
        if existing.end <= cursor {
            continue;
        }
        if existing.start >= range.end {
            break;
        }
        if existing.start > cursor {
            longest = longest.max(existing.start.min(range.end).saturating_sub(cursor));
        }
        cursor = cursor.max(existing.end);
        if cursor >= range.end {
            return longest;
        }
    }
    longest.max(range.end.saturating_sub(cursor))
}

pub(super) fn insert_line_range(covered: &mut Vec<Range<usize>>, mut range: Range<usize>) {
    let first = covered.partition_point(|existing| existing.end < range.start);
    let mut last = first;
    while last < covered.len() && covered[last].start <= range.end {
        range.start = range.start.min(covered[last].start);
        range.end = range.end.max(covered[last].end);
        last += 1;
    }
    covered.splice(first..last, [range]);
}

pub(super) fn instance_has_production_lines(
    instance: &CloneInstance,
    test_regions: &BTreeMap<PathBuf, Vec<LineRange>>,
    health_policy: &HealthPolicy,
    minimum_lines: usize,
) -> bool {
    let Some(info) = lang::detect(&instance.path) else {
        return false;
    };
    if !info.is_code()
        || !health_policy.includes(&instance.path, info)
        || testcov::is_test_file(instance.path.to_string_lossy().as_ref())
    {
        return false;
    }
    let Some(range) = instance_line_range(instance) else {
        return false;
    };
    let minimum_lines = minimum_lines.max(1);
    let Some(test_regions) = test_regions.get(&instance.path) else {
        return range.end.saturating_sub(range.start) >= minimum_lines;
    };
    if test_regions.is_empty() {
        return range.end.saturating_sub(range.start) >= minimum_lines;
    }
    let mut excluded = Vec::new();
    for test_region in test_regions {
        if test_region.start > 0 && test_region.start <= test_region.end {
            insert_line_range(&mut excluded, test_region.start - 1..test_region.end);
        }
    }
    longest_uncovered_run(&range, &excluded) >= minimum_lines
}

pub(super) fn top_duplicate_findings(
    dup: &Duplication,
    top: usize,
) -> Vec<DuplicateFindingSummary> {
    let mut findings = dup
        .findings
        .iter()
        .map(|finding| DuplicateFindingSummary {
            id: finding.id.clone(),
            kind: finding.kind.clone(),
            format: finding.format.clone(),
            tokens: finding.tokens,
            lines: finding.lines_a.max(finding.lines_b),
            similarity: finding.similarity,
            removable_lines: finding.removable_lines,
            locations: vec![
                format!(
                    "{}:{}:{}-{}:{}",
                    finding.fragment_a.path.display(),
                    finding.fragment_a.start_line,
                    finding.fragment_a.start_column,
                    finding.fragment_a.end_line,
                    finding.fragment_a.end_column
                ),
                format!(
                    "{}:{}:{}-{}:{}",
                    finding.fragment_b.path.display(),
                    finding.fragment_b.start_line,
                    finding.fragment_b.start_column,
                    finding.fragment_b.end_line,
                    finding.fragment_b.end_column
                ),
            ],
        })
        .collect::<Vec<_>>();
    findings.sort_by(|a, b| {
        b.removable_lines
            .cmp(&a.removable_lines)
            .then_with(|| b.tokens.cmp(&a.tokens))
            .then_with(|| b.similarity.total_cmp(&a.similarity))
            .then_with(|| a.id.cmp(&b.id))
    });
    findings.truncate(top);
    findings
}
