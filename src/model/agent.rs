use super::{
    Deserialize, PathBuf, ProductionDuplication, ScanDiagnostics, Serialize, SymbolOutline,
    is_false, is_zero, is_zero_u64,
};

/// Stable machine-readable feature discovery for CLI consumers.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CapabilitiesReport {
    pub schema_version: String,
    pub version: String,
    /// Semantic operation used when no subcommand is present.
    pub default_operation: String,
    /// Literal CLI shape for the default operation.
    pub default_invocation: String,
    /// Literal subcommand names accepted after `reposcout`.
    pub commands: Vec<String>,
    pub output_formats: Vec<String>,
    pub symbol_query_formats: Vec<String>,
    pub symbol_kinds: Vec<String>,
    /// Profiles accepted by scan, explain, locate, and config commands.
    pub execution_profiles: Vec<String>,
    /// Separate long-running daemon analysis profiles.
    pub daemon_profiles: Vec<String>,
    pub first_class_languages: Vec<String>,
    pub recognized_languages: Vec<String>,
    /// Formats analyzed by source-health defaults without an opt-in.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub default_health_languages: Vec<String>,
    /// Recognized formats available through `health_includes` or all scope.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub optional_health_formats: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub health_scopes: Vec<String>,
    /// Repeatable path-glob flag applied after health scope and includes.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub health_exclude_flag: String,
    /// Explicit opt-in for minified and bundled duplication inputs.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub duplication_include_artifacts_flag: String,
    pub machine_interfaces: Vec<String>,
    pub error_formats: Vec<String>,
    pub max_graph_depth: usize,
    pub max_symbol_results: usize,
    /// Contract and hard payload limits for the change-focused projection.
    #[serde(default)]
    pub change_summary: ChangeSummaryCapability,
    /// Contract and hard payload limits for raw work-scope evidence.
    #[serde(default)]
    pub work_scope: WorkScopeCapability,
    /// Maximum Type-2 candidate seed pairs examined in one format pool.
    #[serde(default)]
    pub type2_max_seed_pairs_per_pool: u64,
    /// Maximum compact Type-2 matches buffered in one format pool.
    #[serde(default)]
    pub type2_max_matches_per_pool: usize,
    /// Maximum pairwise overlap checks during Type-2 suppression per pool.
    #[serde(default)]
    pub type2_max_overlap_checks_per_pool: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChangeSummaryCapability {
    pub flag: String,
    pub requires_one_of: Vec<String>,
    pub implies: Vec<String>,
    pub formats: Vec<String>,
    pub max_path_entries: usize,
    pub max_gap_entries: usize,
    pub max_validations: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkScopeCapability {
    pub strategy_version: u32,
    pub max_path_entries: usize,
    pub max_components: usize,
}

/// A file that is almost certainly not hand-authored code an agent should read.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkipCandidate {
    pub path: String,
    pub reason: String,
    pub tokens: usize,
}

/// Test-vs-source classification for the scanned tree.
///
/// The `untested_*` names are retained for JSON compatibility. They mean that
/// no matching test file or inline Rust test was found, not measured coverage.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TestPresence {
    pub test_files: usize,
    pub source_files: usize,
    pub untested_source_files: usize,
    pub untested_samples: Vec<String>,
}

/// A source file ranked by composite risk.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RiskEntry {
    pub path: String,
    /// Composite-risk formula version used for this entry.
    #[serde(default)]
    pub algorithm_version: u32,
    pub score: f64,
    pub sloc: usize,
    pub cyclomatic: u32,
    pub churn_commits: usize,
    /// No matching test file or inline Rust test was found. The field name is
    /// retained for JSON compatibility and does not represent code coverage.
    pub untested: bool,
    pub reasons: Vec<String>,
}

/// Quick machine-readable health verdict derived from aggregate signals.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Assessment {
    /// Whether `fits_context` is backed by an enabled token analyzer. Older
    /// reports deserialize this as false, so consumers never have to treat a
    /// legacy zero token count as evidence.
    #[serde(default)]
    pub fits_context_known: bool,
    pub fits_context: bool,
    pub token_budget: usize,
    /// `"low"` | `"medium"` | `"high"`
    pub cleanup_worth: String,
    /// False when one or more inputs used by the cleanup verdict were disabled
    /// or production duplication retained only partial evidence.
    #[serde(default)]
    pub cleanup_worth_complete: bool,
    /// Analyzer signals that were unavailable to this verdict.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unavailable_signals: Vec<String>,
    /// Production-source duplication used by the cleanup verdict. Absent when
    /// duplication analysis was disabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub production_duplication: Option<ProductionDuplication>,
    pub reasons: Vec<String>,
}

/// Compact, versioned evidence about the amount and shape of observed work.
///
/// `RepoScout` reports these measurements without deciding whether an agent
/// should work directly, delegate, or split the task.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkScope {
    pub strategy_version: u32,
    /// Ordered basis labels: `repository` or `diff`, followed by `focus` when
    /// explicit focus paths contributed.
    pub basis: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_scope: Option<String>,
    pub inventory: WorkScopeInventory,
    /// Production-source duplication already computed for the assessment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub production_duplication: Option<ProductionDuplication>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seeds: Option<WorkScopeSeeds>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<WorkScopeContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub impact: Option<WorkScopeImpact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structure: Option<WorkScopeStructure>,
    pub confidence: WorkScopeConfidence,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkScopeInventory {
    /// Files in the post-ignore discovery universe before an optional diff
    /// narrows the primary analysis.
    pub discovery_files: usize,
    /// Files analyzed in the primary repository or diff scope.
    pub primary_files: usize,
    pub source_files: usize,
    pub source_tokens: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkScopeSeeds {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<WorkScopeFocus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changes: Option<WorkScopeChanges>,
    /// Aggregate number of seed-path entries omitted by the work-scope bound.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub paths_omitted: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkScopeFocus {
    pub total: usize,
    pub resolved: usize,
    pub unresolved: usize,
    pub shown: usize,
    pub omitted: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unmatched_paths: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkScopeChanges {
    pub scope: String,
    pub total: usize,
    pub shown: usize,
    pub omitted: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkScopeContext {
    pub budget_tokens: usize,
    pub selected_files: usize,
    pub selected_tokens: usize,
    pub candidate_files: usize,
    pub outline_only_files: usize,
    pub outline_only_tokens: usize,
    pub outline_symbols: usize,
    pub outline_bytes: usize,
    pub outline_omitted_symbols: usize,
    pub omitted_files: usize,
    pub omitted_tokens: usize,
    pub skipped_files: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkScopeImpact {
    pub seed_files: usize,
    pub graph_eligible_seed_files: usize,
    pub graph_covered_seed_files: usize,
    pub direct_dependents: usize,
    pub transitive_dependents: usize,
    pub matching_tests: usize,
    /// False when the selected workflow did not run filename-based test
    /// matching for the seeds.
    pub matching_tests_known: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkScopeStructure {
    pub graph_files: usize,
    pub components: usize,
    pub largest_component_files: usize,
    pub shown: usize,
    pub omitted: usize,
    pub entries: Vec<WorkScopeComponent>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkScopeComponent {
    pub files: usize,
    pub seed_files: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub representative_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub representative_paths_omitted: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkScopeConfidence {
    pub primary: WorkScopeCoverage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planning_universe: Option<WorkScopeCoverage>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub graph_unresolved_imports: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub graph_parse_errors: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub graph_config_errors: usize,
    #[serde(default, skip_serializing_if = "is_false")]
    pub type2_analysis_partial: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unavailable_signals: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "the stable JSON contract records independent discovery completeness conditions without collapsing their semantics"
)]
pub struct WorkScopeCoverage {
    pub discovered_files: usize,
    pub analyzed_files: usize,
    pub unsupported_files: usize,
    pub unreadable_files: usize,
    pub walker_errors: usize,
    /// True when `analyzed_files` is intentionally limited to the selected
    /// diff rather than expected to equal the discovery universe.
    #[serde(default, skip_serializing_if = "is_false")]
    pub diff_scoped: bool,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub oversized_files: usize,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub oversized_bytes: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub files_omitted_by_limit: usize,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub bytes_omitted_by_limit: u64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub omitted_count_incomplete: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub duration_limit_reached: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub truncated: bool,
}

/// A bounded, deterministic set of files an agent should read first.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContextPlan {
    pub strategy_version: u32,
    /// Milliseconds spent in the incremental context-planning phase after the
    /// ordinary scoped scan. Change-aware plans include their full-tree facts
    /// and topology work in this measurement; `0` means under one millisecond.
    #[serde(default)]
    pub planning_ms: usize,
    pub budget_tokens: usize,
    pub selected_tokens: usize,
    pub candidate_files: usize,
    pub omitted_files: usize,
    /// Sum of source tokens across every candidate omitted by the context
    /// budget or file cap. Unlike `omitted`, this total is not detail-capped.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub omitted_tokens: usize,
    pub skipped_files: usize,
    /// Unique resolved focus/change files that seeded the plan, including
    /// deleted change paths.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub seed_files: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub graph_eligible_seed_files: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub graph_covered_seed_files: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub direct_dependents: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub transitive_dependents: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub matching_tests: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub focus: Vec<PathBuf>,
    /// Explicit focus paths that matched no analyzed file or directory after
    /// resolving against both the repository root and scan target.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unmatched_focus: Vec<PathBuf>,
    /// Diff scope that automatically seeded the plan (`since`, `staged`, or
    /// `working`). Absent for an ordinary focus/general context plan.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_scope: Option<String>,
    /// Changed paths inside the requested target, including deleted paths that
    /// have no current [`FileReport`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_files: Vec<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub graph_languages: Vec<String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub graph_unresolved_imports: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub graph_parse_errors: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub graph_config_errors: usize,
    /// Number of compact declarations retained across selected files.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub outline_symbols: usize,
    /// Sum of compact JSON bytes for retained symbol objects (array framing
    /// excluded).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub outline_bytes: usize,
    /// Relevant declarations dropped by per-file or aggregate outline bounds.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub outline_omitted_symbols: usize,
    /// True when a compact report omits the retained declaration objects while
    /// preserving their aggregate counts.
    #[serde(default, skip_serializing_if = "is_false")]
    pub outline_details_omitted: bool,
    /// Coverage of the separate full-tree fact universe used only by a
    /// diff-seeded plan. Primary report diagnostics remain diff-scoped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planning_diagnostics: Option<ScanDiagnostics>,
    pub files: Vec<ContextFile>,
    /// Explicit focus/change seeds whose source could not fit the requested
    /// budget but for which a bounded, body-free declaration outline remains
    /// useful. These entries do not contribute to `selected_tokens`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outline_only: Vec<ContextOutlineOnly>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub omitted: Vec<ContextOmission>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContextFile {
    pub path: PathBuf,
    pub tokens: usize,
    pub score: f64,
    pub reasons: Vec<String>,
    /// Structured evidence behind change-aware selection. The human-readable
    /// `reasons` remain for compatibility and compact rendering.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<ContextEvidence>,
    /// Bounded declaration headers for this selected first-class-language file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbols: Vec<SymbolOutline>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContextOutlineOnly {
    pub path: PathBuf,
    pub source_tokens: usize,
    pub score: f64,
    pub reason: String,
    pub reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<ContextEvidence>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbols: Vec<SymbolOutline>,
}

/// Machine-readable evidence for including one file in a context plan.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextEvidence {
    /// `changed`, `dependency`, `dependent`, `matching-test`, or `nearby`.
    pub role: String,
    /// `high` for direct syntax/config-backed evidence, `partial` for
    /// heuristic or transitive reachability.
    pub confidence: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolver: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContextOmission {
    pub path: PathBuf,
    pub tokens: usize,
    pub reason: String,
}
