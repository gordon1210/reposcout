//! Shared data model for scan results. All analyzers write into these structs,
//! and all reporters read from them. This is the stable, serializable contract
//! that keeps analyzer modules decoupled.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero(value: &usize) -> bool {
    *value == 0
}

fn is_zero_u64(value: &u64) -> bool {
    *value == 0
}

/// Bump when the JSON shape changes in a breaking way.
pub const SCHEMA_VERSION: &str = "1.0";

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
    /// Effective file eligibility shared by marker and duplication health
    /// analysis. Absent when neither analyzer is enabled.
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
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
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
    /// The highest-impact duplicate blocks (exact and near), ranked by how many
    /// lines could be removed by de-duplicating them.
    pub top_duplicates: Vec<DuplicateBlock>,
    /// Highest-impact pair findings with stable IDs and precise locations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub top_duplicate_findings: Vec<DuplicateFindingSummary>,
    /// Aggregated symbol counts across all first-class-language files.
    #[serde(default)]
    pub symbols: SymbolCounts,
    /// Files that are almost certainly not hand-authored code an agent should
    /// read (generated, minified, or vendored), sorted by tokens descending.
    #[serde(default)]
    pub skip_candidates: Vec<SkipCandidate>,
    /// Test-vs-source classification and filename/inline-test match estimate.
    #[serde(default)]
    pub test_presence: TestPresence,
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
    pub machine_interfaces: Vec<String>,
    pub error_formats: Vec<String>,
    pub max_graph_depth: usize,
    pub max_symbol_results: usize,
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
    /// False when one or more inputs used by the cleanup verdict were disabled.
    #[serde(default)]
    pub cleanup_worth_complete: bool,
    /// Analyzer signals that were unavailable to this verdict.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unavailable_signals: Vec<String>,
    pub reasons: Vec<String>,
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
    pub skipped_files: usize,
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
pub struct ScanDiagnostics {
    /// Files discovered after walker filters such as ignore rules.
    pub discovered_files: usize,
    /// Files for which reposcout produced a report.
    pub analyzed_files: usize,
    /// Files discovered but not recognized as a supported language.
    pub unsupported_files: usize,
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
    /// Files omitted after a file-count, aggregate-byte, or duration limit.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub files_omitted_by_limit: usize,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_diagnostics_default_and_expose_partial_type2_analysis() {
        let legacy: ScanDiagnostics = serde_json::from_value(serde_json::json!({
            "discovered_files": 2,
            "analyzed_files": 2,
            "unsupported_files": 0,
            "unreadable_files": 0,
            "walker_errors": 0
        }))
        .expect("legacy scan diagnostics");
        assert!(!legacy.type2_analysis_partial);

        let diagnostics = ScanDiagnostics {
            oversized_files: 2,
            oversized_bytes: 8_388_608,
            files_omitted_by_limit: 3,
            bytes_omitted_by_limit: 12_582_912,
            scan_truncated: true,
            duration_limit_reached: true,
            type2_analysis_partial: true,
            type2_pools_truncated: 1,
            type2_candidate_buckets_skipped: 12,
            type2_candidate_buckets_partially_selected: 1,
            type2_seed_pairs_skipped: 42,
            type2_match_limit_reached: true,
            type2_suppression_limit_reached: true,
            type2_matches_skipped_during_suppression: 7,
            ..ScanDiagnostics::default()
        };
        let json = serde_json::to_value(diagnostics).expect("scan diagnostics JSON");

        assert_eq!(json["type2_analysis_partial"], true);
        assert_eq!(json["oversized_files"], 2);
        assert_eq!(json["oversized_bytes"], 8_388_608u64);
        assert_eq!(json["files_omitted_by_limit"], 3);
        assert_eq!(json["scan_truncated"], true);
        assert_eq!(json["duration_limit_reached"], true);
        assert_eq!(json["type2_seed_pairs_skipped"], 42);
        assert_eq!(json["type2_match_limit_reached"], true);
        assert_eq!(json["type2_suppression_limit_reached"], true);
        assert_eq!(json["type2_matches_skipped_during_suppression"], 7);
    }

    #[test]
    fn partial_scan_profiles_default_new_nested_fields() {
        let json = serde_json::json!({
            "analyzers": { "tokens": true },
            "diff_scope": "full",
            "duplication": { "min_tokens": 50 }
        });

        let profile: ScanProfile = serde_json::from_value(json).expect("partial scan profile JSON");

        assert!(profile.analyzers.tokens);
        assert!(!profile.analyzers.complexity);
        assert_eq!(profile.diff_scope, "full");
        assert_eq!(profile.diff_base, None);
        assert_eq!(profile.health, None);
        assert_eq!(
            profile.duplication.expect("duplication profile").min_tokens,
            50
        );
    }

    #[test]
    fn legacy_graph_blocks_default_parse_error_counts() {
        let graph: DepGraph = serde_json::from_value(serde_json::json!({
            "languages": [],
            "nodes": 0,
            "edges": 0,
            "cycles": [],
            "orphans": [],
            "top_depended": [],
            "most_dependent": [],
            "unresolved_imports": 0
        }))
        .unwrap();
        let impact: ImpactAnalysis = serde_json::from_value(serde_json::json!({
            "changed_files": [],
            "graph_changed_files": [],
            "direct_dependents": [],
            "transitive_dependents": [],
            "unresolved_imports": 0,
            "confidence": "none"
        }))
        .unwrap();

        assert_eq!(graph.parse_errors, 0);
        assert_eq!(impact.parse_errors, 0);
    }

    #[test]
    fn pre_detail_duplication_json_still_deserializes() {
        let json = r#"{
            "exact": [{
                "lines": 3,
                "tokens": 20,
                "similarity": 1.0,
                "instances": [
                    {"path": "a.rs", "start_line": 1, "end_line": 3},
                    {"path": "b.rs", "start_line": 5, "end_line": 7}
                ]
            }],
            "near": []
        }"#;

        let duplication: Duplication = serde_json::from_str(json).expect("old duplication JSON");

        assert_eq!(duplication.exact.len(), 1);
        assert_eq!(duplication.exact[0].format, "");
        assert_eq!(duplication.exact[0].instances[0].start_byte, 0);
        assert!(duplication.findings.is_empty());
        assert!(duplication.file_coverage.is_empty());
    }

    #[test]
    fn pre_detail_summary_json_defaults_new_duplication_fields() {
        let json = serde_json::json!({
            "files": 0,
            "bytes": 0,
            "tokens": 0,
            "loc": 0,
            "sloc": 0,
            "comment_lines": 0,
            "comment_ratio": 0.0,
            "languages": [],
            "complexity": {
                "cyclomatic_total": 0,
                "cyclomatic_avg": 0.0,
                "cyclomatic_max": 0,
                "cognitive_total": 0,
                "cognitive_avg": 0.0,
                "cognitive_max": 0,
                "mi_avg": 0.0,
                "mi_min": 0.0,
                "functions": 0,
                "approximate_files": 0
            },
            "duplication": {
                "exact_groups": 0,
                "near_groups": 0,
                "duplicated_lines": 0,
                "duplicated_pct": 0.0
            },
            "markers": {},
            "top_token_files": [],
            "top_hotspots": [],
            "top_functions": [],
            "top_duplicates": []
        });

        let summary: Summary = serde_json::from_value(json).expect("old summary JSON");

        assert_eq!(summary.source.files, 0);
        assert!(summary.top_source_token_files.is_empty());
        assert_eq!(summary.duplication.analyzed_lines, 0);
        assert_eq!(summary.duplication.analyzed_tokens, 0);
        assert!(summary.duplication.by_language.is_empty());
        assert!(summary.top_duplicate_findings.is_empty());
        assert_eq!(summary.complexity.cyclomatic_threshold, 0);
        assert_eq!(summary.complexity.functions_over_threshold, 0);
        assert!(summary.complexity_violations.is_empty());
    }

    #[test]
    fn precise_clone_coordinates_serialize_a_real_zero_byte_offset() {
        let instance = CloneInstance {
            path: PathBuf::from("sample.rs"),
            start_line: 1,
            end_line: 3,
            start_column: 1,
            end_column: 2,
            start_byte: 0,
            end_byte: 24,
            start_token: 1,
            end_token: 8,
        };

        let value = serde_json::to_value(instance).expect("serialize clone instance");

        assert_eq!(value["start_byte"], 0);
        assert_eq!(value["start_token"], 1);
    }
}
