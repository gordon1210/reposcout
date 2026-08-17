//! Shared data model for scan results. All analyzers write into these structs,
//! and all reporters read from them. This is the stable, serializable contract
//! that keeps analyzer modules decoupled.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if predicates receive fields by shared reference"
)]
fn is_false(b: &bool) -> bool {
    !*b
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if predicates receive fields by shared reference"
)]
fn is_zero(value: &usize) -> bool {
    *value == 0
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if predicates receive fields by shared reference"
)]
fn is_zero_u64(value: &u64) -> bool {
    *value == 0
}

/// Bump when the JSON shape changes in a breaking way.
pub const SCHEMA_VERSION: &str = "2.0";

/// Top-level report produced by a scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanReport {
    pub schema_version: String,
    /// Absolute git-repo root (or the scan target if not in a git repo).
    pub root: PathBuf,
    /// The path that was scanned (may be a subdir/file of `root`).
    pub target: PathBuf,
    /// RFC3339 timestamp.
    pub generated_at: String,
    /// tiktoken encoding used for token counts.
    pub encoding: String,
    /// Effective analyzer/settings profile used to make scoped reports and
    /// baselines comparable. Absent in reports produced before this metadata
    /// was introduced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analysis_profile: Option<ScanProfile>,
    /// Runtime profile, configuration provenance, safety limits, and execution
    /// telemetry. Additive so older reports remain readable.
    #[serde(default)]
    pub execution: ExecutionMetadata,
    /// Complete, versioned catalog of actionable findings. Summary rendering
    /// drops this unless baseline-ready output is requested.
    #[serde(default)]
    pub finding_catalog: FindingCatalog,
    pub summary: Summary,
    /// Compact raw facts describing the observed reading and structural scope.
    /// This is additive and does not trigger analysis beyond the selected
    /// workflow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_scope: Option<WorkScope>,
    pub files: Vec<FileReport>,
    pub duplicates: Duplication,
    /// Per-directory rollup, populated only when `--by-dir` is passed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub directories: Vec<DirSummary>,
    /// Baseline comparison, populated only when `--baseline` is passed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline: Option<BaselineDelta>,
    /// Import dependency graph, populated only when `--graph` is passed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph: Option<DepGraph>,
    /// Token-budgeted, explainable reading plan populated only when context
    /// planning is enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<ContextPlan>,
    /// Discovery and analysis outcome counts, so callers can judge scan
    /// completeness instead of inferring it from the file array alone.
    #[serde(default)]
    pub diagnostics: ScanDiagnostics,
    /// Change impact, populated only when `--impact` is passed with a diff
    /// scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub impact: Option<ImpactAnalysis>,
    /// Bounded, decision-oriented projection populated only by
    /// `--change-summary`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_summary: Option<ChangeSummary>,
    /// Changed-line review, populated only by `--review`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<ReviewReport>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExecutionMetadata {
    /// Effective caller profile (`full`, `agent`, `safe`, or a daemon profile).
    pub profile: String,
    /// `project`, `user`, or `defaults` according to configuration trust mode.
    pub config_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub global_config: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_config: Option<PathBuf>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub safety_limits: Vec<String>,
    /// Stage timings are populated by the scanner and remain optional for old
    /// reports and focused projections.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub stage_ms: BTreeMap<String, usize>,
    /// Whether incremental per-file caching participated in this execution.
    pub cache_enabled: bool,
    /// Content-identical files restored from the analysis cache.
    #[serde(skip_serializing_if = "is_zero")]
    pub cache_hits: usize,
    /// Enabled-cache lookups that missed and required fresh per-file analysis.
    #[serde(skip_serializing_if = "is_zero")]
    pub cache_misses: usize,
    /// Content-identical cache hits that still needed lazy declaration or
    /// graph facts computed for the current query.
    #[serde(skip_serializing_if = "is_zero")]
    pub cache_enrichments: usize,
    /// First-class-language files with reusable import and symbol facts.
    #[serde(skip_serializing_if = "is_zero")]
    pub graph_fact_files: usize,
}

/// Effective settings that determine which aggregate metrics are available.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScanProfile {
    pub analyzers: AnalyzerProfile,
    /// `full`, `since`, `staged`, or `working`.
    pub diff_scope: String,
    /// Resolved base tree object used by a diff-scoped scan.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_base: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplication: Option<DuplicationProfile>,
    /// Effective file eligibility for health analysis and derived rankings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health: Option<HealthProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub findings: Option<FindingProfile>,
    /// Input and runtime bounds that can affect the analyzed file universe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourceProfile>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HealthProfile {
    /// `source` or `all`.
    pub scope: String,
    /// Canonical non-source format names added to `source` scope.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub includes: Vec<String>,
    /// Repository-relative path globs removed after scope and includes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excludes: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "the stable JSON contract records independently available analyzers as additive boolean capabilities"
)]
pub struct AnalyzerProfile {
    pub tokens: bool,
    pub complexity: bool,
    pub imports: bool,
    pub markers: bool,
    pub duplication: bool,
    pub churn: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DuplicationProfile {
    pub min_tokens: usize,
    pub min_lines: usize,
    pub min_similarity: f64,
    pub mode: String,
    pub format_scope: String,
    /// `exclude` for the build-artifact-filtered default or `include` after explicit opt-in.
    /// Empty in reports created before artifact filtering became part of the corpus contract.
    #[serde(default)]
    pub artifact_policy: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FindingProfile {
    pub catalog_version: u32,
    pub max_complexity: u32,
    pub markers: Vec<String>,
    pub risk_algorithm_version: u32,
    pub risk_threshold: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ResourceProfile {
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
    pub max_files: usize,
    pub max_git_blob_bytes: u64,
    pub max_scan_seconds: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub max_churn_deltas_per_commit: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub max_churn_total_deltas: usize,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub max_churn_output_bytes: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub max_git_path_bytes: usize,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub max_churn_cache_bytes: u64,
    /// Present when the resource profile records ignore-loading policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_repository_ignores: Option<bool>,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub max_ignore_file_bytes: u64,
}

/// Per-directory aggregated metrics, produced by `--by-dir[=DEPTH]`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DirSummary {
    pub path: String,
    pub files: usize,
    pub tokens: usize,
    pub loc: usize,
    pub sloc: usize,
    pub cyclomatic_avg: f64,
    pub cyclomatic_max: u32,
    pub mi_avg: f64,
    pub duplicated_lines: usize,
    pub untested_source_files: usize,
}

/// Inventory totals for authored program and build source. The repository-wide
/// summary remains the complete context footprint across all recognized files.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SourceSummary {
    pub files: usize,
    pub bytes: u64,
    pub tokens: usize,
    pub loc: usize,
    pub sloc: usize,
    pub comment_lines: usize,
}

/// Aggregated, repo-wide view — the "status at a glance".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Summary {
    pub files: usize,
    pub bytes: u64,
    pub tokens: usize,
    pub loc: usize,
    pub sloc: usize,
    pub comment_lines: usize,
    pub comment_ratio: f64,
    /// Files whose line/comment counts came from the generic fallback scanner.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub line_metrics_approximate_files: usize,
    /// Source/build-only inventory used by concise human reports.
    #[serde(default)]
    pub source: SourceSummary,
    pub languages: Vec<LanguageStat>,
    pub complexity: ComplexitySummary,
    pub duplication: DuplicationSummary,
    /// Marker keyword -> count (TODO, FIXME, HACK, ...).
    pub markers: BTreeMap<String, usize>,
    pub top_token_files: Vec<FileRef>,
    /// Source/build-only counterpart to `top_token_files`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub top_source_token_files: Vec<FileRef>,
    pub top_hotspots: Vec<Hotspot>,
    /// The most complex individual functions across the scan (by cyclomatic,
    /// then cognitive), regardless of which file they live in.
    pub top_functions: Vec<FunctionHotspot>,
    /// Functions whose cyclomatic complexity exceeds the configured maximum,
    /// ordered by severity and capped by the top-N setting.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub complexity_violations: Vec<FunctionHotspot>,
    /// Compact highest-impact duplicate blocks (exact and near), with nested or
    /// substantially overlapping families suppressed.
    pub top_duplicates: Vec<DuplicateBlock>,
    /// Highest-impact duplicate blocks that touch production source. Test-only
    /// and Rust inline-test-only families are excluded.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub top_production_duplicates: Vec<DuplicateBlock>,
    /// Highest-impact pair findings with stable IDs and precise locations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub top_duplicate_findings: Vec<DuplicateFindingSummary>,
    /// Aggregated symbol counts across all first-class-language files.
    #[serde(default)]
    pub symbols: SymbolCounts,
    /// Files that are almost certainly not hand-authored code an agent should
    /// read (generated, minified, bundled, or vendored), sorted by tokens descending.
    #[serde(default)]
    pub skip_candidates: Vec<SkipCandidate>,
    /// Framework-backed test discovery. Omitted when no supported test setup
    /// can be established from repository-owned configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_presence: Option<TestPresence>,
    /// Source files ranked by composite risk score (size × complexity × churn).
    #[serde(default)]
    pub top_risks: Vec<RiskEntry>,
    /// Quick machine-readable health verdict.
    #[serde(default)]
    pub assessment: Assessment,
}

/// Per-language rollup for the breakdown table.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LanguageStat {
    pub name: String,
    /// Whether the format belongs to the default source/build corpus.
    #[serde(default)]
    pub source: bool,
    pub files: usize,
    pub bytes: u64,
    pub loc: usize,
    pub sloc: usize,
    pub comment_lines: usize,
    pub tokens: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComplexitySummary {
    pub cyclomatic_total: u64,
    pub cyclomatic_avg: f64,
    pub cyclomatic_max: u32,
    pub cognitive_total: u64,
    pub cognitive_avg: f64,
    pub cognitive_max: u32,
    pub mi_avg: f64,
    pub mi_min: f64,
    /// Number of individual functions analyzed (first-class languages only).
    pub functions: usize,
    /// Configured maximum cyclomatic complexity allowed per function.
    #[serde(default)]
    pub cyclomatic_threshold: u32,
    /// Total functions above `cyclomatic_threshold` (the findings list is top-N capped).
    #[serde(default)]
    pub functions_over_threshold: usize,
    /// Number of files whose complexity is approximate (heuristic fallback).
    pub approximate_files: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DuplicationSummary {
    pub exact_groups: usize,
    pub near_groups: usize,
    pub duplicated_lines: usize,
    pub duplicated_pct: f64,
    /// Physical lines in files eligible for duplication analysis.
    #[serde(default)]
    pub analyzed_lines: usize,
    /// Distinct duplication-lexer tokens covered by clone instances.
    #[serde(default)]
    pub duplicated_tokens: usize,
    /// Total tokens produced by the duplication lexer for the scan.
    #[serde(default)]
    pub analyzed_tokens: usize,
    /// Union-based token coverage over `analyzed_tokens`.
    #[serde(default)]
    pub duplicated_tokens_pct: f64,
    /// Duplication coverage partitioned by detected language/format.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub by_language: Vec<LanguageDuplication>,
}

/// Duplication coverage restricted to production source lines.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProductionDuplication {
    /// Stable corpus label; currently `production-source`.
    pub corpus: String,
    pub duplicated_lines: usize,
    pub analyzed_lines: usize,
    pub duplicated_pct: f64,
    /// False when source discovery/reading or Type-2 analysis retained only
    /// partial duplication evidence.
    pub complete: bool,
}

/// Union-based duplication statistics for one detected language/format.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LanguageDuplication {
    pub name: String,
    pub files: usize,
    pub lines: usize,
    pub tokens: usize,
    pub exact_groups: usize,
    pub near_groups: usize,
    pub duplicated_lines: usize,
    pub duplicated_tokens: usize,
    pub duplicated_lines_pct: f64,
    pub duplicated_tokens_pct: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileRef {
    pub path: PathBuf,
    pub tokens: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Hotspot {
    pub path: PathBuf,
    pub commits: usize,
    pub cyclomatic: u32,
    /// churn * complexity heuristic.
    pub score: f64,
}

/// A single function ranked among the most complex in the scan.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FunctionHotspot {
    pub path: PathBuf,
    pub name: String,
    pub line: usize,
    pub cyclomatic: u32,
    pub cognitive: u32,
    pub max_nesting: u32,
}

/// A high-impact duplicate block, summarized for the "top duplicates" ranking.
/// This is a compact view of a [`CloneGroup`] (locations are truncated) so it
/// stays small even in `--summary` output.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DuplicateBlock {
    /// Lines spanned by one instance of the block.
    pub lines: usize,
    /// Tokens in the matched region.
    pub tokens: usize,
    /// 1.0 for an exact clone; the group's similarity for a near-duplicate.
    pub similarity: f64,
    /// How many places this block appears.
    pub copies: usize,
    /// Lines removable by de-duplicating: `lines * (copies - 1)`.
    pub duplicated_lines: usize,
    /// `path:start-end` for each instance (capped for compactness).
    pub locations: Vec<String>,
}

/// Compact pair-oriented finding retained in `--summary` output.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DuplicateFindingSummary {
    pub id: String,
    pub kind: String,
    pub format: String,
    pub tokens: usize,
    pub lines: usize,
    pub similarity: f64,
    pub removable_lines: usize,
    pub locations: Vec<String>,
}

/// Everything known about a single file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileReport {
    pub path: PathBuf,
    pub language: String,
    pub bytes: u64,
    pub tokens: usize,
    pub loc: usize,
    pub sloc: usize,
    pub comment_lines: usize,
    pub comment_ratio: f64,
    /// True when line/comment counts came from the generic fallback scanner.
    #[serde(default, skip_serializing_if = "is_false")]
    pub line_metrics_approximate: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub complexity: Option<Complexity>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub imports: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub markers: BTreeMap<String, usize>,
    /// Precisely located marker occurrences used by review and baseline
    /// finding projection.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub marker_occurrences: Vec<MarkerOccurrence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub churn: Option<Churn>,
    /// True if complexity/structure came from a heuristic fallback (no grammar).
    pub approximate: bool,
    /// Symbol counts derived from the AST (first-class languages only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbols: Option<SymbolCounts>,
    /// Short reason this file is probably not hand-authored code worth reading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip_hint: Option<String>,
    /// True when the file contains Rust inline-test annotations (`#[test]` /
    /// `#[cfg(test)]`), making it count as tested even without a separate
    /// test file.
    #[serde(default, skip_serializing_if = "is_false")]
    pub has_inline_tests: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Complexity {
    pub cyclomatic: u32,
    pub cognitive: u32,
    pub max_nesting: u32,
    pub halstead: Halstead,
    pub maintainability_index: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub functions: Vec<FunctionComplexity>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FunctionComplexity {
    pub name: String,
    pub line: usize,
    /// Inclusive final line of the callable scope.
    #[serde(default)]
    pub end_line: usize,
    /// Path-local semantic identity that remains stable across line movement.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub symbol_key: String,
    pub cyclomatic: u32,
    pub cognitive: u32,
    pub max_nesting: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Halstead {
    pub distinct_operators: usize,
    pub distinct_operands: usize,
    pub total_operators: usize,
    pub total_operands: usize,
    pub vocabulary: usize,
    pub length: usize,
    pub volume: f64,
    pub difficulty: f64,
    pub effort: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Churn {
    pub commits: usize,
    pub authors: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_commit: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Duplication {
    pub exact: Vec<CloneGroup>,
    pub near: Vec<CloneGroup>,
    /// Pair-oriented, precisely located projections of the retained groups.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<DuplicateFinding>,
    /// Per-file union coverage. Kept outside cached [`FileReport`] values.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub file_coverage: Vec<FileDuplication>,
}

/// Union-based duplication coverage for one file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileDuplication {
    pub path: PathBuf,
    pub format: String,
    pub lines: usize,
    pub tokens: usize,
    pub duplicated_lines: usize,
    pub duplicated_tokens: usize,
    pub duplicated_lines_pct: f64,
    pub duplicated_tokens_pct: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CloneGroup {
    /// Number of source lines spanned by each instance (approx for near-dupes).
    pub lines: usize,
    /// Number of tokens in the matched region.
    pub tokens: usize,
    /// Weighted structured-token similarity, `0..1`. Exact clones are `1.0`;
    /// Type-2 clones give partial credit to consistent identifier renames and
    /// same-category literal changes.
    #[serde(default)]
    pub similarity: f64,
    /// Detected language/format. Empty for reports created before this field.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub format: String,
    /// Content-derived family identity independent of locations and copies.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub fingerprint: String,
    pub instances: Vec<CloneInstance>,
}

/// One precisely located annotation marker.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MarkerOccurrence {
    pub marker: String,
    pub line: usize,
    pub column: usize,
    /// Hash of the normalized containing line; the source text itself is not
    /// retained in the report.
    pub context_hash: String,
    /// One-based ordinal among equal marker/context pairs in the file.
    pub occurrence: usize,
}

/// A source region attached to an actionable finding.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindingLocation {
    pub path: PathBuf,
    pub start_line: usize,
    pub end_line: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_column: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_column: Option<usize>,
}

/// One canonical actionable finding shared by review, baselines, and explain.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FindingRecord {
    pub fingerprint: String,
    /// Kind-specific semantic identity used for Git rename remapping.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub identity: String,
    /// `complexity`, `marker`, `duplication`, or `risk`.
    pub kind: String,
    pub severity: String,
    pub message: String,
    pub primary_location: FindingLocation,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related_locations: Vec<FindingLocation>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metrics: BTreeMap<String, f64>,
}

/// Versioned complete finding set for one scan.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FindingCatalog {
    pub version: u32,
    pub findings: Vec<FindingRecord>,
}

impl FindingCatalog {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.findings.is_empty()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CloneInstance {
    pub path: PathBuf,
    pub start_line: usize,
    pub end_line: usize,
    /// One-based start column and exclusive end column.
    #[serde(default)]
    pub start_column: usize,
    #[serde(default)]
    pub end_column: usize,
    /// Zero-based, half-open byte range in the source file.
    #[serde(default)]
    pub start_byte: usize,
    #[serde(default)]
    pub end_byte: usize,
    /// One-based, inclusive token range in the filtered duplication stream.
    #[serde(default)]
    pub start_token: usize,
    #[serde(default)]
    pub end_token: usize,
}

/// A stable, pair-oriented duplicate finding with precise source fragments.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DuplicateFinding {
    pub id: String,
    /// Stable identifier shared by the pair findings projected from one group.
    pub family_id: String,
    /// `exact` (Type-1) or `type2` (identifier/literal-normalized).
    pub kind: String,
    pub format: String,
    pub tokens: usize,
    pub lines_a: usize,
    pub lines_b: usize,
    pub similarity: f64,
    /// `high` for exact clones and Type-2 similarity >= 0.90; otherwise
    /// `medium` for a threshold-qualified Type-2 finding.
    pub confidence: String,
    /// Token filtering/normalization mode used for detection.
    pub normalization: String,
    pub fragment_a: DuplicateFragment,
    pub fragment_b: DuplicateFragment,
    /// Pair-level removable physical lines (the smaller fragment span).
    pub removable_lines: usize,
}

/// One precisely located side of a [`DuplicateFinding`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DuplicateFragment {
    pub path: PathBuf,
    pub start_line: usize,
    pub end_line: usize,
    pub start_column: usize,
    pub end_column: usize,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_token: usize,
    pub end_token: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
}

/// Per-file and aggregate count of first-class structural symbols.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SymbolCounts {
    pub functions: usize,
    pub types: usize,
    pub exports: usize,
}

/// One compact structural declaration projected into a selected context file.
///
/// Outlines deliberately retain only a declaration header, never a source
/// body. They are computed for first-class languages and emitted only through
/// the bounded context plan rather than every [`FileReport`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SymbolOutline {
    pub name: String,
    /// `function`, `method`, `type`, `class`, `interface`, `enum`, or `trait`.
    pub kind: String,
    pub signature: String,
    pub line: usize,
    #[serde(default, skip_serializing_if = "is_false")]
    pub exported: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
}

/// Machine-readable result of a repository symbol lookup.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SymbolQueryReport {
    pub schema_version: String,
    pub root: PathBuf,
    pub target: PathBuf,
    pub generated_at: String,
    pub query: String,
    pub match_mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    pub total_matches: usize,
    pub returned_matches: usize,
    pub truncated: bool,
    pub first_class_files: usize,
    #[serde(default)]
    pub execution: ExecutionMetadata,
    pub matches: Vec<SymbolMatch>,
}

/// One declaration matched by [`SymbolQueryReport`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SymbolMatch {
    pub path: PathBuf,
    pub language: String,
    pub name: String,
    pub kind: String,
    pub signature: String,
    pub line: usize,
    pub exported: bool,
    /// Stable relevance tier: `0` is an exact case-sensitive qualified-name
    /// match; larger values are progressively weaker name matches.
    pub rank: usize,
}

mod agent;
pub use agent::*;

mod analysis;
pub use analysis::*;

#[cfg(test)]
mod tests;
