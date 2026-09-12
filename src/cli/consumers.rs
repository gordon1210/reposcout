use super::{CommonArgs, parse_source_byte_budget, parse_source_token_budget};
use clap::{Args, ValueEnum};
use std::path::PathBuf;

/// Select incoming, outgoing or bidirectional call/reference traversal.
#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub enum ConsumersDirectionArg {
    #[default]
    Incoming,
    Outgoing,
    Both,
}

impl From<ConsumersDirectionArg> for crate::model::ConsumersDirection {
    fn from(value: ConsumersDirectionArg) -> Self {
        match value {
            ConsumersDirectionArg::Incoming => Self::Incoming,
            ConsumersDirectionArg::Outgoing => Self::Outgoing,
            ConsumersDirectionArg::Both => Self::Both,
        }
    }
}

/// Inspect bounded call and reference reachability from explicit or changed worktree definitions.
#[derive(Args, Debug, Clone)]
pub struct ConsumersArgs {
    /// Repository or path defining the consumer-query scope
    #[arg(default_value = ".")]
    pub path: PathBuf,
    #[command(flatten)]
    pub common: CommonArgs,
    /// Select a definition by file and exact symbol name; repeat to share one budget
    #[arg(long, value_names = ["FILE", "SYMBOL"], num_args = 2, action = clap::ArgAction::Append)]
    pub symbol: Vec<String>,
    /// Select the innermost declaration containing a one-based file line; repeatable
    #[arg(long, value_names = ["FILE", "LINE"], num_args = 2, action = clap::ArgAction::Append)]
    pub line: Vec<String>,
    /// Require the selected file to match this SHA-256 hash; mismatches return no source
    #[arg(long, value_names = ["FILE", "SHA256"], num_args = 2, action = clap::ArgAction::Append)]
    pub expect_hash: Vec<String>,
    /// Follow incoming consumers, outgoing dependencies, or both directions
    #[arg(long, value_enum, default_value = "incoming")]
    pub direction: ConsumersDirectionArg,
    /// Maximum relation depth from the selected seeds
    #[arg(long, default_value_t = 1)]
    pub depth: usize,
    /// Maximum uniquely reached symbols before output-budget admission
    #[arg(long, default_value_t = 20)]
    pub limit: usize,
    /// Maximum distinct source paths in the consumer results
    #[arg(long, default_value_t = 20)]
    pub path_limit: usize,
    /// Maximum tokens in the complete rendered output, including metadata and the final newline
    #[arg(long, default_value_t = 4096, value_parser = parse_source_token_budget)]
    pub budget: usize,
    /// Maximum bytes in the complete rendered output, including metadata and the final newline
    #[arg(long, default_value_t = 65_536, value_parser = parse_source_byte_budget)]
    pub max_output_bytes: usize,
}
