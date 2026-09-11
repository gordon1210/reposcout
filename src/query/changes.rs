use super::changed_mapping::{changed_hunks, map_changed_definitions};
use super::source::{self, SourceQueryOptions, SourceQueryOutput, budget, selection};
use crate::config::Config;
use crate::git::DiffScope;
use crate::metrics::tokens::TokenCounter;
use crate::model::{
    DefinitionStatus, LineRange, SourceChangeEvidence, SourceChangeSummary, SourceQueryFile,
    SourceQueryStatus, SourceRevision,
};
use crate::report::Format;
use crate::scan::{self, ExplicitSourceBatch};
use anyhow::{Result, ensure};
use selection::ResolvedTarget;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const MAX_RESULT_TARGETS: usize = 128;

struct Targets<'a> {
    entries: Vec<ResolvedTarget<'a>>,
    total: usize,
}

impl<'a> Targets<'a> {
    fn push(&mut self, create: impl FnOnce() -> ResolvedTarget<'a>) {
        self.total = self.total.saturating_add(1);
        if self.entries.len() < MAX_RESULT_TARGETS {
            self.entries.push(create());
        }
    }
}

/// Diff scope, explicit source delivery and complete-response rendering limits for a changed-definition query.
pub struct ChangeQueryOptions {
    pub scope: DiffScope,
    pub include_source: bool,
    pub token_budget: usize,
    pub byte_budget: usize,
    pub format: Format,
    pub pretty_json: bool,
}

/// Select changed definitions from captured old/new source and render body-free evidence or explicitly requested complete source within shared output budgets.
///
/// # Errors
///
/// Returns an error on non-Unix platforms before source I/O, or for invalid output options, target
/// roots or diff revisions, unrecoverable Git capture, analysis or hunk-generation failures,
/// token-counter initialization or serialization failures, or a budget that cannot hold the
/// minimal status envelope.
pub fn query_changes(
    target: &Path,
    cfg: &Config,
    exclusions: &[PathBuf],
    options: &ChangeQueryOptions,
) -> Result<SourceQueryOutput> {
    ensure!(cfg!(unix), "changes is available only on Unix platforms");
    let source_options = SourceQueryOptions {
        targets: Vec::new(),
        token_budget: options.token_budget,
        byte_budget: options.byte_budget,
        format: options.format,
        pretty_json: options.pretty_json,
    };
    source::validate_output_options(&source_options)?;
    if let DiffScope::Since(reference) = &options.scope {
        ensure!(
            !reference.is_empty() && reference.len() <= 1_024,
            "change revision must contain between 1 and 1024 bytes"
        );
    }
    let captured = scan::capture_changed_sources(
        target,
        &source::query_config(cfg),
        exclusions,
        &options.scope,
    )?;
    let old_files = describe_side(&captured.old, &captured.base_revision, 0);
    let new_files = describe_side(&captured.new, &captured.current_revision, old_files.len());
    let mut summary = SourceChangeSummary {
        scope: match &options.scope {
            DiffScope::Working => "working",
            DiffScope::Staged => "staged",
            DiffScope::Since(_) => "since",
        }
        .to_string(),
        base: captured.base_revision.clone(),
        current: captured.current_revision.clone(),
        total_files: captured.total_changed_files,
        processed_files: captured.changed_files.len(),
        omitted_files: captured.omitted_changed_files,
        mapped_definitions: 0,
        unmapped_ranges: 0,
        unavailable_sides: 0,
        total_hunks: 0,
        omitted_hunks: 0,
        unprocessed_ranges: 0,
    };
    let mut targets = Targets {
        entries: Vec::new(),
        total: 0,
    };
    for file in &captured.changed_files {
        let change = &file.change;
        let old = Side {
            name: "base",
            path: change.old_path.as_deref(),
            counterpart: change.path.as_deref(),
            outside_scope: file.old_outside_scope,
            batch: &captured.old,
            files: &old_files,
            status: &change.status,
        };
        let new = Side {
            name: "current",
            path: change.path.as_deref(),
            counterpart: change.old_path.as_deref(),
            outside_scope: file.new_outside_scope,
            batch: &captured.new,
            files: &new_files,
            status: &change.status,
        };
        collect_pair(
            &old,
            &new,
            options.include_source,
            &mut summary,
            &mut targets,
        )?;
    }
    render_changes(
        &captured.old.root,
        &source_options,
        cfg,
        options.include_source,
        summary,
        targets,
    )
}

fn describe_side(
    batch: &ExplicitSourceBatch,
    revision: &SourceRevision,
    offset: usize,
) -> BTreeMap<PathBuf, SourceQueryFile> {
    let paths = batch
        .files
        .keys()
        .chain(batch.failures.keys())
        .cloned()
        .collect::<Vec<_>>();
    let mut files = source::describe_files(batch, &paths);
    for file in files.values_mut() {
        file.id += offset;
        file.snapshot = revision.clone();
    }
    files
}

struct Side<'a> {
    name: &'static str,
    path: Option<&'a Path>,
    counterpart: Option<&'a Path>,
    outside_scope: bool,
    batch: &'a ExplicitSourceBatch,
    files: &'a BTreeMap<PathBuf, SourceQueryFile>,
    status: &'a str,
}

impl Side<'_> {
    fn content(&self) -> Option<&str> {
        if self.outside_scope {
            return None;
        }
        match self.path {
            None => Some(""),
            Some(path) => self.batch.files.get(path).map(|file| file.content.as_str()),
        }
    }

    fn evidence(&self, reason: &str, ranges: Vec<LineRange>) -> SourceChangeEvidence {
        SourceChangeEvidence {
            side: self.name.to_string(),
            file_status: self.status.to_string(),
            counterpart: self.counterpart.map(Path::to_path_buf),
            reason: reason.to_string(),
            ranges,
            wrapper_ranges: Vec::new(),
            ambiguous: false,
        }
    }

    fn result(
        &self,
        status: SourceQueryStatus,
        reason: &str,
        ranges: Vec<LineRange>,
    ) -> ResolvedTarget<'static> {
        let file = self.path.and_then(|path| self.files.get(path)).cloned();
        let mut result = selection::empty_result(0, file.as_ref().map(|file| file.id));
        result.status = status;
        result.change = Some(self.evidence(reason, ranges));
        ResolvedTarget {
            file,
            result,
            source: None,
        }
    }
}

fn collect_pair<'a>(
    old: &Side<'a>,
    new: &Side<'a>,
    include_source: bool,
    summary: &mut SourceChangeSummary,
    targets: &mut Targets<'a>,
) -> Result<()> {
    let (Some(old_content), Some(new_content)) = (old.content(), new.content()) else {
        for side in [old, new] {
            if side.path.is_none() && !side.outside_scope {
                continue;
            }
            summary.unavailable_sides += 1;
            let (status, reason) = if side.outside_scope {
                (SourceQueryStatus::Excluded, "outside-target")
            } else if let Some(failure) = side.path.and_then(|path| side.batch.failures.get(path)) {
                (selection::failure_status(*failure), "snapshot-unavailable")
            } else {
                (SourceQueryStatus::Unavailable, "counterpart-unavailable")
            };
            targets.push(|| side.result(status, reason, Vec::new()));
        }
        return Ok(());
    };
    let changes = changed_hunks(old_content, new_content)?;
    summary.total_hunks += changes.hunks.len() + changes.omitted_hunks;
    summary.omitted_hunks += changes.omitted_hunks;
    for (side, is_old) in [(old, true), (new, false)] {
        if side.path.is_none() {
            continue;
        }
        let ranges = changes
            .hunks
            .iter()
            .filter_map(|hunk| {
                let (start, count) = if is_old {
                    (hunk.old_start, hunk.old_lines)
                } else {
                    (hunk.new_start, hunk.new_lines)
                };
                (count > 0).then(|| LineRange {
                    start,
                    end: start.saturating_add(count).saturating_sub(1),
                })
            })
            .collect::<Vec<_>>();
        if ranges.is_empty() {
            targets.push(|| {
                side.result(
                    SourceQueryStatus::Changed,
                    "file-change-without-changed-lines",
                    Vec::new(),
                )
            });
        } else {
            collect_side(side, &ranges, include_source, summary, targets);
        }
    }
    Ok(())
}

fn collect_side<'a>(
    side: &Side<'a>,
    ranges: &[LineRange],
    include_source: bool,
    summary: &mut SourceChangeSummary,
    targets: &mut Targets<'a>,
) {
    let Some(loaded) = side.path.and_then(|path| side.batch.files.get(path)) else {
        return;
    };
    let mapped = map_changed_definitions(&loaded.definitions, ranges);
    summary.mapped_definitions += mapped.definitions.len();
    summary.unmapped_ranges += mapped.uncovered.len();
    summary.unprocessed_ranges += mapped.unprocessed.len();
    for selected in mapped.definitions {
        targets.push(|| {
            let definition = &loaded.definitions.definitions[selected.definition_index];
            let mut resolved =
                side.result(SourceQueryStatus::Changed, "changed-lines", selected.ranges);
            resolved.result.definition = Some(selection::describe(definition, false));
            if let Some(evidence) = &mut resolved.result.change {
                if evidence.ranges.is_empty() {
                    evidence.reason = "wrapper-overlap".to_string();
                }
                evidence.wrapper_ranges = selected.wrapper_ranges;
                evidence.ambiguous = selected.ambiguous;
            }
            if include_source {
                if let Some(span) = definition
                    .source_span
                    .filter(|span| loaded.content.get(span.start_byte..span.end_byte).is_some())
                {
                    resolved.result.status = SourceQueryStatus::Complete;
                    resolved.source = Some((&loaded.content, span));
                } else {
                    resolved.result.status = SourceQueryStatus::ParseError;
                }
            }
            resolved
        });
    }
    let (status, reason) = match mapped.status {
        DefinitionStatus::Available => (SourceQueryStatus::Unmapped, "outside-definitions"),
        DefinitionStatus::ParseErrors => (SourceQueryStatus::ParseError, "extraction-gap"),
        DefinitionStatus::Unsupported => (SourceQueryStatus::Unsupported, "unsupported-extraction"),
        DefinitionStatus::Unavailable => (SourceQueryStatus::Unavailable, "unavailable-extraction"),
    };
    if !mapped.uncovered.is_empty() {
        targets.push(|| side.result(status, reason, mapped.uncovered));
    }
    if !mapped.unprocessed.is_empty() {
        targets.push(|| {
            side.result(
                SourceQueryStatus::Unavailable,
                "mapping-work-limit",
                mapped.unprocessed,
            )
        });
    }
}

fn render_changes(
    root: &Path,
    options: &SourceQueryOptions,
    cfg: &Config,
    include_source: bool,
    summary: SourceChangeSummary,
    targets: Targets<'_>,
) -> Result<SourceQueryOutput> {
    let counter = TokenCounter::new(&cfg.encoding)?;
    let mut report = source::empty_report(root, options, counter.name());
    report.kind = "change_query".to_string();
    report.mode = if include_source {
        "changes-source"
    } else {
        "changes"
    }
    .to_string();
    report.requested_targets = targets.total;
    report.omitted_targets = targets.total;
    report.change = Some(summary);
    if !budget::fits(&report, options, &counter)? {
        report.root = None;
        report.root_omitted = true;
    }
    ensure!(
        budget::fits(&report, options, &counter)?,
        "change budget cannot fit the status envelope"
    );
    for (index, mut target) in targets.entries.into_iter().enumerate() {
        target.result.target = index + 1;
        budget::admit(&mut report, target, options, &counter)?;
    }
    let rendered = crate::report::source::render(&report, options.format, options.pretty_json)?;
    ensure!(
        rendered.len() <= options.byte_budget && counter.count(&rendered) <= options.token_budget,
        "change output exceeded the validated budget"
    );
    Ok(SourceQueryOutput { report, rendered })
}
