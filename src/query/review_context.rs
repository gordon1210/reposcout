mod analysis;
mod changes;
#[cfg(test)]
mod tests;

use crate::config::Config;
use crate::metrics::tokens::TokenCounter;
use crate::model::{
    ReviewContextCapability, ReviewContextReport, ReviewContextTotals, SCHEMA_VERSION,
};
use crate::report::Format;
use anyhow::{Result, ensure};
use std::path::{Path, PathBuf};

const ENTRY_LIMIT: usize = 100;

/// Immutable comparison, explicit content options, and independent source/output budgets.
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent query options preserve explicit comparison and content choices"
)]
pub struct ReviewContextOptions {
    pub base: String,
    pub head: String,
    pub merge_base: bool,
    pub context: bool,
    pub include_source: bool,
    pub include_diff: bool,
    pub token_budget: usize,
    pub byte_budget: usize,
    pub format: Format,
    pub pretty_json: bool,
}

pub struct ReviewContextOutput {
    pub report: ReviewContextReport,
    pub rendered: String,
}

/// Prepare revision-consistent changes, static impact, context costs and optional source.
///
/// # Errors
/// Returns an error for unsupported platforms, invalid revisions/options, inaccessible
/// repositories, Git/analysis failures, or an output budget smaller than the status envelope.
pub fn review_context(
    target: &Path,
    cfg: &Config,
    exclusions: &[PathBuf],
    options: &ReviewContextOptions,
) -> Result<ReviewContextOutput> {
    ensure!(
        cfg!(unix),
        "review-context is available only on Unix platforms"
    );
    super::source::validate_output_options(&super::SourceQueryOptions {
        targets: Vec::new(),
        token_budget: options.token_budget,
        byte_budget: options.byte_budget,
        format: options.format,
        pretty_json: options.pretty_json,
    })?;
    let mut config = super::declaration_query_config(cfg);
    config.enabled.tokens = true;
    let counter = TokenCounter::new(&config.encoding)?;
    let mut capture = crate::scan::capture_review(
        target,
        &config,
        exclusions,
        &options.base,
        &options.head,
        options.merge_base,
    )?;
    let changes = changes::describe(&capture, &counter, options.include_diff)?;
    let mut graph_limits = crate::graph::GraphReadLimits::from_config(&config);
    graph_limits.deadline = Some(capture.deadline);
    let (mut context, relations) =
        analysis::analyze(&mut capture, &changes, &counter, graph_limits)?;
    check_deadline(capture.deadline)?;
    let mut totals = totals(&changes, &context, relations.len(), capture.total_changes);
    let selection = options.context || options.include_source;
    if selection {
        crate::context::review::select(
            &mut context,
            config.context_budget,
            config.context_max_files,
            &mut totals,
        );
    }
    if options.include_source {
        for file in &mut context {
            check_deadline(capture.deadline)?;
            if file.selection != "selected" {
                continue;
            }
            let revision = &capture.revisions[usize::from(file.side == "head")];
            file.source = revision
                .files()
                .get(&file.path)
                .map(|source| source.content.clone());
        }
    }
    totals.source_files = context.iter().filter(|file| file.source.is_some()).count();
    let mut report = ReviewContextReport {
        kind: "review_context".into(), schema_version: SCHEMA_VERSION.into(), strategy_version: 1,
        comparison: capture.comparison, encoding: counter.name().into(), token_budget: options.token_budget, byte_budget: options.byte_budget,
        context_budget: selection.then_some(config.context_budget), context_max_files: selection.then_some(config.context_max_files),
        totals, coverage: capture.revisions.into_iter().map(|revision| revision.coverage).collect(),
        limitations: [
            "Static evidence describes observed potential impact; dynamic/runtime and external consumers are unknown.",
            "Source costs count each whole file once per revision; base and head are distinct. Diff costs and rendered output are separate, not total model usage.",
            "Test hints are filename conventions or syntax, not executed tests or measured coverage.",
            "Current configuration and ignore policy apply to both revisions; source and resolver contents come only from pinned Git trees.",
            "An empty dependency result is inconclusive for excluded, unavailable or unsupported graph files.",
            "Literal Unix backslash paths retain source/change evidence but are excluded from graph inputs; unsupported_graph_paths counts them.",
        ].map(str::to_string).to_vec(),
        changes, relations, context,
    };
    project(&mut report, options, &counter, capture.deadline)?;
    let rendered =
        crate::report::review_context::render(&report, options.format, options.pretty_json)?;
    check_deadline(capture.deadline)?;
    Ok(ReviewContextOutput { report, rendered })
}

fn totals(
    changes: &[crate::model::ReviewContextChange],
    context: &[crate::model::ReviewContextFile],
    relations: usize,
    total_changes: usize,
) -> ReviewContextTotals {
    ReviewContextTotals {
        changes: total_changes,
        changes_not_analyzed: total_changes.saturating_sub(changes.len()),
        changes_without_hunks: changes
            .iter()
            .filter(|change| change.hunk_status == crate::model::ReviewAnalysisStatus::Unavailable)
            .count(),
        definitions: changes
            .iter()
            .flat_map(|change| [change.base.as_ref(), change.head.as_ref()])
            .flatten()
            .map(|side| side.definitions.len())
            .sum(),
        relations,
        candidates: context.len(),
        candidate_tokens: context.iter().filter_map(|file| file.tokens).sum(),
        candidate_bytes: context.iter().filter_map(|file| file.bytes).sum(),
        unknown_candidate_sizes: context.iter().filter(|file| file.tokens.is_none()).count(),
        diff_tokens: changes.iter().filter_map(|change| change.diff_tokens).sum(),
        diff_unavailable_files: changes
            .iter()
            .filter(|change| change.diff_tokens.is_none())
            .count()
            + total_changes.saturating_sub(changes.len()),
        ..ReviewContextTotals::default()
    }
}

fn project(
    report: &mut ReviewContextReport,
    options: &ReviewContextOptions,
    counter: &TokenCounter,
    deadline: std::time::Instant,
) -> Result<()> {
    report.changes.truncate(ENTRY_LIMIT);
    report.context.truncate(ENTRY_LIMIT);
    report.relations.truncate(ENTRY_LIMIT);
    loop {
        check_deadline(deadline)?;
        recount(report, options);
        let rendered =
            crate::report::review_context::render(report, options.format, options.pretty_json)?;
        if rendered.len() <= options.byte_budget && counter.count(&rendered) <= options.token_budget
        {
            check_deadline(deadline)?;
            return Ok(());
        }
        if let Some(file) = report
            .context
            .iter_mut()
            .rev()
            .find(|file| file.source.is_some())
        {
            file.source = None;
        } else if let Some(change) = report
            .changes
            .iter_mut()
            .rev()
            .find(|change| change.diff.is_some())
        {
            change.diff = None;
        } else if !(report.relations.pop().is_some()
            || report.context.pop().is_some()
            || report.changes.pop().is_some())
        {
            anyhow::bail!(
                "review-context output budget cannot hold the status envelope; increase --budget or --max-output-bytes"
            );
        }
    }
}

fn check_deadline(deadline: std::time::Instant) -> Result<()> {
    ensure!(
        std::time::Instant::now() < deadline,
        "review analysis exceeded the configured duration limit"
    );
    Ok(())
}

fn recount(report: &mut ReviewContextReport, options: &ReviewContextOptions) {
    let totals = &mut report.totals;
    totals.changes_omitted = totals
        .changes
        .saturating_sub(totals.changes_not_analyzed)
        .saturating_sub(report.changes.len());
    totals.definitions_omitted = totals.definitions.saturating_sub(
        report
            .changes
            .iter()
            .flat_map(|change| [change.base.as_ref(), change.head.as_ref()])
            .flatten()
            .map(|side| side.definitions.len())
            .sum(),
    );
    totals.relations_omitted = totals.relations.saturating_sub(report.relations.len());
    totals.candidates_omitted = totals.candidates.saturating_sub(report.context.len());
    totals.source_files_omitted = totals.source_files.saturating_sub(
        report
            .context
            .iter()
            .filter(|file| file.source.is_some())
            .count(),
    );
    if options.include_diff {
        totals.diff_files_omitted = totals
            .changes
            .saturating_sub(totals.diff_unavailable_files)
            .saturating_sub(
                report
                    .changes
                    .iter()
                    .filter(|change| change.diff.is_some())
                    .count(),
            );
    }
}

pub(super) fn capability() -> ReviewContextCapability {
    ReviewContextCapability {
        command: "review-context".into(),
        available: cfg!(unix),
        formats: ["table", "json", "markdown", "ndjson"]
            .map(str::to_string)
            .to_vec(),
        comparisons: ["direct", "merge-base"].map(str::to_string).to_vec(),
        max_inventory_files_per_revision: 10_000,
        max_input_files: 10_000,
        max_input_bytes: 32 * 1024 * 1024,
        max_input_file_bytes: 8 * 1024 * 1024,
        default_tokens: 4096,
        default_bytes: 65_536,
        context_unit: "unique-whole-file-per-revision".into(),
    }
}
