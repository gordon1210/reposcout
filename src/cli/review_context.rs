use super::{CommonArgs, parse_source_byte_budget, parse_source_token_budget};
use clap::Args;
use std::path::PathBuf;

/// Prepare PR review facts without choosing agents or changing the user's tokenizer.
#[derive(Args, Debug, Clone)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent opt-in CLI flags preserve explicit comparison and content choices"
)]
pub struct ReviewContextArgs {
    /// Repository or directory whose changed paths seed repository-wide impact
    #[arg(default_value = ".")]
    pub path: PathBuf,
    #[command(flatten)]
    pub common: CommonArgs,
    /// Commit or reference used as the comparison base
    #[arg(long, value_name = "REF")]
    pub base: String,
    /// Commit or reference to review; local/index changes are never included
    #[arg(long, default_value = "HEAD", value_name = "REF")]
    pub head: String,
    /// Compare the unique merge base of --base and --head with --head
    #[arg(long)]
    pub merge_base: bool,
    /// Select an initial reading list using the configured context budget and file limit
    #[arg(long)]
    pub context: bool,
    /// Override the source-token budget for the optional reading list (implies --context)
    #[arg(long)]
    pub context_budget: Option<usize>,
    /// Override the number of selected file sides (implies --context)
    #[arg(long)]
    pub context_max_files: Option<usize>,
    /// Include complete selected source files within the shared output budget (implies --context)
    #[arg(long)]
    pub source: bool,
    /// Include captured unified diffs within the shared output budget
    #[arg(long)]
    pub diff: bool,
    /// Maximum tokens in the complete rendered response, using the configured encoding
    #[arg(long, default_value_t = 4096, value_parser = parse_source_token_budget)]
    pub budget: usize,
    /// Maximum bytes in the complete rendered response
    #[arg(long, default_value_t = 65_536, value_parser = parse_source_byte_budget)]
    pub max_output_bytes: usize,
}
