use super::{
    CallResolutionCoverage, DefinitionFact, DefinitionStatus, Deserialize, GraphEdge, LineRange,
    PathBuf, ResolvedCallReference, Serialize,
};
use std::collections::{BTreeMap, BTreeSet};

/// Immutable comparison identity. `base_commit` is the actual comparison base.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReviewComparison {
    pub mode: String,
    pub requested_base_commit: String,
    pub base_commit: String,
    pub head_commit: String,
    pub base_tree: String,
    pub head_tree: String,
    pub rename_detection_complete: bool,
}

/// Independent input, analysis and output coverage for one revision.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReviewRevisionCoverage {
    pub tree: String,
    pub observed_files: usize,
    pub inventory_truncated: bool,
    pub captured_files: usize,
    pub unsupported_inventory_files: usize,
    #[serde(default)]
    pub unsupported_graph_paths: usize,
    pub unavailable_files: BTreeMap<String, usize>,
    pub graph_files: usize,
    pub changed_graph_files: usize,
    pub changed_files_without_graph: usize,
    pub unresolved_imports: usize,
    pub unresolved_type_relations: usize,
    pub parse_errors: usize,
    pub config_errors: usize,
    pub unsupported_call_files: usize,
    pub incomplete_call_files: usize,
    pub call_resolution: CallResolutionCoverage,
}

/// Exact counters survive omission of individual output entries.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReviewContextTotals {
    pub changes: usize,
    pub changes_not_analyzed: usize,
    #[serde(default)]
    pub changes_without_hunks: usize,
    pub changes_omitted: usize,
    pub definitions: usize,
    pub definitions_omitted: usize,
    pub relations: usize,
    pub relations_omitted: usize,
    pub candidates: usize,
    pub candidates_omitted: usize,
    pub candidate_tokens: usize,
    pub candidate_bytes: usize,
    pub unknown_candidate_sizes: usize,
    pub selected_files: usize,
    pub selected_tokens: usize,
    pub selection_omitted_files: usize,
    pub selection_omitted_tokens: usize,
    pub source_files: usize,
    pub source_files_omitted: usize,
    pub diff_tokens: usize,
    pub diff_unavailable_files: usize,
    pub diff_files_omitted: usize,
}

/// Body-free PR review evidence; source and unified diffs are explicit opt-ins.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewContextReport {
    pub kind: String,
    pub schema_version: String,
    pub strategy_version: u32,
    pub comparison: ReviewComparison,
    pub encoding: String,
    pub token_budget: usize,
    pub byte_budget: usize,
    pub context_budget: Option<usize>,
    pub context_max_files: Option<usize>,
    pub totals: ReviewContextTotals,
    pub coverage: Vec<ReviewRevisionCoverage>,
    pub limitations: Vec<String>,
    pub changes: Vec<ReviewContextChange>,
    pub relations: Vec<ReviewContextRelation>,
    pub context: Vec<ReviewContextFile>,
    /// Facts for the text-only agent brief; never part of the JSON contract.
    #[serde(skip)]
    pub brief: Option<ReviewBrief>,
}

/// Brief-only evidence derived from the same captured revisions before output projection.
#[derive(Debug, Clone, Default)]
pub struct ReviewBrief {
    /// Direct relations touching changed paths before output projection.
    pub direct_relations: usize,
    /// Per revision (base, head): dependents at distance two or more, counted but not listed.
    pub transitive_dependents: [usize; 2],
    /// Per revision: files included only through a changed resolver configuration's scope.
    pub config_scope_files: [usize; 2],
    /// Head-side shortest import chains from a changed file to its nearest non-test entry.
    pub entry_chains: Vec<Vec<PathBuf>>,
    /// Changed identifiers searched in head-snapshot documentation.
    pub terms: Vec<String>,
    pub mentions: Vec<ReviewDocMention>,
    /// Matching documentation lines before per-file and output limits.
    pub mentions_total: usize,
    /// Direct importer lines naming identifiers of their change, in admission priority order.
    pub usage: Vec<ReviewUsageLine>,
    /// Matching importer lines before per-file and output limits.
    pub usage_total: usize,
    /// Per revision: changed files without call-reference support, and with incomplete references.
    pub call_gaps: [[usize; 2]; 2],
    /// Token edits repeated in at least three hunks, in display order (`R1`, `R2`, …).
    pub rewrites: Vec<ReviewRewrite>,
    /// Every parsed diff hunk, in change and hunk order.
    pub hunks: Vec<ReviewBriefHunk>,
    /// Rewrite-only hunks shown as examples, covering each coverable rewrite once.
    pub examples: Vec<usize>,
    /// Indices of changes that render as their own entry; other changes are fully described by
    /// rewrites, moves or the rename list.
    pub entry_changes: Vec<usize>,
    /// Indices of renamed, copied or mode-changed changes without an entry of their own.
    pub renames: Vec<usize>,
    /// Changed lines recurring in another hunk, per pair of source and destination hunk, in
    /// that order.
    pub moves: Vec<ReviewBriefMove>,
    /// Old spellings replaced by the change that still occur in the head snapshot.
    pub stale: Vec<ReviewStaleName>,
    /// Added methods reusing base method names without resolved callers, per member name.
    pub dispatch: Vec<ReviewDispatchGap>,
    /// Distinct head files declaring the added methods of `dispatch`.
    pub dispatch_files: usize,
    /// Output projection state; the renderer shows only selected optional items.
    pub selection: ReviewBriefSelection,
}

/// One token edit that the brief names once instead of repeating its hunks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewRewrite {
    /// Changed tokens, joined by single spaces; empty for pure insertions.
    pub old: String,
    /// Replacement tokens, joined by single spaces; empty for pure deletions.
    pub new: String,
    /// The edit also applies to part of a longer identifier or literal.
    pub inside: bool,
    /// Distinct hunks containing the edit.
    pub hunks: usize,
    /// Every occurrence replaces an identifier token by an identifier token, as a whole or in
    /// part; only hunks made of renames fold without showing their lines.
    pub rename: bool,
}

/// Changed lines of one hunk that recur in another hunk after rewrites: removed lines of
/// `from` (base line numbers) and the added lines of `to` (head line numbers) they pair with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewBriefMove {
    /// Indices into `ReviewBrief::hunks`.
    pub from: usize,
    pub to: usize,
    /// First and last paired base line of `from` and head line of `to`.
    pub from_lines: [usize; 2],
    pub to_lines: [usize; 2],
}

/// How the brief accounts for one diff hunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReviewHunkRole {
    /// Every token edit is a listed rename rewrite.
    Rewrite,
    /// Every changed line recurs in another hunk after rewrites (`ReviewBrief::moves`).
    Moved,
    /// The hunk has its own lines to show.
    #[default]
    Shown,
}

/// One unified-diff hunk with its brief classification and rendered body.
#[derive(Debug, Clone, Default)]
pub struct ReviewBriefHunk {
    /// Index into the report's changes.
    pub change: usize,
    pub old_start: usize,
    pub old_lines: usize,
    pub new_start: usize,
    pub new_lines: usize,
    pub role: ReviewHunkRole,
    /// Rewrites whose edits occur in the hunk, as indices into `ReviewBrief::rewrites`.
    pub rewrites: Vec<usize>,
    /// A rewrite this hunk deviates from: an edit shares one side with it or differs in one token.
    pub variant_of: Option<usize>,
    /// Changed lines left out of `text` because they recur in another hunk after rewrites.
    pub moved_lines: usize,
    /// Changed lines left out of `text` because the line at the same position of the same block
    /// on the other side equals them after rewrites.
    pub unchanged_lines: usize,
    /// First changed line on the surviving side (head, or base for deleted files).
    pub site_line: usize,
    /// Header line without annotations, then the body: the whole hunk, only its changed lines,
    /// or numbered residual changed lines.
    pub text: String,
    /// The hunk's changed lines, for rewrite examples.
    pub changed: Vec<String>,
    /// Admission order among shown hunks; lower values are kept first.
    pub priority: u8,
    pub test: bool,
    /// Every edit is a listed rewrite, but not every one is a proven pure rename; `text` holds
    /// only the numbered changed lines, and omitted such hunks are grouped by rewrites.
    pub explained: bool,
    /// Rename rewrites applied to moved lines this hunk receives, as rewrite index and head line.
    pub moved_sites: Vec<(usize, usize)>,
}

/// One old spelling with the head lines that still contain it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewStaleName {
    pub old: String,
    pub new: String,
    /// Lines containing the old spelling in base and head.
    pub base_lines: usize,
    pub head_lines: usize,
    /// Head occurrences in path and line order, bounded; `head_lines` stays exact.
    pub hits: Vec<ReviewDocMention>,
}

/// Methods that non-test changed files declare beyond their base count of that name, that reuse a
/// method name of non-test base files and that no resolved head reference targets: new
/// implementations reached only through dynamic dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewDispatchGap {
    pub name: String,
    /// Added methods with this name.
    pub definitions: usize,
    /// Head receiver call or reference sites naming the member, outside test files and the files
    /// declaring these added methods, in path and line order, bounded; `sites_total` stays exact.
    pub sites: Vec<(PathBuf, usize)>,
    pub sites_total: usize,
}

/// Optional brief items admitted by output projection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReviewBriefSelection {
    pub examples: bool,
    pub site_lines: bool,
    /// Omitted rule-explained hunks are listed by site per rewrite combination; otherwise each
    /// combination is counted and one command fetches them all.
    pub explained_sites: bool,
    /// Change entries with heading and analysis.
    pub entries: BTreeSet<usize>,
    /// Definitions, callers, type relations and entry chains of a change entry.
    pub details: BTreeSet<usize>,
    pub importers: BTreeSet<usize>,
    pub hunks: BTreeSet<usize>,
    pub mentions: BTreeSet<usize>,
    pub usage: BTreeSet<usize>,
    /// Mandatory sections shrink at levels 1 (no stale lines), 2 (omitted hunks as one line)
    /// and 3 (rewrites, moves, renames, stale spellings and omissions as counts with one diff
    /// command).
    pub level: u8,
}

/// One line in a direct importer without resolved references that names an identifier of a change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewUsageLine {
    /// Base and head paths of the change whose file the importer imports.
    pub change: [Option<PathBuf>; 2],
    /// Revision whose snapshot supplied the line: `head`, or `base` for base-only importers.
    pub side: String,
    pub path: PathBuf,
    pub line: usize,
    pub text: String,
}

/// One documentation line in the head snapshot that names a changed identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewDocMention {
    pub path: PathBuf,
    pub line: usize,
    pub text: String,
}

/// Git change identity, including modes and explicit per-side availability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewContextChange {
    pub status: String,
    pub base: Option<ReviewChangedSide>,
    pub head: Option<ReviewChangedSide>,
    #[serde(default)]
    pub hunk_status: ReviewAnalysisStatus,
    #[serde(default)]
    pub hunks: Option<usize>,
    pub hunks_omitted: usize,
    pub diff_tokens: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
}

/// Old/new declaration and raw line evidence from the same captured bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewChangedSide {
    pub path: PathBuf,
    pub mode: u32,
    pub blob: String,
    pub status: String,
    pub sha256: Option<String>,
    pub extraction: DefinitionStatus,
    #[serde(default)]
    pub mapping_status: ReviewAnalysisStatus,
    pub ranges: Vec<LineRange>,
    pub definitions: Vec<DefinitionFact>,
    /// Output omissions do not change the availability of the underlying mapping.
    #[serde(default, skip_serializing_if = "super::is_zero")]
    pub definitions_omitted: usize,
    #[serde(default, skip_serializing_if = "super::is_zero")]
    pub ranges_omitted: usize,
    /// Declarations retained through an unchanged header on the opposite hunk side.
    #[serde(default, skip_serializing_if = "super::is_zero")]
    pub counterpart_definitions: usize,
    #[serde(default, skip_serializing_if = "super::is_zero")]
    pub ambiguous_counterparts: usize,
    #[serde(default, skip_serializing_if = "super::is_zero")]
    pub unprocessed_counterparts: usize,
    /// The opposite side could not establish a complete set of changed declarations.
    #[serde(default, skip_serializing_if = "super::is_false")]
    pub counterpart_seed_mapping_incomplete: bool,
    pub unmapped_ranges: usize,
    pub unprocessed_ranges: usize,
    pub ambiguous_definitions: usize,
    pub wrapper_ranges: usize,
}

/// Availability of comparison work, independent of source extraction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewAnalysisStatus {
    Available,
    Partial,
    #[default]
    Unavailable,
}

/// Revision-local directed evidence; imports and concrete symbol references stay separate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewContextRelation {
    pub side: String,
    pub kind: String,
    pub edge: GraphEdge,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_basis: Option<ReviewChangeBasis>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<ResolvedCallReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_relation: Option<super::GraphSymbolEdge>,
}

/// Why a resolved symbol reference is relevant to the changed source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewChangeBasis {
    ChangedDefinition,
    ChangedFile,
}

/// One unique file side, with whole-file cost and explainable selection state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewContextFile {
    pub side: String,
    pub snapshot: String,
    pub path: PathBuf,
    pub status: String,
    pub sha256: Option<String>,
    pub bytes: Option<usize>,
    pub tokens: Option<usize>,
    pub roles: Vec<String>,
    pub distance: usize,
    pub via: Option<PathBuf>,
    pub test_evidence: Vec<String>,
    pub selection: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewContextCapability {
    pub command: String,
    pub available: bool,
    pub formats: Vec<String>,
    pub comparisons: Vec<String>,
    pub max_inventory_files_per_revision: usize,
    pub max_input_files: usize,
    pub max_input_bytes: u64,
    pub max_input_file_bytes: u64,
    pub default_tokens: usize,
    pub default_bytes: usize,
    pub context_unit: String,
}
