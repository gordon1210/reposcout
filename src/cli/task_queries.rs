use super::{CommonArgs, parse_source_byte_budget, parse_source_token_budget};
use clap::{Args, ValueEnum};
use std::path::PathBuf;

#[derive(ValueEnum, Debug, Clone, Copy, Default)]
pub enum FindMatchArg {
    #[default]
    All,
    Any,
}

impl From<FindMatchArg> for crate::model::FindMatchMode {
    fn from(value: FindMatchArg) -> Self {
        match value {
            FindMatchArg::All => Self::All,
            FindMatchArg::Any => Self::Any,
        }
    }
}

#[derive(ValueEnum, Debug, Clone, Copy, Default)]
pub enum TaskDiagnosticFormatArg {
    #[default]
    Auto,
    Sarif,
    RustcJson,
    Text,
}

impl From<TaskDiagnosticFormatArg> for crate::model::TaskDiagnosticFormat {
    fn from(value: TaskDiagnosticFormatArg) -> Self {
        match value {
            TaskDiagnosticFormatArg::Auto => Self::Auto,
            TaskDiagnosticFormatArg::Sarif => Self::Sarif,
            TaskDiagnosticFormatArg::RustcJson => Self::RustcJson,
            TaskDiagnosticFormatArg::Text => Self::Text,
        }
    }
}

/// Find declaration candidates by lexical evidence under shared output limits.
#[derive(Args, Debug, Clone)]
pub struct FindArgs {
    /// Lexical terms to match across names, paths, signatures, comments and bounded code
    pub query: String,

    /// Repository or path defining the search scope
    #[arg(default_value = ".")]
    pub path: PathBuf,

    #[command(flatten)]
    pub common: CommonArgs,

    /// Require all query terms or any query term to match a candidate
    #[arg(long = "match", value_enum, default_value_t = FindMatchArg::All)]
    pub match_mode: FindMatchArg,

    /// Filter by exact declaration kind, ignoring case
    #[arg(long)]
    pub kind: Option<String>,

    /// Filter by exact language name, ignoring case
    #[arg(long)]
    pub language: Option<String>,

    /// Maximum candidate hits before token and byte budget admission
    #[arg(long, default_value_t = 20)]
    pub limit: usize,

    /// Maximum tokens in the complete rendered output, including metadata and the final newline
    #[arg(long, default_value_t = 4096, value_parser = parse_source_token_budget)]
    pub budget: usize,

    /// Maximum bytes in the complete rendered output, including metadata and the final newline
    #[arg(long, default_value_t = 65_536, value_parser = parse_source_byte_budget)]
    pub max_output_bytes: usize,
}

/// Plan known definitions and supported environment from a selected source snapshot.
#[derive(Args, Debug, Clone)]
pub struct PlanArgs {
    /// Repository or directory containing the selected files
    #[arg(default_value = ".")]
    pub path: PathBuf,

    #[command(flatten)]
    pub common: CommonArgs,

    /// Plan worktree, index, or a Git revision resolved once for this query
    #[arg(long, default_value = "worktree", value_name = "SNAPSHOT")]
    pub snapshot: String,

    /// Seed a definition by file and exact symbol name; repeat to share one plan
    #[arg(long, value_names = ["FILE", "SYMBOL"], num_args = 2, action = clap::ArgAction::Append)]
    pub symbol: Vec<String>,

    /// Seed the innermost declaration containing a one-based file line; repeatable
    #[arg(long, value_names = ["FILE", "LINE"], num_args = 2, action = clap::ArgAction::Append)]
    pub line: Vec<String>,

    /// Seed declarations in a selected file without requesting the entire file source; repeatable
    #[arg(long, value_name = "FILE")]
    pub file: Vec<PathBuf>,

    /// Require the selected file to match this SHA-256 hash; mismatches cannot supply source
    #[arg(long, value_names = ["FILE", "SHA256"], num_args = 2, action = clap::ArgAction::Append)]
    pub expect_hash: Vec<String>,

    /// Maximum source tokens selected by the plan after overlapping ranges are counted once
    #[arg(long, default_value_t = 12_000)]
    pub context_budget: usize,

    /// Maximum tokens in the complete rendered output, including metadata and the final newline
    #[arg(long, default_value_t = 4096, value_parser = parse_source_token_budget)]
    pub budget: usize,

    /// Maximum bytes in the complete rendered output, including metadata and the final newline
    #[arg(long, default_value_t = 65_536, value_parser = parse_source_byte_budget)]
    pub max_output_bytes: usize,

    /// Maximum distinct files selected by the definition plan
    #[arg(long, default_value_t = 8)]
    pub max_plan_files: usize,

    /// Maximum definitions selected by the plan
    #[arg(long, default_value_t = 16)]
    pub max_definitions: usize,

    /// Include complete selected source ranges within the shared output budget
    #[arg(long)]
    pub source: bool,
}
