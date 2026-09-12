use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
/// The explicitly selected or detected external diagnostic input format.
pub enum TaskDiagnosticFormat {
    #[default]
    Auto,
    Sarif,
    RustcJson,
    Text,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
/// Normalized producer severity; unknown means no supported severity was established.
pub enum TaskDiagnosticSeverity {
    Error,
    Warning,
    Note,
    Info,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
/// Whether an input location resolves within the target, lies outside its scope or remains unresolved.
pub enum TaskDiagnosticStatus {
    Resolved,
    OutOfScope,
    #[default]
    Unresolved,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
/// A bounded normalized external diagnostic and its report-local location evidence, not a health finding.
pub struct TaskDiagnostic {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_column: Option<u32>,
    pub severity: TaskDiagnosticSeverity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    pub message: String,
    pub confidence: String,
    pub status: TaskDiagnosticStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
/// External-input parsing and resolution coverage with separately capped diagnostic details.
pub struct TaskDiagnosticEvidence {
    pub format: TaskDiagnosticFormat,
    pub status: String,
    pub bytes_read: usize,
    pub parsed_records: usize,
    pub deduplicated_records: usize,
    pub resolved_records: usize,
    pub unresolved_records: usize,
    pub out_of_scope_records: usize,
    pub parse_errors: usize,
    pub ignored_records: usize,
    pub input_truncated: bool,
    pub records_truncated: bool,
    pub omitted_records: usize,
    pub omitted_records_exact: bool,
    pub omitted_details: usize,
    pub diagnostics: Vec<TaskDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Diagnostic input-byte, record and serialized-detail bounds for one profile class.
pub struct TaskDiagnosticLimitCapability {
    pub input_bytes: usize,
    pub records: usize,
    pub details: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Advertised diagnostic inputs, formats, normalization limits and report-local identity semantics.
pub struct TaskDiagnosticCapability {
    pub flag: String,
    pub format_flag: String,
    pub formats: Vec<String>,
    pub input_sources: Vec<String>,
    pub implies_context: bool,
    pub normal_limits: TaskDiagnosticLimitCapability,
    pub safe_limits: TaskDiagnosticLimitCapability,
    pub max_probe_bytes: usize,
    pub max_message_chars: usize,
    pub max_tool_or_code_chars: usize,
    pub max_path_bytes: usize,
    pub id_scope: String,
}
