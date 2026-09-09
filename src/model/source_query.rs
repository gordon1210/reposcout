use super::{DefinitionStatus, Deserialize, PathBuf, Serialize, SourceSpan};

/// Bounded results for explicit worktree selections, including content identity, coverage, and output omissions.
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
    pub files: Vec<SourceQueryFile>,
    pub results: Vec<SourceQueryResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<SourceQueryChunk>,
}

/// A selected file and its captured content identity and definition-extraction coverage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceQueryFile {
    pub id: usize,
    pub path: PathBuf,
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

/// The outcome of one explicit target, with bounded candidates and optional references to a definition and shared source chunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceQueryResult {
    pub target: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<usize>,
    pub status: SourceQueryStatus,
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

/// The resolution or delivery state of an explicit source-query selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceQueryStatus {
    Complete,
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

/// The advertised selectors, formats, limits and language support of explicit source queries.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SourceQueryCapability {
    pub command: String,
    pub formats: Vec<String>,
    pub selectors: Vec<String>,
    pub snapshot: String,
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
