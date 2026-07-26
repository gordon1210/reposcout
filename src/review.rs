//! Changed-line review over canonical findings.

use crate::cli::ReviewMode;
use crate::config::Config;
use crate::dup::{self, DetectionOptions, DupInput};
use crate::git::DiffScope;
use crate::model::{
    FindingCatalog, FindingChange, FindingLocation, ReviewChangedFile, ReviewCounts,
    ReviewDiagnostics, ReviewFinding, ReviewReport,
};
use crate::snapshot::SourceSnapshot;
use anyhow::Result;
use std::path::{Path, PathBuf};

pub fn run(
    root: &Path,
    cfg: &Config,
    scope: &DiffScope,
    base_tree_id: Option<&str>,
    changed_files: Vec<ReviewChangedFile>,
    exclusions: &[PathBuf],
) -> Result<ReviewReport> {
    let current = SourceSnapshot::current(root, cfg, scope, exclusions)?;
    let current_catalog = catalog(&current, cfg);
    if cfg.review == Some(ReviewMode::Deep) {
        let base = SourceSnapshot::base(root, cfg, scope, base_tree_id, exclusions)?;
        let before = crate::findings::remap_renames(&catalog(&base, cfg), &changed_files);
        let delta = crate::findings::compare(&before, &current_catalog);
        let binary_files = changed_files.iter().filter(|file| file.binary).count();
        let findings = delta
            .changes
            .iter()
            .filter(|change| {
                change
                    .after
                    .as_ref()
                    .or(change.before.as_ref())
                    .is_some_and(|finding| finding.kind != "risk")
            })
            .filter(|change| change_intersects(change, &changed_files))
            .map(review_change)
            .collect::<Vec<_>>();
        let counts = ReviewCounts {
            new: findings.iter().filter(|item| item.state == "new").count(),
            resolved: findings
                .iter()
                .filter(|item| item.state == "resolved")
                .count(),
            worsened: findings
                .iter()
                .filter(|item| item.state == "worsened")
                .count(),
            improved: findings
                .iter()
                .filter(|item| item.state == "improved")
                .count(),
            current: 0,
        };
        return Ok(ReviewReport {
            mode: "deep".to_string(),
            scope: scope_name(scope).to_string(),
            changed_files,
            counts,
            findings,
            diagnostics: ReviewDiagnostics {
                binary_files,
                unreadable_files: base.unreadable_files + current.unreadable_files,
            },
        });
    }
    Ok(filter_lines(&current_catalog, changed_files, scope))
}

fn catalog(snapshot: &SourceSnapshot, cfg: &Config) -> FindingCatalog {
    let files = snapshot
        .iter()
        .filter_map(|(path, content)| crate::scan::analyze_source(path, content, cfg, None))
        .collect::<Vec<_>>();
    let duplication = if cfg.enabled.duplication {
        let inputs = snapshot
            .iter()
            .map(|(path, content)| DupInput {
                path: path.to_path_buf(),
                content: content.to_string(),
            })
            .collect::<Vec<_>>();
        dup::analyze(
            &inputs,
            cfg.min_dup_tokens,
            cfg.min_dup_lines,
            cfg.near_dup_min_similarity,
            DetectionOptions {
                mode: cfg.duplication_mode,
                format_scope: cfg.duplication_format_scope,
                report_snippets: false,
            },
        )
        .duplication
    } else {
        Default::default()
    };
    crate::findings::build(&files, &duplication, &[], cfg)
}

pub fn filter_lines(
    catalog: &FindingCatalog,
    changed_files: Vec<ReviewChangedFile>,
    scope: &DiffScope,
) -> ReviewReport {
    let mut findings = catalog
        .findings
        .iter()
        .filter(|finding| finding.kind != "risk")
        .filter(|finding| {
            location_changed(&finding.primary_location, &changed_files)
                || finding
                    .related_locations
                    .iter()
                    .any(|location| location_changed(location, &changed_files))
        })
        .cloned()
        .map(|finding| ReviewFinding {
            state: "current".to_string(),
            finding: finding.clone(),
            before: None,
            after: Some(finding),
        })
        .collect::<Vec<_>>();
    findings.sort_by(|a, b| {
        a.finding
            .primary_location
            .path
            .cmp(&b.finding.primary_location.path)
            .then(
                a.finding
                    .primary_location
                    .start_line
                    .cmp(&b.finding.primary_location.start_line),
            )
            .then(a.finding.kind.cmp(&b.finding.kind))
            .then(a.finding.fingerprint.cmp(&b.finding.fingerprint))
    });
    let current = findings.len();
    let binary_files = changed_files.iter().filter(|file| file.binary).count();
    ReviewReport {
        mode: "lines".to_string(),
        scope: scope_name(scope).to_string(),
        changed_files,
        counts: ReviewCounts {
            current,
            ..ReviewCounts::default()
        },
        findings,
        diagnostics: ReviewDiagnostics {
            binary_files,
            unreadable_files: 0,
        },
    }
}

fn location_changed(location: &FindingLocation, files: &[ReviewChangedFile]) -> bool {
    files.iter().any(|file| {
        file.path.as_deref() == Some(location.path.as_path())
            && file.ranges.iter().any(|range| {
                ranges_overlap(
                    location.start_line,
                    location.end_line.max(location.start_line),
                    range.start,
                    range.end,
                )
            })
    })
}

fn old_location_changed(location: &FindingLocation, files: &[ReviewChangedFile]) -> bool {
    files.iter().any(|file| {
        (file.old_path.as_deref() == Some(location.path.as_path())
            || (file.status == "renamed" && file.path.as_deref() == Some(location.path.as_path())))
            && file.old_ranges.iter().any(|range| {
                ranges_overlap(
                    location.start_line,
                    location.end_line.max(location.start_line),
                    range.start,
                    range.end,
                )
            })
    })
}

fn change_intersects(change: &FindingChange, files: &[ReviewChangedFile]) -> bool {
    change
        .after
        .as_ref()
        .is_some_and(|finding| finding_intersects(finding, files, false))
        || change
            .before
            .as_ref()
            .is_some_and(|finding| finding_intersects(finding, files, true))
}

fn finding_intersects(
    finding: &crate::model::FindingRecord,
    files: &[ReviewChangedFile],
    old: bool,
) -> bool {
    let matches = |location: &FindingLocation| {
        if old {
            old_location_changed(location, files)
        } else {
            location_changed(location, files)
        }
    };
    matches(&finding.primary_location) || finding.related_locations.iter().any(matches)
}

fn review_change(change: &FindingChange) -> ReviewFinding {
    let finding = change
        .after
        .as_ref()
        .or(change.before.as_ref())
        .expect("finding changes retain one side")
        .clone();
    ReviewFinding {
        state: change.state.clone(),
        finding,
        before: change.before.clone(),
        after: change.after.clone(),
    }
}

fn ranges_overlap(a_start: usize, a_end: usize, b_start: usize, b_end: usize) -> bool {
    a_start <= b_end && b_start <= a_end
}

fn scope_name(scope: &DiffScope) -> &'static str {
    match scope {
        DiffScope::Since(_) => "since",
        DiffScope::Staged => "staged",
        DiffScope::Working => "working",
    }
}

#[cfg(test)]
mod tests {
    use super::old_location_changed;
    use crate::model::{FindingLocation, LineRange, ReviewChangedFile};
    use std::path::PathBuf;

    #[test]
    fn remapped_rename_location_uses_old_line_ranges() {
        let location = FindingLocation {
            path: PathBuf::from("new.py"),
            start_line: 3,
            end_line: 3,
            ..FindingLocation::default()
        };
        let changed = ReviewChangedFile {
            old_path: Some(PathBuf::from("old.py")),
            path: Some(PathBuf::from("new.py")),
            status: "renamed".to_string(),
            old_ranges: vec![LineRange { start: 3, end: 3 }],
            ..ReviewChangedFile::default()
        };

        assert!(old_location_changed(&location, &[changed]));
    }
}
