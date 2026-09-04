//! Hard-bounded JSON projection for common agent scouting decisions.
//!
//! This module consumes only facts already present in [`ScanReport`]. It does
//! not run analyzers, rebuild graph topology, or change the underlying context
//! plan; its only responsibility is selecting a small, honest decision view.

use crate::model::{
    AnalyzerProfile, Assessment, ComplexitySummary, ContextEvidence, ContextFile,
    ContextOutlineOnly, DuplicateBlock, DuplicationProfile, FileRef, FunctionHotspot,
    HealthProfile, RiskEntry, ScanDiagnostics, ScanReport, SkipCandidate, SourceSummary,
    SymbolCounts, TestFramework, WorkScopeCoverage,
};
use anyhow::{Context as _, Result, bail};
use serde::Serialize;
use std::path::{Path, PathBuf};

pub(crate) const STRATEGY_VERSION: u32 = 1;
pub(crate) const MAX_BYTES: usize = 16 * 1024;
pub(crate) const MAX_SIGNAL_ENTRIES: usize = 3;
pub(crate) const MAX_DIRECT_CONTEXT_ENTRIES: usize = 5;
pub(crate) const MAX_EXPANSION_CONTEXT_ENTRIES: usize = 3;
pub(crate) const MAX_OUTLINE_ONLY_ENTRIES: usize = 3;
pub(crate) const MAX_UNMATCHED_FOCUS_ENTRIES: usize = 3;

#[derive(Serialize)]
struct AgentSummary<'a> {
    schema_version: &'a str,
    report_kind: &'static str,
    projection: ProjectionMetadata,
    root: &'a Path,
    target: &'a Path,
    generated_at: &'a str,
    encoding: &'a str,
    interpretation: Interpretation<'a>,
    coverage: Coverage,
    inventory: Inventory<'a>,
    assessment: &'a Assessment,
    signals: Signals<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<AgentContext<'a>>,
}

#[derive(Serialize)]
struct ProjectionMetadata {
    strategy_version: u32,
    max_bytes: usize,
    entries_omitted: usize,
    byte_limit_reached: bool,
}

#[derive(Serialize)]
struct Interpretation<'a> {
    profile: &'a str,
    config_mode: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    analyzers: Option<&'a AnalyzerProfile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diff_scope: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    health: Option<HealthPolicy<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    duplication: Option<DuplicationPolicy<'a>>,
}

#[derive(Serialize)]
struct HealthPolicy<'a> {
    scope: &'a str,
    included_formats: usize,
    excluded_paths: usize,
}

impl<'a> From<&'a HealthProfile> for HealthPolicy<'a> {
    fn from(profile: &'a HealthProfile) -> Self {
        Self {
            scope: &profile.scope,
            included_formats: profile.includes.len(),
            excluded_paths: profile.excludes.len(),
        }
    }
}

#[derive(Serialize)]
struct DuplicationPolicy<'a> {
    mode: &'a str,
    format_scope: &'a str,
    artifact_policy: &'a str,
    min_tokens: usize,
    min_lines: usize,
    min_similarity: f64,
}

impl<'a> From<&'a DuplicationProfile> for DuplicationPolicy<'a> {
    fn from(profile: &'a DuplicationProfile) -> Self {
        Self {
            mode: &profile.mode,
            format_scope: &profile.format_scope,
            artifact_policy: &profile.artifact_policy,
            min_tokens: profile.min_tokens,
            min_lines: profile.min_lines,
            min_similarity: profile.min_similarity,
        }
    }
}

#[derive(Serialize)]
struct Coverage {
    primary: WorkScopeCoverage,
    #[serde(skip_serializing_if = "Option::is_none")]
    planning_universe: Option<WorkScopeCoverage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    graph: Option<GraphCoverage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    type2_analysis_partial: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    churn_analysis_partial: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    churn_deltas_omitted: Option<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    unavailable_signals: Vec<String>,
}

#[derive(Serialize)]
struct GraphCoverage {
    unresolved_imports: usize,
    parse_errors: usize,
    config_errors: usize,
}

#[derive(Serialize)]
struct Inventory<'a> {
    recognized_files: usize,
    recognized_tokens: usize,
    source: &'a SourceSummary,
}

#[derive(Serialize)]
struct Signals<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    complexity: Option<&'a ComplexitySummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    symbols: Option<&'a SymbolCounts>,
    #[serde(skip_serializing_if = "Option::is_none")]
    duplication: Option<DuplicationSignals<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tests: Option<TestSignals<'a>>,
    top_source_files: BoundedList<&'a FileRef>,
    top_risks: BoundedList<&'a RiskEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    complexity_violations: Option<BoundedList<&'a FunctionHotspot>>,
    skip_candidates: BoundedList<&'a SkipCandidate>,
}

#[derive(Serialize)]
struct DuplicationSignals<'a> {
    exact_groups: usize,
    near_groups: usize,
    duplicated_lines: usize,
    duplicated_pct: f64,
    analyzed_lines: usize,
    duplicated_tokens: usize,
    analyzed_tokens: usize,
    duplicated_tokens_pct: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    production: Option<&'a crate::model::ProductionDuplication>,
    top_block_scope: &'static str,
    top_blocks: BoundedList<&'a DuplicateBlock>,
}

#[derive(Serialize)]
struct TestSignals<'a> {
    test_files: usize,
    frameworks: BoundedList<&'a TestFramework>,
}

#[derive(Serialize)]
struct AgentContext<'a> {
    strategy_version: u32,
    budget: ContextBudget,
    evidence: ContextEvidenceSummary,
    focus_files: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    change_scope: Option<&'a str>,
    unmatched_focus: BoundedList<&'a PathBuf>,
    direct_evidence: ContextBoundedList<ContextEntry<'a>>,
    expand_if_needed: ContextBoundedList<ContextEntry<'a>>,
    outline_only: ContextBoundedList<OutlineEntry<'a>>,
}

#[derive(Serialize)]
struct ContextEvidenceSummary {
    seed_files: usize,
    graph_eligible_seed_files: usize,
    graph_covered_seed_files: usize,
    direct_dependents: usize,
    transitive_dependents: usize,
    matching_tests: usize,
}

#[derive(Serialize)]
struct ContextBudget {
    budget_tokens: usize,
    selected_files: usize,
    selected_tokens: usize,
    candidate_files: usize,
    plan_omitted_files: usize,
    plan_omitted_tokens: usize,
    skipped_files: usize,
    outline_only_files: usize,
    outline_only_tokens: usize,
    outline_symbols: usize,
    outline_omitted_symbols: usize,
    truncated: bool,
}

#[derive(Serialize)]
struct ContextEntry<'a> {
    path: &'a Path,
    tokens: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    evidence: Vec<&'a ContextEvidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'a str>,
}

#[derive(Serialize)]
struct OutlineEntry<'a> {
    path: &'a Path,
    source_tokens: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    evidence: Vec<&'a ContextEvidence>,
    reason: &'a str,
}

#[derive(Serialize)]
struct BoundedList<T> {
    available: usize,
    shown: usize,
    omitted: usize,
    entries: Vec<T>,
}

#[derive(Serialize)]
struct ContextBoundedList<T> {
    available: usize,
    available_tokens: usize,
    shown: usize,
    shown_tokens: usize,
    omitted: usize,
    omitted_tokens: usize,
    entries: Vec<T>,
}

trait TokenCost {
    fn token_cost(&self) -> usize;
}

impl TokenCost for ContextEntry<'_> {
    fn token_cost(&self) -> usize {
        self.tokens
    }
}

impl TokenCost for OutlineEntry<'_> {
    fn token_cost(&self) -> usize {
        self.source_tokens
    }
}

impl<T> BoundedList<T> {
    fn new(mut entries: Vec<T>, maximum: usize) -> Self {
        let available = entries.len();
        entries.truncate(maximum);
        let shown = entries.len();
        Self {
            available,
            shown,
            omitted: available.saturating_sub(shown),
            entries,
        }
    }

    fn remove_last(&mut self) -> bool {
        if self.entries.pop().is_none() {
            return false;
        }
        self.shown = self.shown.saturating_sub(1);
        self.omitted = self.omitted.saturating_add(1);
        true
    }
}

impl<T: TokenCost> ContextBoundedList<T> {
    fn new(mut entries: Vec<T>, maximum: usize) -> Self {
        let available = entries.len();
        let available_tokens = sum_token_cost(&entries);
        entries.truncate(maximum);
        let shown = entries.len();
        let shown_tokens = sum_token_cost(&entries);
        Self {
            available,
            available_tokens,
            shown,
            shown_tokens,
            omitted: available.saturating_sub(shown),
            omitted_tokens: available_tokens.saturating_sub(shown_tokens),
            entries,
        }
    }

    fn remove_last(&mut self) -> bool {
        let Some(removed) = self.entries.pop() else {
            return false;
        };
        let removed_tokens = removed.token_cost();
        self.shown = self.shown.saturating_sub(1);
        self.shown_tokens = self.shown_tokens.saturating_sub(removed_tokens);
        self.omitted = self.omitted.saturating_add(1);
        self.omitted_tokens = self.omitted_tokens.saturating_add(removed_tokens);
        true
    }
}

fn sum_token_cost<T: TokenCost>(entries: &[T]) -> usize {
    entries
        .iter()
        .fold(0, |total, entry| total.saturating_add(entry.token_cost()))
}

impl<'a> AgentSummary<'a> {
    fn new(report: &'a ScanReport) -> Self {
        let profile = report.analysis_profile.as_ref();
        let analyzers = profile.map(|profile| &profile.analyzers);
        let health = profile.and_then(|profile| profile.health.as_ref());
        let duplication = profile.and_then(|profile| profile.duplication.as_ref());
        let graph_requested = report.context.is_some()
            || report.graph.is_some()
            || report.impact.is_some()
            || report.change_summary.is_some();
        let coverage = build_coverage(report, analyzers, graph_requested);
        let production = report.summary.assessment.production_duplication.as_ref();
        let (top_block_scope, top_blocks) = production.map_or_else(
            || {
                (
                    "configured-health",
                    report.summary.top_duplicates.as_slice(),
                )
            },
            |_| {
                (
                    "production-source",
                    report.summary.top_production_duplicates.as_slice(),
                )
            },
        );
        let tests = report
            .summary
            .test_presence
            .as_ref()
            .map(|tests| TestSignals {
                test_files: tests.test_files,
                frameworks: bounded_refs(&tests.frameworks, MAX_SIGNAL_ENTRIES),
            });

        Self {
            schema_version: &report.schema_version,
            report_kind: "agent-summary",
            projection: ProjectionMetadata {
                strategy_version: STRATEGY_VERSION,
                max_bytes: MAX_BYTES,
                entries_omitted: 0,
                byte_limit_reached: false,
            },
            root: &report.root,
            target: &report.target,
            generated_at: &report.generated_at,
            encoding: &report.encoding,
            interpretation: Interpretation {
                profile: &report.execution.profile,
                config_mode: &report.execution.config_mode,
                analyzers: profile.map(|profile| &profile.analyzers),
                diff_scope: profile.map(|profile| profile.diff_scope.as_str()),
                health: health.map(HealthPolicy::from),
                duplication: duplication.map(DuplicationPolicy::from),
            },
            coverage,
            inventory: Inventory {
                recognized_files: report.summary.files,
                recognized_tokens: report.summary.tokens,
                source: &report.summary.source,
            },
            assessment: &report.summary.assessment,
            signals: Signals {
                complexity: analyzers
                    .is_some_and(|analyzers| analyzers.complexity)
                    .then_some(&report.summary.complexity),
                symbols: analyzers
                    .is_some_and(|analyzers| analyzers.complexity || analyzers.imports)
                    .then_some(&report.summary.symbols),
                duplication: analyzers
                    .is_some_and(|analyzers| analyzers.duplication)
                    .then(|| DuplicationSignals {
                        exact_groups: report.summary.duplication.exact_groups,
                        near_groups: report.summary.duplication.near_groups,
                        duplicated_lines: report.summary.duplication.duplicated_lines,
                        duplicated_pct: report.summary.duplication.duplicated_pct,
                        analyzed_lines: report.summary.duplication.analyzed_lines,
                        duplicated_tokens: report.summary.duplication.duplicated_tokens,
                        analyzed_tokens: report.summary.duplication.analyzed_tokens,
                        duplicated_tokens_pct: report.summary.duplication.duplicated_tokens_pct,
                        production,
                        top_block_scope,
                        top_blocks: bounded_refs(top_blocks, MAX_SIGNAL_ENTRIES),
                    }),
                tests,
                top_source_files: bounded_refs(
                    &report.summary.top_source_token_files,
                    MAX_SIGNAL_ENTRIES,
                ),
                top_risks: bounded_refs(&report.summary.top_risks, MAX_SIGNAL_ENTRIES),
                complexity_violations: analyzers
                    .is_some_and(|analyzers| analyzers.complexity)
                    .then(|| {
                        bounded_refs(&report.summary.complexity_violations, MAX_SIGNAL_ENTRIES)
                    }),
                skip_candidates: bounded_refs(&report.summary.skip_candidates, MAX_SIGNAL_ENTRIES),
            },
            context: report.context.as_ref().map(build_context),
        }
    }

    fn refresh_projection_omissions(&mut self) {
        self.projection.entries_omitted = self.signals.omitted_entries()
            + self
                .context
                .as_ref()
                .map_or(0, AgentContext::omitted_entries);
    }

    fn trim_one(&mut self) -> bool {
        self.signals
            .tests
            .as_mut()
            .is_some_and(|tests| tests.frameworks.remove_last())
            || self.signals.skip_candidates.remove_last()
            || self
                .signals
                .duplication
                .as_mut()
                .is_some_and(|duplication| duplication.top_blocks.remove_last())
            || self
                .context
                .as_mut()
                .is_some_and(|context| context.expand_if_needed.remove_last())
            || self.signals.top_source_files.remove_last()
            || self.signals.top_risks.remove_last()
            || self
                .signals
                .complexity_violations
                .as_mut()
                .is_some_and(BoundedList::remove_last)
            || self
                .context
                .as_mut()
                .is_some_and(|context| context.outline_only.remove_last())
            || self
                .context
                .as_mut()
                .is_some_and(|context| context.unmatched_focus.remove_last())
            || self
                .context
                .as_mut()
                .is_some_and(|context| context.direct_evidence.remove_last())
    }
}

fn build_coverage(
    report: &ScanReport,
    analyzers: Option<&AnalyzerProfile>,
    graph_requested: bool,
) -> Coverage {
    report.work_scope.as_ref().map_or_else(
        || coverage_from_diagnostics(&report.diagnostics),
        |scope| Coverage {
            primary: scope.confidence.primary.clone(),
            planning_universe: scope.confidence.planning_universe.clone(),
            graph: graph_requested.then_some(GraphCoverage {
                unresolved_imports: scope.confidence.graph_unresolved_imports,
                parse_errors: scope.confidence.graph_parse_errors,
                config_errors: scope.confidence.graph_config_errors,
            }),
            type2_analysis_partial: analyzers
                .is_some_and(|analyzers| analyzers.duplication)
                .then_some(scope.confidence.type2_analysis_partial),
            churn_analysis_partial: analyzers
                .is_some_and(|analyzers| analyzers.churn)
                .then_some(report.diagnostics.churn_analysis_partial),
            churn_deltas_omitted: analyzers
                .is_some_and(|analyzers| analyzers.churn)
                .then_some(report.diagnostics.churn_deltas_omitted),
            unavailable_signals: scope.confidence.unavailable_signals.clone(),
        },
    )
}

impl Signals<'_> {
    fn omitted_entries(&self) -> usize {
        self.tests
            .as_ref()
            .map_or(0, |tests| tests.frameworks.omitted)
            + self.top_source_files.omitted
            + self.top_risks.omitted
            + self
                .complexity_violations
                .as_ref()
                .map_or(0, |violations| violations.omitted)
            + self.skip_candidates.omitted
            + self
                .duplication
                .as_ref()
                .map_or(0, |duplication| duplication.top_blocks.omitted)
    }
}

impl AgentContext<'_> {
    fn omitted_entries(&self) -> usize {
        self.unmatched_focus.omitted
            + self.direct_evidence.omitted
            + self.expand_if_needed.omitted
            + self.outline_only.omitted
    }
}

fn bounded_refs<T>(entries: &[T], maximum: usize) -> BoundedList<&T> {
    BoundedList::new(entries.iter().collect(), maximum)
}

fn build_context(context: &crate::model::ContextPlan) -> AgentContext<'_> {
    let (direct, expansion): (Vec<_>, Vec<_>) = context
        .files
        .iter()
        .map(context_entry)
        .partition(|entry| entry.evidence.iter().any(|evidence| is_direct(evidence)));
    let outline_only = context
        .outline_only
        .iter()
        .map(outline_entry)
        .collect::<Vec<_>>();

    AgentContext {
        strategy_version: context.strategy_version,
        budget: ContextBudget {
            budget_tokens: context.budget_tokens,
            selected_files: context.files.len(),
            selected_tokens: context.selected_tokens,
            candidate_files: context.candidate_files,
            plan_omitted_files: context.omitted_files,
            plan_omitted_tokens: context.omitted_tokens,
            skipped_files: context.skipped_files,
            outline_only_files: context.outline_only.len(),
            outline_only_tokens: context
                .outline_only
                .iter()
                .map(|file| file.source_tokens)
                .sum(),
            outline_symbols: context.outline_symbols,
            outline_omitted_symbols: context.outline_omitted_symbols,
            truncated: context.omitted_files > 0 || context.outline_omitted_symbols > 0,
        },
        evidence: ContextEvidenceSummary {
            seed_files: context.seed_files,
            graph_eligible_seed_files: context.graph_eligible_seed_files,
            graph_covered_seed_files: context.graph_covered_seed_files,
            direct_dependents: context.direct_dependents,
            transitive_dependents: context.transitive_dependents,
            matching_tests: context.matching_tests,
        },
        focus_files: context.focus.len(),
        change_scope: context.change_scope.as_deref(),
        unmatched_focus: bounded_refs(&context.unmatched_focus, MAX_UNMATCHED_FOCUS_ENTRIES),
        direct_evidence: ContextBoundedList::new(direct, MAX_DIRECT_CONTEXT_ENTRIES),
        expand_if_needed: ContextBoundedList::new(expansion, MAX_EXPANSION_CONTEXT_ENTRIES),
        outline_only: ContextBoundedList::new(outline_only, MAX_OUTLINE_ONLY_ENTRIES),
    }
}

fn context_entry(file: &ContextFile) -> ContextEntry<'_> {
    let evidence = file.evidence.iter().collect::<Vec<_>>();
    ContextEntry {
        path: &file.path,
        tokens: file.tokens,
        reason: evidence
            .is_empty()
            .then(|| file.reasons.first())
            .flatten()
            .map(String::as_str),
        evidence,
    }
}

fn outline_entry(file: &ContextOutlineOnly) -> OutlineEntry<'_> {
    OutlineEntry {
        path: &file.path,
        source_tokens: file.source_tokens,
        evidence: file.evidence.iter().collect(),
        reason: &file.reason,
    }
}

fn is_direct(evidence: &ContextEvidence) -> bool {
    match evidence.role.as_str() {
        "focus" | "changed" | "matching-test" => true,
        "dependency" | "dependent" => evidence.distance.is_none_or(|distance| distance <= 1),
        _ => false,
    }
}

fn coverage_from_diagnostics(diagnostics: &ScanDiagnostics) -> Coverage {
    Coverage {
        primary: WorkScopeCoverage {
            discovered_files: diagnostics.discovered_files,
            analyzed_files: diagnostics.analyzed_files,
            unsupported_files: diagnostics.unsupported_files,
            unreadable_files: diagnostics.unreadable_files,
            walker_errors: diagnostics.walker_errors,
            oversized_files: diagnostics.oversized_files,
            oversized_bytes: diagnostics.oversized_bytes,
            files_omitted_by_limit: diagnostics.files_omitted_by_limit,
            bytes_omitted_by_limit: diagnostics.bytes_omitted_by_limit,
            omitted_count_incomplete: diagnostics.files_omitted_count_incomplete,
            duration_limit_reached: diagnostics.duration_limit_reached,
            truncated: diagnostics.scan_truncated,
            ..WorkScopeCoverage::default()
        },
        planning_universe: None,
        graph: None,
        type2_analysis_partial: None,
        churn_analysis_partial: None,
        churn_deltas_omitted: None,
        unavailable_signals: Vec::new(),
    }
}

/// Render one complete newline-terminated compact JSON document within
/// [`MAX_BYTES`].
///
/// Fixed list caps are applied first. If repository-controlled path lengths
/// still exceed the byte ceiling, complete optional entries are removed in a
/// deterministic low-value-first order and all omission counters are updated.
pub(super) fn json(report: &ScanReport, pretty: bool) -> Result<String> {
    if pretty {
        bail!("agent-summary output is compact JSON and cannot be combined with --pretty");
    }

    let mut projection = AgentSummary::new(report);
    loop {
        projection.refresh_projection_omissions();
        let rendered =
            serde_json::to_string(&projection).context("failed to render agent-summary JSON")?;
        if rendered.len() < MAX_BYTES {
            return Ok(format!("{rendered}\n"));
        }

        if !projection.projection.byte_limit_reached {
            projection.projection.byte_limit_reached = true;
            continue;
        }
        if !projection.trim_one() {
            bail!("agent-summary required fields exceed the hard {MAX_BYTES}-byte JSON limit");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        AnalyzerProfile, ContextPlan, Duplication, ExecutionMetadata, FindingCatalog, ScanProfile,
        Summary, WorkScope,
    };

    fn report(summary: Summary) -> ScanReport {
        ScanReport {
            schema_version: crate::model::SCHEMA_VERSION.to_string(),
            root: PathBuf::from("/repo"),
            target: PathBuf::from("/repo"),
            generated_at: "2026-09-04T00:00:00Z".to_string(),
            encoding: "o200k_base".to_string(),
            analysis_profile: None,
            execution: ExecutionMetadata {
                profile: "agent".to_string(),
                config_mode: "defaults".to_string(),
                ..ExecutionMetadata::default()
            },
            finding_catalog: FindingCatalog::default(),
            summary,
            work_scope: Some(WorkScope::default()),
            files: Vec::new(),
            duplicates: Duplication::default(),
            directories: Vec::new(),
            baseline: None,
            graph: None,
            context: None,
            diagnostics: ScanDiagnostics::default(),
            impact: None,
            change_summary: None,
            review: None,
        }
    }

    #[test]
    fn hard_byte_limit_removes_complete_entries_and_keeps_valid_json() {
        let long = "🚀\n\"\\x".repeat(1_500);
        let summary = Summary {
            top_source_token_files: (0..10)
                .map(|index| FileRef {
                    path: PathBuf::from(format!("{long}-{index}.rs")),
                    tokens: index,
                })
                .collect(),
            top_risks: (0..10)
                .map(|index| RiskEntry {
                    path: format!("{long}-risk-{index}.rs"),
                    ..RiskEntry::default()
                })
                .collect(),
            ..Summary::default()
        };

        let rendered = json(&report(summary), false).expect("bounded projection");
        let value: serde_json::Value = serde_json::from_str(&rendered).expect("valid JSON");

        assert!(rendered.len() <= MAX_BYTES);
        assert!(rendered.ends_with('\n'));
        assert_eq!(value["projection"]["byte_limit_reached"], true);
        assert!(value["projection"]["entries_omitted"].as_u64().unwrap() > 0);
    }

    #[test]
    fn context_projection_keeps_plan_and_projection_omissions_separate() {
        let mut scan = report(Summary::default());
        scan.context = Some(ContextPlan {
            strategy_version: 3,
            budget_tokens: 1_000,
            selected_tokens: 28,
            candidate_files: 9,
            omitted_files: 4,
            omitted_tokens: 400,
            seed_files: 2,
            graph_eligible_seed_files: 1,
            graph_covered_seed_files: 0,
            direct_dependents: 3,
            transitive_dependents: 4,
            matching_tests: 2,
            focus: vec![PathBuf::from("src/lib.rs")],
            files: (1..=7)
                .map(|tokens| ContextFile {
                    path: PathBuf::from(format!("src/direct-{tokens}.rs")),
                    tokens,
                    evidence: vec![ContextEvidence {
                        role: "focus".to_string(),
                        confidence: "high".to_string(),
                        distance: Some(0),
                        resolver: None,
                    }],
                    ..ContextFile::default()
                })
                .collect(),
            ..ContextPlan::default()
        });

        let value: serde_json::Value = serde_json::from_str(&json(&scan, false).unwrap()).unwrap();

        assert_eq!(value["context"]["budget"]["plan_omitted_files"], 4);
        assert_eq!(value["context"]["evidence"]["seed_files"], 2);
        assert_eq!(value["context"]["evidence"]["graph_eligible_seed_files"], 1);
        assert_eq!(value["context"]["evidence"]["graph_covered_seed_files"], 0);
        assert_eq!(value["context"]["evidence"]["direct_dependents"], 3);
        assert_eq!(value["context"]["evidence"]["transitive_dependents"], 4);
        assert_eq!(value["context"]["evidence"]["matching_tests"], 2);
        assert_eq!(value["context"]["direct_evidence"]["available"], 7);
        assert_eq!(value["context"]["direct_evidence"]["shown"], 5);
        assert_eq!(value["context"]["direct_evidence"]["omitted"], 2);
        assert_eq!(value["context"]["direct_evidence"]["available_tokens"], 28);
        assert_eq!(value["context"]["direct_evidence"]["shown_tokens"], 15);
        assert_eq!(value["context"]["direct_evidence"]["omitted_tokens"], 13);
        assert_eq!(value["context"]["expand_if_needed"]["shown"], 0);
        assert_eq!(value["projection"]["entries_omitted"], 2);
    }

    #[test]
    fn analyzer_specific_partiality_is_present_only_when_the_analyzer_ran() {
        let mut scan = report(Summary::default());
        scan.analysis_profile = Some(ScanProfile {
            analyzers: AnalyzerProfile {
                duplication: true,
                churn: true,
                ..AnalyzerProfile::default()
            },
            ..ScanProfile::default()
        });
        scan.work_scope
            .as_mut()
            .unwrap()
            .confidence
            .type2_analysis_partial = true;
        scan.diagnostics.churn_analysis_partial = true;
        scan.diagnostics.churn_deltas_omitted = 17;

        let value: serde_json::Value = serde_json::from_str(&json(&scan, false).unwrap()).unwrap();

        assert_eq!(value["coverage"]["type2_analysis_partial"], true);
        assert_eq!(value["coverage"]["churn_analysis_partial"], true);
        assert_eq!(value["coverage"]["churn_deltas_omitted"], 17);
    }
}
