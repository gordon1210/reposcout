use super::{
    DefinitionStatus, Deserialize, Serialize, SourceQueryReport, SourceRevision, SourceSpan,
};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
/// Content- and encoding-identified declaration costs and environment facts with extraction omissions.
pub struct DefinitionPlanningFacts {
    pub sha256: String,
    pub encoding: String,
    pub definitions: Vec<DefinitionPlanningFact>,
    pub omitted_definitions: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
/// A captured declaration's source-token cost, supported environment links and explicit gaps.
pub struct DefinitionPlanningFact {
    pub definition: usize,
    pub tokens: usize,
    pub environment: Vec<DefinitionEnvironment>,
    pub gaps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// A bounded link to an additional declaration justified by captured syntax evidence.
pub struct DefinitionEnvironment {
    pub definition: usize,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// A body-free definition plan with independent discovery, planning and output coverage and optional explicit source.
pub struct DefinitionPlanReport {
    pub kind: String,
    pub schema_version: String,
    pub strategy_version: u32,
    pub encoding: String,
    pub token_budget: usize,
    pub context_budget: usize,
    pub byte_budget: usize,
    pub max_files: usize,
    pub max_definitions: usize,
    pub candidate_definitions: usize,
    pub input_definitions: usize,
    pub unresolved_seeds: usize,
    pub ambiguous_seeds: usize,
    pub unavailable_seeds: usize,
    pub input_files: usize,
    pub unavailable_files: usize,
    pub discovery_incomplete: bool,
    pub planning_omitted_definitions: usize,
    pub output_omitted_files: usize,
    /// Source-token cost of the selected captured-range union before output projection.
    pub selected_tokens: usize,
    pub selected_files: usize,
    pub omitted_definitions: usize,
    /// Selected entries omitted by output projection, separate from planning-budget omissions.
    pub output_omitted: usize,
    pub files: Vec<DefinitionPlanFile>,
    pub selected: Vec<PlannedDefinition>,
    pub omissions: Vec<DefinitionPlanOmission>,
    pub omitted_details: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceQueryReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// A content-identified file side and its declaration and planning coverage.
pub struct DefinitionPlanFile {
    pub id: usize,
    pub path: PathBuf,
    pub snapshot: SourceRevision,
    pub sha256: String,
    pub extraction: DefinitionStatus,
    pub definitions: usize,
    pub costed_definitions: usize,
    pub planning_omitted_definitions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// One selected declaration with source ranges, planning role and environment evidence.
pub struct PlannedDefinition {
    pub file: usize,
    pub definition: usize,
    pub name: String,
    pub kind: String,
    pub declaration_span: SourceSpan,
    pub source_span: SourceSpan,
    pub tokens: usize,
    pub role: String,
    pub reasons: Vec<String>,
    pub environment_gaps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// A bounded explanation for a seed or candidate that could not be selected.
pub struct DefinitionPlanOmission {
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub reason: String,
    pub explicit: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Advertised definition-planning limits, supported environment forms and explicit source option.
pub struct DefinitionPlanCapability {
    pub command: String,
    pub available: bool,
    pub strategy_version: u32,
    pub formats: Vec<String>,
    pub snapshots: Vec<String>,
    pub selectors: Vec<String>,
    pub default_context_budget: usize,
    pub min_context_budget: usize,
    pub max_context_budget: usize,
    pub default_token_budget: usize,
    pub min_token_budget: usize,
    pub max_token_budget: usize,
    pub default_byte_budget: usize,
    pub min_byte_budget: usize,
    pub max_byte_budget: usize,
    pub default_files: usize,
    pub default_definitions: usize,
    pub max_files: usize,
    pub max_definitions: usize,
    pub max_costed_definitions_per_file: usize,
    pub environment_forms: Vec<String>,
    pub environment_languages: Vec<String>,
    pub max_environment_definitions: usize,
    pub source_flag: String,
}
