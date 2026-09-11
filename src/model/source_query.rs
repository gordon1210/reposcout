use super::{DefinitionStatus, Deserialize, PathBuf, Serialize, SourceSpan};

/// The content side of a captured source file, with a pinned tree revision when applicable.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "revision", rename_all = "kebab-case")]
pub enum SourceRevision {
    #[default]
    Worktree,
    Index,
    Tree(String),
    Empty,
}

/// The advertised availability, diff scopes, source opt-in and work and output limits of changed-definition queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeQueryCapability {
    pub command: String,
    pub available: bool,
    pub formats: Vec<String>,
    pub requires_one_of: Vec<String>,
    pub source_flag: String,
    pub max_changed_file_pairs: usize,
    pub max_result_targets: usize,
    pub max_hunks_per_file: usize,
    pub max_mapping_work_per_side: usize,
    pub embedded_flag: String,
    pub embedded_tokens: usize,
    pub embedded_bytes: usize,
}

/// Changed-file and definition-selection accounting, including capture gaps and bounded output omissions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceChangeSummary {
    pub scope: String,
    pub base: SourceRevision,
    pub current: SourceRevision,
    pub total_files: usize,
    pub processed_files: usize,
    pub omitted_files: usize,
    pub mapped_definitions: usize,
    pub unmapped_ranges: usize,
    pub unavailable_sides: usize,
    pub total_hunks: usize,
    pub omitted_hunks: usize,
    pub unprocessed_ranges: usize,
}

/// The captured diff-side evidence connecting a changed range to a selected definition or mapping gap.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceChangeEvidence {
    pub side: String,
    pub file_status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counterpart: Option<PathBuf>,
    pub reason: String,
    pub ranges: Vec<super::LineRange>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wrapper_ranges: Vec<super::LineRange>,
    #[serde(default, skip_serializing_if = "super::is_false")]
    pub ambiguous: bool,
}

/// Bounded results for explicit snapshot selections or changed definitions, including content identity, coverage and output omissions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceQueryReport {
    pub kind: String,
    pub schema_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "super::is_false")]
    pub root_omitted: bool,
    pub encoding: String,
    pub mode: String,
    pub token_budget: usize,
    pub byte_budget: usize,
    pub requested_targets: usize,
    pub omitted_targets: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change: Option<SourceChangeSummary>,
    pub files: Vec<SourceQueryFile>,
    pub results: Vec<SourceQueryResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<SourceQueryChunk>,
}

/// A selected file and its captured snapshot identity, content hash and definition-extraction coverage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceQueryFile {
    pub id: usize,
    pub path: PathBuf,
    #[serde(default)]
    pub snapshot: SourceRevision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<SourceQueryStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extraction: Option<DefinitionStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declarations: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_definitions: Option<usize>,
}

/// The outcome of one explicit or derived changed-definition target, with bounded candidates and optional definition, change evidence and shared source references.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceQueryResult {
    pub target: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<usize>,
    pub status: SourceQueryStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change: Option<SourceChangeEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition: Option<SourceQueryDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<SourceQueryDefinition>,
    #[serde(default, skip_serializing_if = "super::is_zero")]
    pub total_candidates: usize,
    #[serde(default, skip_serializing_if = "super::is_zero")]
    pub omitted_candidates: usize,
}

/// The resolution, change-mapping or delivery state of a source-query target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceQueryStatus {
    Complete,
    Changed,
    Unmapped,
    Binary,
    Conflict,
    Outline,
    Ambiguous,
    NotFound,
    Stale,
    Excluded,
    Unsupported,
    Unavailable,
    ParseError,
    InvalidPath,
    IgnoreError,
    Unreadable,
    NotRegularFile,
    Oversized,
    InputBudgetExceeded,
    DeadlineExceeded,
    BudgetOmitted,
}

/// A declaration identity and its selection and retrieval ranges within captured file content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceQueryDefinition {
    pub name: String,
    pub kind: String,
    pub declaration_span: SourceSpan,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_span: Option<SourceSpan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

/// One complete source range shared by target results to avoid repeated source content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceQueryChunk {
    pub id: usize,
    pub file: usize,
    pub span: SourceSpan,
    pub content: String,
}

/// The advertised availability, snapshots, selectors, formats, limits and language support of source and changed-definition queries.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SourceQueryCapability {
    pub command: String,
    /// Whether explicit source queries are available on the current platform.
    #[serde(default)]
    pub available: bool,
    /// Platform families supporting explicit source queries; currently Unix only.
    #[serde(default)]
    pub platforms: Vec<String>,
    pub formats: Vec<String>,
    pub selectors: Vec<String>,
    pub snapshot: String,
    #[serde(default)]
    pub snapshots: Vec<String>,
    pub hash_algorithm: String,
    pub default_tokens: usize,
    pub min_tokens: usize,
    pub max_tokens: usize,
    pub default_bytes: usize,
    pub min_bytes: usize,
    pub max_bytes: usize,
    pub max_targets: usize,
    pub max_candidates: usize,
    pub max_outline_declarations: usize,
    pub max_input_file_bytes: u64,
    pub max_input_total_bytes: u64,
    pub languages: Vec<SourceQueryLanguage>,
}

/// A language and the canonical declaration kinds supported for precise source retrieval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceQueryLanguage {
    pub language: String,
    pub kinds: Vec<String>,
}
