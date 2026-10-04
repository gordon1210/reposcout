use crate::metrics::tokens::TokenCounter;
use crate::model::{
    DefinitionStatus, LineRange, ReviewAnalysisStatus, ReviewChangedSide, ReviewContextChange,
};
use crate::query::changed_mapping::{changed_hunks, map_changed_definitions};
use crate::scan::{ReviewCapture, ReviewRevision};
use crate::snapshot::comparison::ComparisonFile;
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;

pub(super) fn sha256(content: &str) -> String {
    Sha256::digest(content.as_bytes())
        .iter()
        .fold(String::with_capacity(64), |mut hash, byte| {
            let _ = write!(hash, "{byte:02x}");
            hash
        })
}

pub(super) fn describe(
    captured: &ReviewCapture,
    counter: &TokenCounter,
    include_diff: bool,
) -> Result<Vec<ReviewContextChange>> {
    super::check_deadline(captured.deadline)?;
    captured
        .changes
        .iter()
        .map(|change| {
            super::check_deadline(captured.deadline)?;
            let mut base = change
                .base
                .as_ref()
                .map(|file| side(file, &captured.revisions[0]));
            let mut head = change
                .head
                .as_ref()
                .map(|file| side(file, &captured.revisions[1]));
            let old = content(change.base.as_ref(), &captured.revisions[0]);
            let new = content(change.head.as_ref(), &captured.revisions[1]);
            let mut result = ReviewContextChange {
                status: change.status.clone(),
                base: None,
                head: None,
                hunk_status: ReviewAnalysisStatus::Unavailable,
                hunks: None,
                hunks_omitted: 0,
                diff_tokens: None,
                diff: None,
            };
            if let (Some(old), Some(new)) = (old, new) {
                let hunks = changed_hunks(old, new)?;
                result.hunks = Some(hunks.hunks.len() + hunks.omitted_hunks);
                result.hunks_omitted = hunks.omitted_hunks;
                result.hunk_status = if hunks.omitted_hunks == 0 {
                    ReviewAnalysisStatus::Available
                } else {
                    ReviewAnalysisStatus::Partial
                };
                for (side, revision, old_side) in [
                    (&mut base, &captured.revisions[0], true),
                    (&mut head, &captured.revisions[1], false),
                ] {
                    if let Some(side) = side {
                        side.ranges = ranges(&hunks, old_side);
                        map(side, revision);
                        if hunks.omitted_hunks > 0
                            && side.mapping_status == ReviewAnalysisStatus::Available
                        {
                            side.mapping_status = ReviewAnalysisStatus::Partial;
                        }
                    }
                }
                let patch = patch(old, new, change.base.as_ref(), change.head.as_ref())?;
                result.diff_tokens = Some(counter.count(&patch));
                if include_diff {
                    result.diff = Some(patch);
                }
            }
            result.base = base;
            result.head = head;
            super::check_deadline(captured.deadline)?;
            Ok(result)
        })
        .collect()
}

fn ranges(hunks: &crate::query::changed_mapping::HunkChanges, old_side: bool) -> Vec<LineRange> {
    hunks
        .hunks
        .iter()
        .filter_map(|hunk| {
            let (start, count) = if old_side {
                (hunk.old_start, hunk.old_lines)
            } else {
                (hunk.new_start, hunk.new_lines)
            };
            (count > 0).then(|| LineRange {
                start,
                end: start.saturating_add(count).saturating_sub(1),
            })
        })
        .collect()
}

fn content<'a>(file: Option<&ComparisonFile>, revision: &'a ReviewRevision) -> Option<&'a str> {
    file.map_or(Some(""), |file| {
        revision
            .files()
            .get(&file.path)
            .map(|source| source.content.as_str())
    })
}

fn side(file: &ComparisonFile, revision: &ReviewRevision) -> ReviewChangedSide {
    let captured = revision.files().get(&file.path);
    ReviewChangedSide {
        path: file.path.clone(),
        mode: file.mode,
        blob: file.blob.clone(),
        status: captured
            .map_or_else(
                || {
                    revision
                        .sources
                        .failures
                        .get(&file.path)
                        .map_or("not-captured", |reason| crate::scan::failure_name(*reason))
                },
                |_| "captured",
            )
            .into(),
        sha256: captured.map(|file| sha256(&file.content)),
        extraction: captured.map_or(DefinitionStatus::Unavailable, |file| {
            file.definitions.status
        }),
        mapping_status: ReviewAnalysisStatus::Unavailable,
        ranges: Vec::new(),
        definitions: Vec::new(),
        definitions_omitted: 0,
        ranges_omitted: 0,
        unmapped_ranges: 0,
        unprocessed_ranges: 0,
        ambiguous_definitions: 0,
        wrapper_ranges: 0,
    }
}

fn map(side: &mut ReviewChangedSide, revision: &ReviewRevision) {
    let Some(file) = revision.files().get(&side.path) else {
        return;
    };
    let mapped = map_changed_definitions(&file.definitions, &side.ranges);
    side.mapping_status = match file.definitions.status {
        DefinitionStatus::Unavailable | DefinitionStatus::Unsupported => {
            ReviewAnalysisStatus::Unavailable
        }
        DefinitionStatus::ParseErrors => ReviewAnalysisStatus::Partial,
        DefinitionStatus::Available
            if !mapped.unprocessed.is_empty()
                || mapped.definitions.iter().any(|item| item.ambiguous) =>
        {
            ReviewAnalysisStatus::Partial
        }
        DefinitionStatus::Available => ReviewAnalysisStatus::Available,
    };
    side.unmapped_ranges = mapped.uncovered.len();
    side.unprocessed_ranges = mapped.unprocessed.len();
    side.ambiguous_definitions = mapped
        .definitions
        .iter()
        .filter(|item| item.ambiguous)
        .count();
    side.wrapper_ranges = mapped
        .definitions
        .iter()
        .map(|item| item.wrapper_ranges.len())
        .sum();
    side.definitions = mapped
        .definitions
        .iter()
        .map(|item| file.definitions.definitions[item.definition_index].clone())
        .collect();
}

fn patch(
    old: &str,
    new: &str,
    base: Option<&ComparisonFile>,
    head: Option<&ComparisonFile>,
) -> Result<String> {
    let mut options = git2::DiffOptions::new();
    options.context_lines(3);
    let mut patch = git2::Patch::from_buffers(
        old.as_bytes(),
        base.map(|file| file.path.as_path()),
        new.as_bytes(),
        head.map(|file| file.path.as_path()),
        Some(&mut options),
    )?;
    let buffer = patch.to_buf()?;
    Ok(String::from_utf8_lossy(buffer.as_ref()).into_owned())
}
