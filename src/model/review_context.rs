use super::{
    CallResolutionCoverage, DefinitionFact, DefinitionStatus, Deserialize, GraphEdge, LineRange,
    PathBuf, ResolvedCallReference, Serialize,
};
use std::collections::BTreeMap;

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
}

/// Git change identity, including modes and explicit per-side availability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewContextChange {
    pub status: String,
    pub base: Option<ReviewChangedSide>,
    pub head: Option<ReviewChangedSide>,
    pub hunks: usize,
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
    pub ranges: Vec<LineRange>,
    pub definitions: Vec<DefinitionFact>,
    pub unmapped_ranges: usize,
    pub unprocessed_ranges: usize,
    pub ambiguous_definitions: usize,
    pub wrapper_ranges: usize,
}

/// Revision-local directed evidence; imports and concrete symbol references stay separate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewContextRelation {
    pub side: String,
    pub kind: String,
    pub edge: GraphEdge,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<ResolvedCallReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_relation: Option<super::GraphSymbolEdge>,
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
