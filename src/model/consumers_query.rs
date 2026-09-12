use super::{
    CallResolutionCoverage, CallSymbolIdentity, Deserialize, FindReadTarget, PathBuf,
    ResolvedCallReference, Serialize, UnresolvedCallReference,
};

/// Whether to traverse incoming consumers, outgoing dependencies or both relation directions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConsumersDirection {
    #[default]
    Incoming,
    Outgoing,
    Both,
}

/// One uniquely reached symbol with shortest-path relation evidence and a content-checked source-read handoff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsumerHit {
    pub symbol: CallSymbolIdentity,
    pub depth: usize,
    pub evidence: Vec<ResolvedCallReference>,
    pub read: FindReadTarget,
}

/// Extraction, resolution and traversal coverage, distinct from output-budget omissions.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConsumerCoverage {
    pub files_total: usize,
    pub files_available: usize,
    pub files_unsupported: usize,
    pub files_unavailable: usize,
    pub files_parse_errors: usize,
    pub files_truncated: usize,
    pub declarations_omitted: usize,
    pub relations_omitted: usize,
    pub discovery_omitted: usize,
    pub discovery_omitted_count_incomplete: bool,
    pub unreadable_files: usize,
    pub oversized_files: usize,
    pub walker_errors: usize,
    pub scan_truncated: bool,
    pub deadline_reached: bool,
    pub resolution: CallResolutionCoverage,
}

/// Body-free reachable symbols and relation evidence with separate extraction, resolution and projection coverage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsumersQueryReport {
    pub kind: String,
    pub schema_version: String,
    pub root: Option<PathBuf>,
    pub root_omitted: bool,
    pub encoding: String,
    pub direction: ConsumersDirection,
    pub depth: usize,
    pub limit: usize,
    pub path_limit: usize,
    pub token_budget: usize,
    pub byte_budget: usize,
    pub seeds: Vec<CallSymbolIdentity>,
    pub coverage: ConsumerCoverage,
    pub total_matches: usize,
    pub returned_matches: usize,
    pub depth_omitted: usize,
    pub path_omitted: usize,
    pub limit_omitted: usize,
    pub budget_omitted: usize,
    pub unresolved: Vec<UnresolvedCallReference>,
    pub unresolved_omitted: usize,
    pub hits: Vec<ConsumerHit>,
}

/// Advertised worktree selectors, relation directions and hard consumer-query limits.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConsumersQueryCapability {
    pub command: String,
    pub formats: Vec<String>,
    pub snapshots: Vec<String>,
    pub directions: Vec<String>,
    pub default_depth: usize,
    pub max_depth: usize,
    pub default_limit: usize,
    pub max_limit: usize,
    pub default_path_limit: usize,
    pub max_path_limit: usize,
    pub default_tokens: usize,
    pub min_tokens: usize,
    pub max_tokens: usize,
    pub default_bytes: usize,
    pub min_bytes: usize,
    pub max_bytes: usize,
}
