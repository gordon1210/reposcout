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
        kind: "review_context".into(), schema_version: SCHEMA_VERSION.into(), strategy_version: 2,
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
    let originals = report.clone();
    loop {
        if within_budget(report, options, counter, deadline)? {
            restore_change_identities(report, &originals.changes, options, counter, deadline)?;
            restore_change_details(report, &originals.changes, options, counter, deadline)?;
            restore_evidence(report, &originals, options, counter, deadline)?;
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
        } else if trim_definitions(report) || trim_ranges(report) {
            // Preserve changed-file identities before spending the budget on unbounded details.
        } else if !(report.relations.pop().is_some()
            || report.context.pop().is_some()
            || remove_largest_change(report)?)
        {
            anyhow::bail!(
                "review-context output budget cannot hold the status envelope; increase --budget or --max-output-bytes"
            );
        }
    }
}

fn within_budget(
    report: &mut ReviewContextReport,
    options: &ReviewContextOptions,
    counter: &TokenCounter,
    deadline: std::time::Instant,
) -> Result<bool> {
    check_deadline(deadline)?;
    recount(report, options);
    let rendered =
        crate::report::review_context::render(report, options.format, options.pretty_json)?;
    let fits =
        rendered.len() <= options.byte_budget && counter.count(&rendered) <= options.token_budget;
    check_deadline(deadline)?;
    Ok(fits)
}

fn restore_change_identities(
    report: &mut ReviewContextReport,
    originals: &[crate::model::ReviewContextChange],
    options: &ReviewContextOptions,
    counter: &TokenCounter,
    deadline: std::time::Instant,
) -> Result<()> {
    let mut index = 0;
    for original in originals {
        if report.changes.get(index).is_some_and(|change| {
            change.base.as_ref().map(|side| &side.path)
                == original.base.as_ref().map(|side| &side.path)
                && change.head.as_ref().map(|side| &side.path)
                    == original.head.as_ref().map(|side| &side.path)
        }) {
            index += 1;
            continue;
        }
        let mut compact = original.clone();
        compact.diff = None;
        for side in [compact.base.as_mut(), compact.head.as_mut()]
            .into_iter()
            .flatten()
        {
            side.definitions_omitted += side.definitions.len();
            side.definitions.clear();
            side.ranges_omitted += side.ranges.len();
            side.ranges.clear();
        }
        // Byte-ranked eviction is only a heuristic: admission uses the actual format and tokenizer.
        report.changes.insert(index, compact);
        if within_budget(report, options, counter, deadline)? {
            index += 1;
        } else {
            report.changes.remove(index);
        }
    }
    recount(report, options);
    Ok(())
}

fn restore_change_details(
    report: &mut ReviewContextReport,
    originals: &[crate::model::ReviewContextChange],
    options: &ReviewContextOptions,
    counter: &TokenCounter,
    deadline: std::time::Instant,
) -> Result<()> {
    let mut candidates = Vec::new();
    for (index, change) in report.changes.iter().enumerate() {
        if ![change.base.as_ref(), change.head.as_ref()]
            .into_iter()
            .flatten()
            .any(|side| side.definitions_omitted > 0 || side.ranges_omitted > 0)
        {
            continue;
        }
        let identity = |change: &crate::model::ReviewContextChange| {
            (
                change.base.as_ref().map(|side| side.path.clone()),
                change.head.as_ref().map(|side| side.path.clone()),
            )
        };
        if let Some(original) = originals
            .iter()
            .find(|original| identity(original) == identity(change))
        {
            let mut restored = original.clone();
            restored.diff.clone_from(&change.diff);
            candidates.push((serde_json::to_vec(&restored)?.len(), index, restored));
        }
    }
    // Whole-entry removal can free space for previously trimmed small changes.
    candidates.sort_by_key(|(cost, index, _)| (*cost, *index));
    for (_, index, restored) in candidates {
        let previous = std::mem::replace(&mut report.changes[index], restored);
        if !within_budget(report, options, counter, deadline)? {
            report.changes[index] = previous;
        }
    }
    recount(report, options);
    Ok(())
}

fn restore_evidence(
    report: &mut ReviewContextReport,
    originals: &ReviewContextReport,
    options: &ReviewContextOptions,
    counter: &TokenCounter,
    deadline: std::time::Instant,
) -> Result<()> {
    for relation in originals.relations.iter().skip(report.relations.len()) {
        report.relations.push(relation.clone());
        if !within_budget(report, options, counter, deadline)? {
            report.relations.pop();
        }
    }
    for file in originals.context.iter().skip(report.context.len()) {
        let mut metadata = file.clone();
        metadata.source = None;
        report.context.push(metadata);
        if !within_budget(report, options, counter, deadline)? {
            report.context.pop();
        }
    }
    // Restore requested whole bodies only after identities and impact evidence have had a chance.
    for index in 0..report.changes.len() {
        if report.changes[index].diff.is_some() {
            continue;
        }
        report.changes[index].diff = originals.changes.iter().find_map(|original| {
            let current = &report.changes[index];
            (original.base.as_ref().map(|side| &side.path)
                == current.base.as_ref().map(|side| &side.path)
                && original.head.as_ref().map(|side| &side.path)
                    == current.head.as_ref().map(|side| &side.path))
            .then(|| original.diff.clone())
            .flatten()
        });
        if report.changes[index].diff.is_some()
            && !within_budget(report, options, counter, deadline)?
        {
            report.changes[index].diff = None;
        }
    }
    for index in 0..report.context.len() {
        if report.context[index].source.is_some() {
            continue;
        }
        report.context[index].source = originals.context.iter().find_map(|original| {
            let current = &report.context[index];
            (original.side == current.side && original.path == current.path)
                .then(|| original.source.clone())
                .flatten()
        });
        if report.context[index].source.is_some()
            && !within_budget(report, options, counter, deadline)?
        {
            report.context[index].source = None;
        }
    }
    recount(report, options);
    Ok(())
}

fn trim_definitions(report: &mut ReviewContextReport) -> bool {
    let side = report
        .changes
        .iter_mut()
        .flat_map(|change| [change.base.as_mut(), change.head.as_mut()])
        .flatten()
        .filter(|side| !side.definitions.is_empty())
        .max_by_key(|side| side.definitions.len());
    let Some(side) = side else {
        return false;
    };
    // Halving bounds projection work even for large declaration inventories.
    let keep = side.definitions.len() / 2;
    side.definitions_omitted += side.definitions.len() - keep;
    side.definitions.truncate(keep);
    true
}

fn trim_ranges(report: &mut ReviewContextReport) -> bool {
    let side = report
        .changes
        .iter_mut()
        .flat_map(|change| [change.base.as_mut(), change.head.as_mut()])
        .flatten()
        .filter(|side| !side.ranges.is_empty())
        .max_by_key(|side| side.ranges.len());
    let Some(side) = side else {
        return false;
    };
    let keep = side.ranges.len() / 2;
    side.ranges_omitted += side.ranges.len() - keep;
    side.ranges.truncate(keep);
    true
}

fn remove_largest_change(report: &mut ReviewContextReport) -> Result<bool> {
    let mut largest = None;
    for (index, change) in report.changes.iter().enumerate() {
        let cost = (serde_json::to_vec(change)?.len(), index);
        if largest.is_none_or(|previous| cost > previous) {
            largest = Some(cost);
        }
    }
    if let Some((_, index)) = largest {
        report.changes.remove(index);
        Ok(true)
    } else {
        Ok(false)
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
