use super::{DefinitionFact, Deserialize, PathBuf, Serialize, SourceRevision};

/// Availability and completeness of bounded lexical extraction for one captured file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LexicalStatus {
    Inspected,
    ParseErrors,
    Unsupported,
    #[default]
    Unavailable,
}

/// The source field that supplied a lexical term.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LexicalField {
    Name,
    Path,
    Signature,
    Comment,
    Code,
}

/// Bounded normalized terms from one search field, without source bodies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LexicalFieldTerms {
    pub field: LexicalField,
    pub terms: Vec<String>,
    #[serde(default, skip_serializing_if = "super::is_false")]
    pub truncated: bool,
}

/// A declaration identity and its bounded field-specific search terms.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LexicalDefinitionFacts {
    pub definition: DefinitionFact,
    pub fields: Vec<LexicalFieldTerms>,
}

/// Content-identified lexical facts and extraction coverage for one captured file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LexicalFileFacts {
    pub language: String,
    pub sha256: String,
    pub status: LexicalStatus,
    pub definitions_total: usize,
    #[serde(default, skip_serializing_if = "super::is_zero")]
    pub definitions_omitted: usize,
    pub definitions: Vec<LexicalDefinitionFacts>,
}

/// Whether every query term or at least one query term must match a candidate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FindMatchMode {
    #[default]
    All,
    Any,
}

/// A search field whose bounded extraction omitted additional content or terms.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindFieldTruncation {
    pub field: LexicalField,
    pub files: usize,
}

/// The inspected search universe and its extraction or input limits, independent of output omissions.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FindSearchCoverage {
    pub files_total: usize,
    pub files_inspected: usize,
    pub unsupported_files: usize,
    pub unavailable_files: usize,
    pub parse_error_files: usize,
    pub field_truncated_files: usize,
    pub definitions_total: usize,
    pub definitions_inspected: usize,
    pub definitions_omitted: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub truncated_fields: Vec<FindFieldTruncation>,
}

/// A bounded lexical match attributed to the field that supplied the evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindMatchEvidence {
    pub field: LexicalField,
    pub terms: Vec<String>,
    #[serde(default, skip_serializing_if = "super::is_false")]
    pub exact: bool,
}

/// An explicit definition selector for a follow-up source read.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "kebab-case")]
pub enum FindReadSelector {
    Symbol(String),
}

/// A content-identified target for an explicit follow-up read with a matching hash expectation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindReadTarget {
    pub path: PathBuf,
    pub selector: FindReadSelector,
    pub expected_hash: String,
    pub snapshot: SourceRevision,
}

/// One ranked declaration candidate with lexical evidence and an explicit source-read handoff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindQueryHit {
    pub rank: usize,
    pub path: PathBuf,
    pub name: String,
    pub kind: String,
    pub language: String,
    pub declaration_span: super::SourceSpan,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_span: Option<super::SourceSpan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    pub sha256: String,
    pub snapshot: SourceRevision,
    pub score: u32,
    pub matched_fields: Vec<FindMatchEvidence>,
    pub reason: String,
    pub read: FindReadTarget,
}

/// Body-free lexical candidates with separate search coverage and output omission accounting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindQueryReport {
    pub kind: String,
    pub schema_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "super::is_false")]
    pub root_omitted: bool,
    pub encoding: String,
    pub query: String,
    pub query_terms: Vec<String>,
    pub match_mode: FindMatchMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind_filter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language_filter: Option<String>,
    pub limit: usize,
    pub token_budget: usize,
    pub byte_budget: usize,
    pub coverage: FindSearchCoverage,
    pub total_matches: usize,
    pub returned_matches: usize,
    pub limit_omitted: usize,
    pub budget_omitted: usize,
    pub hits: Vec<FindQueryHit>,
}

/// The advertised lexical-search fields, matching modes, filters and hard query limits.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FindQueryCapability {
    pub command: String,
    pub formats: Vec<String>,
    pub match_modes: Vec<String>,
    pub default_match_mode: String,
    pub fields: Vec<String>,
    pub default_limit: usize,
    pub max_limit: usize,
    pub max_query_chars: usize,
    pub max_query_terms: usize,
    pub default_tokens: usize,
    pub min_tokens: usize,
    pub max_tokens: usize,
    pub default_bytes: usize,
    pub min_bytes: usize,
    pub max_bytes: usize,
    pub max_definitions_per_file: usize,
    pub max_terms_per_field: usize,
    pub max_term_chars: usize,
    pub max_code_bytes_per_definition: usize,
    pub max_comment_bytes_per_definition: usize,
    pub max_comment_nodes_per_file: usize,
    /// Hard cap on syntax nodes visited per file during lexical extraction.
    pub max_syntax_nodes_per_file: usize,
}
