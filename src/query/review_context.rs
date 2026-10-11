mod analysis;
mod brief;
mod changes;
#[cfg(test)]
mod tests;

use crate::config::Config;
use crate::metrics::tokens::TokenCounter;
use crate::model::{
    CallSymbolIdentity, ReviewBrief, ReviewBriefSelection, ReviewChangeBasis,
    ReviewContextCapability, ReviewContextRelation, ReviewContextReport, ReviewContextTotals,
    ReviewHunkRole, SCHEMA_VERSION,
};
use crate::report::Format;
use anyhow::{Result, ensure};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const ENTRY_LIMIT: usize = 100;
/// Rendered-token ceiling of the agent brief when `--budget` is absent; smaller briefs render in
/// full.
pub const REVIEW_BRIEF_TOKENS: usize = 8000;
/// Soft evidence (importers, usage and documentation lines) may always use this many tokens, and
/// otherwise at most half of the shown diff text.
const BRIEF_SOFT_TOKENS: usize = 400;

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
    /// Render the compact agent brief; requires `include_diff` and a table/Markdown format.
    pub brief: bool,
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
    let (mut context, mut relations) =
        analysis::analyze(&mut capture, &changes, &counter, graph_limits)?;
    check_deadline(capture.deadline)?;
    let mut totals = totals(&changes, &context, relations.len(), capture.total_changes);
    let brief = if options.brief {
        ensure!(
            options.include_diff && !options.context && !options.include_source,
            "review-context brief requires diffs and no reading-list selection"
        );
        Some(brief::prepare(
            &capture,
            &changes,
            &mut relations,
            &context,
        )?)
    } else {
        None
    };
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
        changes, relations, context, brief,
    };
    if options.brief {
        project_brief(&mut report, options, &counter, capture.deadline)?;
    } else {
        project(&mut report, options, &counter, capture.deadline)?;
    }
    let rendered = render(&report, options)?;
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
    report.relations.truncate(ENTRY_LIMIT);
    // Keep already-captured handles needed by retained direct evidence, even when
    // the source-selection order puts them beyond the output candidate cap.
    let endpoints: BTreeSet<_> = report
        .relations
        .iter()
        .filter(|relation| direct_relation(relation))
        .filter_map(|relation| relation.symbol.as_ref().map(|symbol| (relation, symbol)))
        .flat_map(|(relation, symbol)| {
            [&symbol.source, &symbol.target]
                .map(|endpoint| (relation.side.clone(), PathBuf::from(&endpoint.path)))
        })
        .collect();
    let mut index = 0;
    report.context.retain(|file| {
        let keep = index < ENTRY_LIMIT
            || (!options.include_source
                && !options.include_diff
                && endpoints.contains(&(file.side.clone(), file.path.clone())));
        index += 1;
        keep
    });
    let originals = report.clone();
    report.context.truncate(ENTRY_LIMIT);
    loop {
        if within_budget(report, options, counter, deadline)? {
            restore_change_identities(report, &originals.changes, options, counter, deadline)?;
            restore_direct_evidence(report, &originals, options, counter, deadline)?;
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

/// Optional brief evidence in admission order.
#[derive(Clone, Copy)]
enum BriefItem {
    Examples,
    SiteLines,
    ExplainedSites,
    Entry(usize),
    Details(usize),
    Hunk(usize),
    Importers(usize),
    Mention(usize),
    Usage(usize),
}

/// Greedy admissions tried after the longest fitting prefix.
const BRIEF_GREEDY_ATTEMPTS: usize = 64;

/// Rewrite site lines, every change entry heading, then shown hunks with deviating hunks first
/// and the rest by priority. A change's details follow its first hunk in that order, so detailed
/// entries never crowd out the diff. Rewrite examples only illustrate rules that are already
/// listed, so they follow the diff; details of changes without shown hunks, importers,
/// documentation lines and usage lines come last.
fn brief_items(report: &ReviewContextReport) -> Vec<BriefItem> {
    let Some(brief) = report.brief.as_ref() else {
        return Vec::new();
    };
    let path = |change: usize| {
        let change = &report.changes[change];
        change
            .head
            .as_ref()
            .or(change.base.as_ref())
            .map(|side| side.path.clone())
    };
    let mut hunks: Vec<_> = brief
        .hunks
        .iter()
        .enumerate()
        .filter(|(_, hunk)| hunk.role == ReviewHunkRole::Shown)
        .map(|(index, hunk)| (hunk.priority, path(hunk.change), index))
        .collect();
    hunks.sort();
    let mut items = vec![BriefItem::SiteLines];
    items.extend(
        brief
            .entry_changes
            .iter()
            .map(|&change| BriefItem::Entry(change)),
    );
    let mut detailed = BTreeSet::new();
    for (_, _, index) in hunks {
        items.push(BriefItem::Hunk(index));
        let change = brief.hunks[index].change;
        if detailed.insert(change) {
            items.push(BriefItem::Details(change));
        }
    }
    items.push(BriefItem::Examples);
    items.extend(
        brief
            .entry_changes
            .iter()
            .filter(|change| !detailed.contains(*change))
            .map(|&change| BriefItem::Details(change)),
    );
    items.extend(
        brief
            .entry_changes
            .iter()
            .map(|&change| BriefItem::Importers(change)),
    );
    items.extend((0..brief.mentions.len()).map(BriefItem::Mention));
    items.extend((0..brief.usage.len()).map(BriefItem::Usage));
    items
}

/// Change entry an item belongs to; it renders only once that entry is admitted.
fn item_entry(report: &ReviewContextReport, brief: &ReviewBrief, item: BriefItem) -> Option<usize> {
    match item {
        BriefItem::Details(change) | BriefItem::Importers(change) => Some(change),
        BriefItem::Hunk(index) => Some(brief.hunks[index].change),
        BriefItem::Usage(index) => {
            let identity = &brief.usage[index].change;
            report.changes.iter().position(|change| {
                [&change.base, &change.head].map(|side| side.as_ref().map(|side| side.path.clone()))
                    == *identity
            })
        }
        _ => None,
    }
}

fn select(selection: &mut ReviewBriefSelection, item: BriefItem, admit: bool) {
    let toggle = |set: &mut BTreeSet<usize>, value: usize| {
        if admit {
            set.insert(value);
        } else {
            set.remove(&value);
        }
    };
    match item {
        BriefItem::Examples => selection.examples = admit,
        BriefItem::SiteLines => selection.site_lines = admit,
        BriefItem::ExplainedSites => selection.explained_sites = admit,
        BriefItem::Entry(change) => toggle(&mut selection.entries, change),
        BriefItem::Details(change) => toggle(&mut selection.details, change),
        BriefItem::Hunk(index) => toggle(&mut selection.hunks, index),
        BriefItem::Importers(change) => toggle(&mut selection.importers, change),
        BriefItem::Mention(index) => toggle(&mut selection.mentions, index),
        BriefItem::Usage(index) => toggle(&mut selection.usage, index),
    }
}

/// Highest collapse level of the brief's mandatory sections.
const BRIEF_LEVELS: u8 = 3;

/// First collapse level that reduces the omitted-hunk lists to one line each.
const OMITTED_LINE_LEVEL: u8 = 2;

/// Brief projection renders mandatory summaries (rewrites, moves, renames, stale spellings and
/// the omission list), then admits the longest prefix of optional evidence that fits and tries
/// a bounded number of later items. Omitted rule-explained hunks are counted per rewrite
/// combination; below level 2 their sites are listed instead when the sites fit and the
/// admission with them keeps every other hunk that the admission without them keeps. Mandatory
/// sections collapse level by level until they fit; a level that fits but admits no shown hunk
/// (no change entry when no hunk is shown) gives way to the next level that admits one. Soft
/// evidence stays proportional to shown diff text.
fn project_brief(
    report: &mut ReviewContextReport,
    options: &ReviewContextOptions,
    counter: &TokenCounter,
    deadline: std::time::Instant,
) -> Result<()> {
    let items = brief_items(report);
    let mut fallback = None;
    for level in 0..=BRIEF_LEVELS {
        set_selection(report, level, &[]);
        if !brief_fits(report, options, counter, deadline)? {
            continue;
        }
        admit(report, options, counter, deadline, level, &[], &items)?;
        if level < OMITTED_LINE_LEVEL
            && report
                .brief
                .as_ref()
                .is_some_and(|brief| brief.hunks.iter().any(|hunk| hunk.explained))
        {
            let without_sites = report.brief.as_ref().map(|brief| brief.selection.clone());
            let kept = admitted_full_hunks(report);
            set_selection(report, level, &[BriefItem::ExplainedSites]);
            let grouped = brief_fits(report, options, counter, deadline)? && {
                let base = [BriefItem::ExplainedSites];
                admit(report, options, counter, deadline, level, &base, &items)?;
                kept.is_subset(&admitted_full_hunks(report))
            };
            if !grouped
                && let (Some(selection), Some(brief)) = (without_sites, report.brief.as_mut())
            {
                brief.selection = selection;
            }
        }
        let Some(brief) = report.brief.as_ref() else {
            return Ok(());
        };
        let shown = brief
            .hunks
            .iter()
            .any(|hunk| hunk.role == ReviewHunkRole::Shown);
        let admitted = if shown {
            brief
                .selection
                .hunks
                .iter()
                .any(|&index| brief.selection.entries.contains(&brief.hunks[index].change))
        } else {
            brief.entry_changes.is_empty() || !brief.selection.entries.is_empty()
        };
        if admitted {
            return Ok(());
        }
        fallback.get_or_insert_with(|| brief.selection.clone());
    }
    if let (Some(selection), Some(brief)) = (fallback, report.brief.as_mut()) {
        brief.selection = selection;
        return Ok(());
    }
    anyhow::bail!(
        "review-context output budget cannot hold the status envelope; increase --budget or --max-output-bytes"
    )
}

/// Admitted shown hunks that are not rule-explained, in admitted change entries.
fn admitted_full_hunks(report: &ReviewContextReport) -> BTreeSet<usize> {
    report.brief.as_ref().map_or_else(BTreeSet::new, |brief| {
        brief
            .selection
            .hunks
            .iter()
            .copied()
            .filter(|&index| {
                let hunk = &brief.hunks[index];
                !hunk.explained && brief.selection.entries.contains(&hunk.change)
            })
            .collect()
    })
}

fn set_selection(report: &mut ReviewContextReport, level: u8, admitted: &[BriefItem]) {
    if let Some(brief) = report.brief.as_mut() {
        brief.selection = ReviewBriefSelection {
            level,
            ..ReviewBriefSelection::default()
        };
        for item in admitted {
            select(&mut brief.selection, *item, true);
        }
    }
}

/// Longest fitting prefix of `items` after the `base` items at `level`, then bounded greedy
/// admissions.
fn admit(
    report: &mut ReviewContextReport,
    options: &ReviewContextOptions,
    counter: &TokenCounter,
    deadline: std::time::Instant,
    level: u8,
    base: &[BriefItem],
    items: &[BriefItem],
) -> Result<()> {
    let (mut low, mut high) = (0, items.len());
    while low < high {
        let middle = low + (high - low).div_ceil(2);
        set_selection(report, level, &[base, &items[..middle]].concat());
        if brief_fits(report, options, counter, deadline)? {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    set_selection(report, level, &[base, &items[..low]].concat());
    let mut attempts = 0;
    for item in items.iter().skip(low + 1) {
        if attempts == BRIEF_GREEDY_ATTEMPTS {
            break;
        }
        let Some(brief) = report.brief.as_ref() else {
            break;
        };
        let entry = item_entry(report, brief, *item);
        if entry.is_some_and(|change| !brief.selection.entries.contains(&change))
            || (matches!(item, BriefItem::Usage(_)) && entry.is_none())
        {
            continue;
        }
        attempts += 1;
        if let Some(brief) = report.brief.as_mut() {
            select(&mut brief.selection, *item, true);
        }
        if !brief_fits(report, options, counter, deadline)?
            && let Some(brief) = report.brief.as_mut()
        {
            select(&mut brief.selection, *item, false);
        }
    }
    Ok(())
}

fn brief_fits(
    report: &ReviewContextReport,
    options: &ReviewContextOptions,
    counter: &TokenCounter,
    deadline: std::time::Instant,
) -> Result<bool> {
    check_deadline(deadline)?;
    let parts = crate::report::review_context::render_brief_parts(
        report,
        options.format == Format::Markdown,
    );
    if parts.text.len() > options.byte_budget || counter.count(&parts.text) > options.token_budget {
        return Ok(false);
    }
    Ok(parts.soft.is_empty()
        || counter.count(&parts.soft) <= BRIEF_SOFT_TOKENS.max(counter.count(&parts.diff) / 2))
}

fn direct_relation(relation: &ReviewContextRelation) -> bool {
    relation.kind == "symbol-reference"
        && relation.change_basis == Some(ReviewChangeBasis::ChangedDefinition)
        && relation.symbol.is_some()
}

fn same_relation(left: &ReviewContextRelation, right: &ReviewContextRelation) -> bool {
    let same_type = match (&left.type_relation, &right.type_relation) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            left.source == right.source
                && left.target == right.target
                && left.relation == right.relation
                && left.resolver == right.resolver
        }
        _ => false,
    };
    left.side == right.side
        && left.kind == right.kind
        && left.edge.source == right.edge.source
        && left.edge.target == right.edge.target
        && left.edge.resolver == right.edge.resolver
        && left.change_basis == right.change_basis
        && left.symbol == right.symbol
        && same_type
}

fn endpoint_available(
    report: &ReviewContextReport,
    side: &str,
    endpoint: &CallSymbolIdentity,
) -> bool {
    let snapshot = if side == "base" {
        &report.comparison.base_tree
    } else {
        &report.comparison.head_tree
    };
    report.changes.iter().any(|change| {
        let changed = if side == "base" {
            &change.base
        } else {
            &change.head
        };
        changed.as_ref().is_some_and(|changed| {
            changed.path == Path::new(&endpoint.path)
                && changed.sha256.as_deref() == Some(endpoint.source_hash.as_str())
        })
    }) || report.context.iter().any(|file| {
        file.side == side
            && file.snapshot == *snapshot
            && file.path == Path::new(&endpoint.path)
            && file.sha256.as_deref() == Some(endpoint.source_hash.as_str())
    })
}

fn admit_relation(
    report: &mut ReviewContextReport,
    relation: &ReviewContextRelation,
    originals: &ReviewContextReport,
    options: &ReviewContextOptions,
    counter: &TokenCounter,
    deadline: std::time::Instant,
) -> Result<bool> {
    if report.relations.len() == ENTRY_LIMIT {
        return Ok(false);
    }
    let context_len = report.context.len();
    if !options.include_source
        && !options.include_diff
        && matches!(options.format, Format::Table | Format::Markdown)
        && direct_relation(relation)
        && let Some(symbol) = &relation.symbol
    {
        for endpoint in [&symbol.source, &symbol.target] {
            if endpoint_available(report, &relation.side, endpoint) {
                continue;
            }
            let file = originals.context.iter().find(|file| {
                let snapshot = if relation.side == "base" {
                    &report.comparison.base_tree
                } else {
                    &report.comparison.head_tree
                };
                file.side == relation.side
                    && file.snapshot == *snapshot
                    && file.path == Path::new(&endpoint.path)
                    && file.sha256.as_deref() == Some(endpoint.source_hash.as_str())
            });
            let Some(file) = file.filter(|_| report.context.len() < ENTRY_LIMIT) else {
                report.context.truncate(context_len);
                return Ok(false);
            };
            let mut metadata = file.clone();
            metadata.source = None;
            report.context.push(metadata);
        }
    }
    report.relations.push(relation.clone());
    if within_budget(report, options, counter, deadline)? {
        return Ok(true);
    }
    report.relations.pop();
    report.context.truncate(context_len);
    Ok(false)
}

fn restore_direct_evidence(
    report: &mut ReviewContextReport,
    originals: &ReviewContextReport,
    options: &ReviewContextOptions,
    counter: &TokenCounter,
    deadline: std::time::Instant,
) -> Result<()> {
    // Explicit body requests keep their established allocation and affordable bodies.
    if options.include_source || options.include_diff {
        return Ok(());
    }
    let direct: Vec<_> = originals
        .relations
        .iter()
        .filter(|relation| direct_relation(relation))
        .collect();
    let human = matches!(options.format, Format::Table | Format::Markdown);
    if direct.iter().all(|original| {
        report
            .relations
            .iter()
            .any(|relation| same_relation(relation, original))
            && (!human
                || original.symbol.as_ref().is_some_and(|symbol| {
                    [&symbol.source, &symbol.target]
                        .into_iter()
                        .all(|endpoint| endpoint_available(report, &original.side, endpoint))
                }))
    }) {
        return Ok(());
    }
    let previous_relations = std::mem::take(&mut report.relations);
    let previous_context = std::mem::take(&mut report.context);
    for relation in direct {
        admit_relation(report, relation, originals, options, counter, deadline)?;
    }
    // If no whole direct packet fits, preserve the existing type/import/file
    // fallback instead of trading useful evidence for an empty reservation.
    if report.relations.is_empty() {
        report.relations = previous_relations;
        report.context = previous_context;
    }
    recount(report, options);
    Ok(())
}

fn render(report: &ReviewContextReport, options: &ReviewContextOptions) -> Result<String> {
    if options.brief {
        Ok(crate::report::review_context::render_brief(
            report,
            options.format == Format::Markdown,
        ))
    } else {
        crate::report::review_context::render(report, options.format, options.pretty_json)
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
    let rendered = render(report, options)?;
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
    for relation in &originals.relations {
        if !report
            .relations
            .iter()
            .any(|retained| same_relation(retained, relation))
        {
            admit_relation(report, relation, originals, options, counter, deadline)?;
        }
    }
    for file in &originals.context {
        if report.context.len() == ENTRY_LIMIT {
            break;
        }
        if report
            .context
            .iter()
            .any(|retained| retained.side == file.side && retained.path == file.path)
        {
            continue;
        }
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
