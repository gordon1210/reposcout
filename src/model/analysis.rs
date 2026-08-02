use super::{
    Deserialize, ExecutionMetadata, FileReport, FindingRecord, PathBuf, Serialize, is_false,
    is_zero, is_zero_u64,
};

/// A node in the import dependency graph (single file with fan metrics).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GraphNode {
    pub path: String,
    pub fan_in: usize,
    pub fan_out: usize,
}

/// One file in the deterministic, machine-readable graph projection.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GraphFile {
    pub path: String,
    pub language: String,
    pub fan_in: usize,
    pub fan_out: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependents: Vec<String>,
    /// Distance from the nearest focus seed; absent for an unfiltered graph.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus_distance: Option<usize>,
    /// Strongest explicit type relationship declared in this file. This is
    /// separate from file import fan-in/fan-out so consumers never have to
    /// infer inheritance from an ordinary dependency edge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol_reach: Option<GraphSymbolReach>,
}

/// A directed internal import edge (`source` imports `target`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    /// `relative`, `python-relative`, `python-absolute`, `python-src-root`,
    /// `tsconfig-paths`, `tsconfig-base-url`, `package-imports`,
    /// `package-exports`, `package-subpath`, `package-entrypoint`,
    /// `package-index`, `composer-psr-4`, `composer-psr-0`, `php-include`,
    /// `php-namespace-heuristic`, `rust-mod`, `rust-path`, `rust-use`,
    /// `rust-workspace`, `go-module`, `go-relative`, or `heuristic-alias`.
    pub resolver: String,
}

/// One declared type participating in a statically proven type relationship.
///
/// Only symbols attached to at least one resolved `extends`, `implements`, or
/// `embeds` edge are retained, keeping the on-demand graph payload bounded to
/// useful architectural evidence rather than becoming a full symbol index.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GraphSymbol {
    pub id: String,
    pub name: String,
    pub qualified_name: String,
    /// `class`, `interface`, `trait`, `struct`, or `type`.
    pub kind: String,
    pub path: String,
    pub language: String,
    pub line: usize,
    pub fan_in: usize,
    pub fan_out: usize,
}

/// A directed, syntax-proven relationship from a derived/implementing symbol
/// to its resolved base contract or type.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GraphSymbolEdge {
    pub source: String,
    pub target: String,
    /// `extends`, `implements`, or `embeds`.
    pub relation: String,
    /// `qualified`, `same-file`, `same-scope`, or `unique-name`.
    pub resolver: String,
}

/// Compact file-level projection of the most structurally connected symbol.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GraphSymbolReach {
    pub symbol_id: String,
    pub name: String,
    pub kind: String,
    pub fan_in: usize,
    pub fan_out: usize,
    /// Dominant incoming relation, or the dominant outgoing relation when the
    /// symbol has no resolved incoming relationships.
    pub relation: String,
}

/// Import and explicit type-relationship graph for every first-class language
/// (opt-in via `--graph`).
///
/// Language scope: Rust, Python, JavaScript, TypeScript, JSX, TSX, Go, and PHP.
/// Resolution remains deliberately heuristic and records the provenance of
/// every internal edge.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DepGraph {
    /// Languages whose imports were resolved (e.g. "Rust", "Python", "Go").
    pub languages: Vec<String>,
    pub nodes: usize,
    pub edges: usize,
    /// Stable path-sorted adjacency for the selected graph projection.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<GraphFile>,
    /// Stable source/target-sorted internal import edges.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edge_list: Vec<GraphEdge>,
    /// Types participating in explicit cross-symbol relationships.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbols: Vec<GraphSymbol>,
    /// Explicit `extends`, `implements`, and `embeds` relationships.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbol_edges: Vec<GraphSymbolEdge>,
    /// Explicit type references that were syntactically present but could not
    /// be resolved to one unambiguous symbol in the scanned repository.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub unresolved_symbol_relations: usize,
    /// Normalized file or directory focus paths used for a bounded query.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub focus: Vec<String>,
    /// Focus paths that matched no supported graph file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unmatched_focus: Vec<String>,
    /// `all` for a complete graph, otherwise `dependencies`, `dependents`, or
    /// `both`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub direction: String,
    /// Maximum traversal hops for a focused query.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<usize>,
    /// Strongly-connected components of size >= 2 (import cycles), each listed
    /// as the sorted file paths involved. Self-imports appear as 1-element vecs.
    pub cycles: Vec<Vec<String>>,
    /// Files that nothing imports and that are not obvious entrypoints or tests
    /// (candidates for dead code).
    pub orphans: Vec<String>,
    /// Highest fan-in (most depended-upon) files, top 10.
    pub top_depended: Vec<GraphNode>,
    /// Highest fan-out (files importing the most internal modules), top 10.
    pub most_dependent: Vec<GraphNode>,
    /// Count of local import specifiers that did not resolve to a scanned file.
    pub unresolved_imports: usize,
    /// Error or missing nodes found in supported-language syntax trees.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub parse_errors: usize,
    /// Invalid or ambiguous resolver configuration encountered while loading
    /// tsconfig/jsconfig, package metadata, Composer metadata, Cargo.toml, or go.mod.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub config_errors: usize,
    /// Configuration files that contributed language resolver settings or local
    /// package/module metadata.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub config_files: Vec<String>,
}

/// Counts that explain how complete a scan was.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "the stable JSON contract records independent scan truncation conditions without collapsing their semantics"
)]
pub struct ScanDiagnostics {
    /// Files discovered after walker filters such as ignore rules.
    pub discovered_files: usize,
    /// Files for which reposcout produced a report.
    pub analyzed_files: usize,
    /// Files discovered but not recognized as a supported language.
    pub unsupported_files: usize,
    /// Bounded repository-relative examples of unsupported files.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unsupported_samples: Vec<String>,
    /// Files that could not be read as UTF-8 text or inspected.
    pub unreadable_files: usize,
    /// Traversal errors skipped by the filesystem walker.
    pub walker_errors: usize,
    /// Recognized worktree files skipped because they exceeded the per-file limit.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub oversized_files: usize,
    /// Aggregate bytes in recognized worktree files skipped as oversized.
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub oversized_bytes: u64,
    /// Known files omitted after a file-count, aggregate-byte, or duration limit.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub files_omitted_by_limit: usize,
    /// Traversal stopped before the exact omitted-file count could be established.
    #[serde(default, skip_serializing_if = "is_false")]
    pub files_omitted_count_incomplete: bool,
    /// Aggregate known bytes omitted by resource limits.
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub bytes_omitted_by_limit: u64,
    /// At least one input or runtime limit made the scan partial.
    #[serde(default, skip_serializing_if = "is_false")]
    pub scan_truncated: bool,
    /// The cooperative wall-clock scan budget elapsed.
    #[serde(default, skip_serializing_if = "is_false")]
    pub duration_limit_reached: bool,
    /// The Type-2 detector stopped at a safety limit, so near-duplicate
    /// findings are useful but incomplete.
    #[serde(default, skip_serializing_if = "is_false")]
    pub type2_analysis_partial: bool,
    /// Format pools whose Type-2 candidate search stopped early.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub type2_pools_truncated: usize,
    /// Repetitive fingerprint buckets omitted by rare-first admission control.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub type2_candidate_buckets_skipped: usize,
    /// Fingerprint buckets for which only part of the planned seed work ran.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub type2_candidate_buckets_partially_selected: usize,
    /// Candidate seed pairs not examined after applying Type-2 safety limits.
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub type2_seed_pairs_skipped: u64,
    /// At least one format pool reached the retained-match memory limit.
    #[serde(default, skip_serializing_if = "is_false")]
    pub type2_match_limit_reached: bool,
    /// At least one format pool reached the overlap-comparison work limit.
    #[serde(default, skip_serializing_if = "is_false")]
    pub type2_suppression_limit_reached: bool,
    /// Buffered Type-2 matches omitted when bounded overlap suppression
    /// stopped early.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub type2_matches_skipped_during_suppression: usize,
    /// Churn collection stopped early because a delta, output-byte, path, or
    /// cache limit was reached.
    #[serde(default, skip_serializing_if = "is_false")]
    pub churn_analysis_partial: bool,
    /// Tree deltas omitted after applying churn safety limits.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub churn_deltas_omitted: usize,
}

/// The likely internal blast radius of a diff-scoped change set.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImpactAnalysis {
    /// Paths selected by both the diff scope and scan target.
    pub changed_files: Vec<String>,
    /// Changed paths supported by the current first-class-language graph.
    pub graph_changed_files: Vec<String>,
    /// Unchanged files that directly import a changed graph file.
    pub direct_dependents: Vec<String>,
    /// Additional unchanged files that transitively import a changed graph file.
    pub transitive_dependents: Vec<String>,
    /// Unresolved local import specifiers in the full topology used for impact.
    pub unresolved_imports: usize,
    /// Error or missing nodes found in supported-language syntax trees.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub parse_errors: usize,
    /// Invalid or ambiguous resolver configuration in the full topology.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub config_errors: usize,
    /// High, partial, or none based on graph coverage.
    pub confidence: String,
}

/// Bounded, change-focused decision report assembled from shared scan,
/// context, and graph facts.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChangeSummary {
    pub strategy_version: u32,
    pub scope: String,
    pub executive: ChangeExecutive,
    pub changed: ChangeFileList,
    pub reading_order: Vec<ChangeReadingFile>,
    pub reading_order_total: usize,
    pub reading_order_shown: usize,
    pub reading_order_omitted: usize,
    pub impact: ChangeImpactSummary,
    pub tests: ChangeTestList,
    pub coverage: ChangeCoverage,
    pub validations: Vec<ChangeValidation>,
    pub validations_omitted: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChangeExecutive {
    pub changed_files: usize,
    pub graph_eligible_changed_files: usize,
    pub known_direct_dependents: usize,
    pub known_transitive_dependents: usize,
    pub matching_tests: usize,
    /// `high`, `partial`, or `none`.
    pub confidence: String,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChangeFileList {
    pub total: usize,
    pub shown: usize,
    pub omitted: usize,
    pub files: Vec<ChangeFile>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChangeFile {
    pub path: String,
    pub graph_eligible: bool,
    pub graph_covered: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChangeReadingFile {
    pub path: String,
    pub roles: Vec<String>,
    /// `high` or `partial`.
    pub confidence: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolver: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChangeImpactSummary {
    pub direct_total: usize,
    pub transitive_total: usize,
    pub shown: usize,
    pub omitted: usize,
    pub files: Vec<ChangeImpactFile>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChangeImpactFile {
    pub path: String,
    pub distance: usize,
    /// Graph resolution is heuristic, so version one reports `partial`.
    pub confidence: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolver: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChangeTestList {
    pub total: usize,
    pub shown: usize,
    pub omitted: usize,
    pub files: Vec<ChangeTestFile>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChangeTestFile {
    pub path: String,
    pub matched_sources: Vec<String>,
    /// Filename/convention matching is useful but not measured coverage.
    pub confidence: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChangeCoverage {
    /// `high`, `partial`, `none`, or `not-applicable`.
    pub observed_scope_confidence: String,
    /// `high`, `partial`, or `none`.
    pub discovery_completeness: String,
    /// `partial`, `none`, or `not-applicable`.
    pub test_mapping_confidence: String,
    pub graph_eligible_changed: usize,
    pub graph_covered_changed: usize,
    pub non_graph_changed: usize,
    pub relevant_gaps: ChangeGapCounts,
    pub outside_known_scope_gaps: ChangeGapCounts,
    pub gaps: Vec<ChangeGap>,
    pub gaps_omitted: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChangeGapCounts {
    pub unreadable_files: usize,
    pub parse_errors: usize,
    pub unresolved_imports: usize,
    pub config_errors: usize,
}

impl ChangeGapCounts {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.unreadable_files == 0
            && self.parse_errors == 0
            && self.unresolved_imports == 0
            && self.config_errors == 0
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChangeGap {
    pub path: String,
    /// `changed`, `known-impact`, `selected-context`, or
    /// `outside-known-scope`.
    pub scope: String,
    pub unreadable: bool,
    pub parse_errors: usize,
    pub unresolved_imports: usize,
    pub config_errors: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChangeValidation {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    pub reason: String,
    /// `high` for directly observed file categories, otherwise `partial`.
    pub confidence: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LineRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReviewChangedFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    pub status: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub old_ranges: Vec<LineRange>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ranges: Vec<LineRange>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub binary: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReviewFinding {
    /// `current` in line mode; four-way finding state in deep mode.
    pub state: String,
    pub finding: FindingRecord,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<FindingRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<FindingRecord>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReviewCounts {
    pub current: usize,
    pub new: usize,
    pub resolved: usize,
    pub worsened: usize,
    pub improved: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReviewDiagnostics {
    pub binary_files: usize,
    pub unreadable_files: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub oversized_files: usize,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub oversized_bytes: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub files_omitted_by_limit: usize,
    #[serde(default, skip_serializing_if = "is_false")]
    pub files_omitted_count_incomplete: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub scan_truncated: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub duration_limit_reached: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReviewReport {
    /// `lines` or `deep`.
    pub mode: String,
    pub scope: String,
    pub changed_files: Vec<ReviewChangedFile>,
    pub counts: ReviewCounts,
    pub findings: Vec<ReviewFinding>,
    #[serde(default)]
    pub diagnostics: ReviewDiagnostics,
}

/// Focused full-repository context for one requested file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExplainReport {
    pub schema_version: String,
    pub root: PathBuf,
    pub path: PathBuf,
    pub generated_at: String,
    pub encoding: String,
    #[serde(default)]
    pub execution: ExecutionMetadata,
    pub discovery: DiscoveryExplanation,
    pub repository: ExplainRepository,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<FileReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk: Option<RiskExplanation>,
    #[serde(default)]
    pub testing: TestExplanation,
    #[serde(default)]
    pub graph: FileGraphContext,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<FindingRecord>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiscoveryExplanation {
    /// `analyzed`, `ignored`, `unsupported`, `unreadable`, `missing`, or
    /// `directory`.
    pub status: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<ExclusionRule>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExclusionRule {
    /// `reposcoutignore`, `gitignore`, `hidden`, `lockfile`, `exclude`, or `symlink`.
    pub kind: String,
    pub source: String,
    pub pattern: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExplainRepository {
    pub files: usize,
    pub tokens: usize,
    pub source_files: usize,
    pub test_files: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RiskExplanation {
    /// Composite-risk formula version used for this explanation.
    #[serde(default)]
    pub algorithm_version: u32,
    pub score: f64,
    pub sloc: usize,
    pub cyclomatic: u32,
    pub churn_commits: usize,
    pub size_factor: f64,
    pub complexity_factor: f64,
    pub churn_factor: f64,
    /// No matching test file or inline Rust test was found. Retained under its
    /// original JSON name for compatibility.
    pub untested: bool,
    /// Legacy compatibility field. Filename matching no longer changes risk,
    /// so new reports always emit `1.0`.
    pub untested_multiplier: f64,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TestExplanation {
    /// `source`, `test`, `non-code`, or `unavailable`.
    pub classification: String,
    /// Whether a matching test file or inline Rust test was found.
    pub tested: bool,
    pub has_inline_tests: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub logical_key: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub matches: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileGraphContext {
    pub supported: bool,
    pub fan_in: usize,
    pub fan_out: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependents: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cycles: Vec<Vec<String>>,
    pub unresolved_imports: usize,
}

impl ReviewReport {
    #[must_use]
    pub fn fails_gate(&self) -> bool {
        if self.mode == "deep" {
            self.counts.new > 0 || self.counts.worsened > 0
        } else {
            self.counts.current > 0
        }
    }
}

/// A single metric comparison between a baseline scan and the current scan.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MetricDelta {
    pub metric: String,
    pub baseline: f64,
    pub current: f64,
    pub delta: f64,
}

/// Comparison of the current scan against a previously saved baseline report.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BaselineDelta {
    pub baseline_generated_at: String,
    pub metrics: Vec<MetricDelta>,
    pub regressions: Vec<String>,
    pub regressed: bool,
    #[serde(default)]
    pub finding_changes: FindingDelta,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FindingChangeCounts {
    pub new: usize,
    pub resolved: usize,
    pub worsened: usize,
    pub improved: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FindingChange {
    /// `new`, `resolved`, `worsened`, or `improved`.
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<FindingRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<FindingRecord>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FindingDelta {
    /// `complete` or `unavailable`.
    pub comparison: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub counts: FindingChangeCounts,
    pub changes: Vec<FindingChange>,
}
