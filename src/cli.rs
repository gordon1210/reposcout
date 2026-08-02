//! Command-line interface. The default invocation (`reposcout [PATH]`) runs a
//! full scan; focused subcommands narrow the analyzer set.

use crate::dup::{DuplicationFormatScope, DuplicationMode};
use crate::lang::{HealthInclude, HealthScope};
use clap::{Args, Parser, Subcommand, ValueEnum};
use std::net::IpAddr;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "reposcout",
    version,
    about = "Fast repository scout: tokens, complexity, duplication & health metrics",
    args_conflicts_with_subcommands = true
)]
pub struct Cli {
    /// Error rendering for automation (`json` emits one object on stderr)
    #[arg(long, value_enum, global = true, default_value_t = ErrorFormat::Text)]
    pub error_format: ErrorFormat,

    /// Pretty-print JSON output instead of the compact default
    #[arg(long, global = true)]
    pub pretty: bool,

    /// Write flushed NDJSON diagnostics for slow or crashing runs
    #[arg(long, value_name = "FILE", global = true)]
    pub debug_log: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Command>,

    #[command(flatten)]
    pub args: ScanArgs,
}

#[derive(ValueEnum, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ErrorFormat {
    #[default]
    Text,
    Json,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Describe stable commands, formats, profiles, and language coverage
    Capabilities(CapabilitiesArgs),
    /// Inspect or clear `RepoScout`'s OS-managed caches
    Cache(CacheArgs),
    /// Token counts only
    Tokens(ScanArgs),
    /// Complexity metrics only
    Complexity(ScanArgs),
    /// Duplication detection only
    Dup(ScanArgs),
    /// Git churn / hotspots only
    Churn(ScanArgs),
    /// Line metrics, language breakdown, markers & imports
    Metrics(ScanArgs),
    /// Explain why one file matters using full-repository context
    Explain(ExplainArgs),
    /// Locate declarations by symbol name across first-class languages
    Locate(LocateArgs),
    /// Update an installer-managed copy from the latest stable GitHub release
    Update,
    /// Show layered global/project configuration and effective values
    Config(ConfigArgs),
    /// Serve live scan results over HTTP and watch for repository changes
    Daemon(DaemonArgs),
}

#[derive(Args, Debug, Clone)]
pub struct CacheArgs {
    #[command(subcommand)]
    pub command: CacheCommand,
}

#[derive(Subcommand, Debug, Clone)]
pub enum CacheCommand {
    /// Clear cached analysis and Git-history facts
    Clear(CacheClearArgs),
}

#[derive(Args, Debug, Clone)]
pub struct CacheClearArgs {
    /// Repository, directory, or file whose scan-root cache should be cleared
    #[arg(conflicts_with = "all")]
    pub path: Option<PathBuf>,

    /// Clear every cache managed by `RepoScout`
    #[arg(long)]
    pub all: bool,
}

#[derive(Args, Debug, Clone)]
pub struct CapabilitiesArgs {
    /// Output format [default: table on a TTY, else json]
    #[arg(short, long, value_enum)]
    pub format: Option<ConfigOutputFormat>,
}

#[derive(Args, Debug, Clone)]
pub struct ConfigArgs {
    /// Path used to discover the nearest project configuration
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// Output format [default: table on a TTY, else json]
    #[arg(short, long, value_enum)]
    pub format: Option<ConfigOutputFormat>,

    /// Built-in execution profile whose effective settings should be inspected
    #[arg(long, value_enum, default_value_t = ExecutionProfile::Full)]
    pub profile: ExecutionProfile,

    /// Ignore the nearest repository-owned reposcout configuration
    #[arg(long)]
    pub no_project_config: bool,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigOutputFormat {
    Table,
    Json,
}

#[derive(Args, Debug, Clone)]
pub struct DaemonArgs {
    /// Path to scan and watch (repo root, subdirectory, or file)
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// Address to bind the HTTP server to
    #[arg(long, default_value = "127.0.0.1")]
    pub host: IpAddr,

    /// Port to bind the HTTP server to
    #[arg(long, default_value_t = 7331)]
    pub port: u16,

    /// Delay used to coalesce bursts of filesystem events
    #[arg(long, default_value_t = 300, value_name = "MS")]
    pub debounce_ms: u64,

    /// Analyzer profile: lite omits expensive whole-repository duplication and churn
    #[arg(long, value_enum, default_value_t = DaemonProfile::Full)]
    pub profile: DaemonProfile,

    /// Ignore the nearest repository-owned reposcout configuration
    #[arg(long)]
    pub no_project_config: bool,

    /// Disable the per-start bearer token (explicitly unauthenticated mode)
    #[arg(long)]
    pub unsafe_no_auth: bool,

    /// Allow non-loopback binding over plain HTTP (use only behind a TLS proxy)
    #[arg(long)]
    pub allow_insecure_remote: bool,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonProfile {
    Lite,
    Full,
    Safe,
}

#[derive(Args, Debug, Clone)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "Clap presence-only switches are independent boolean CLI inputs, not a programmatic options interface"
)]
pub struct CommonArgs {
    /// Output format [default: table on a TTY, else json]
    #[arg(short, long, value_enum)]
    pub format: Option<OutputFormat>,

    /// Write output to a file instead of stdout
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Built-in execution profile: full analysis, cheap agent scouting, or
    /// bounded scouting that ignores repository-owned configuration
    #[arg(long, value_enum)]
    pub profile: Option<ExecutionProfile>,

    /// Ignore the nearest repository-owned reposcout configuration
    #[arg(long)]
    pub no_project_config: bool,

    /// tiktoken encoding (`o200k_base` or `cl100k_base`)
    #[arg(long)]
    pub encoding: Option<String>,

    /// Extra ignore glob (repeatable)
    #[arg(long = "exclude")]
    pub exclude: Vec<String>,

    /// Include hidden files
    #[arg(long)]
    pub hidden: bool,

    /// Do not respect .gitignore
    #[arg(long = "no-ignore")]
    pub no_ignore: bool,

    /// Include dependency lockfiles (skipped by default)
    #[arg(long = "include-lockfiles")]
    pub include_lockfiles: bool,

    /// Number of worker threads [default: CPU count]
    #[arg(short = 'j', long)]
    pub jobs: Option<usize>,

    /// Maximum bytes accepted from one recognized worktree file
    #[arg(long, value_name = "BYTES")]
    pub max_file_bytes: Option<u64>,

    /// Maximum aggregate bytes accepted during one discovery pass
    #[arg(long, value_name = "BYTES")]
    pub max_total_bytes: Option<u64>,

    /// Maximum filesystem entries accepted during one discovery pass
    #[arg(long, value_name = "N")]
    pub max_files: Option<usize>,

    /// Maximum bytes accepted from one Git blob during deep review
    #[arg(long, value_name = "BYTES")]
    pub max_git_blob_bytes: Option<u64>,

    /// Cooperative wall-clock budget for one scan
    #[arg(long, value_name = "SECONDS")]
    pub max_scan_seconds: Option<u64>,

    /// Disable the incremental cache
    #[arg(long = "no-cache")]
    pub no_cache: bool,

    /// Maximum cyclomatic complexity per function before it is flagged [default: 20]
    #[arg(long = "max-complexity", value_name = "N")]
    pub max_complexity: Option<u32>,

    /// Duplication trivia mode used for related clone findings
    #[arg(long = "dup-mode", value_enum)]
    pub duplication_mode: Option<DuplicationMode>,

    /// Duplication format scope used for related clone findings
    #[arg(long = "dup-format-scope", value_enum)]
    pub duplication_format_scope: Option<DuplicationFormatScope>,

    /// Starting file scope for actionable health analysis
    #[arg(long = "health-scope", value_enum)]
    pub health_scope: Option<HealthScope>,

    /// Add a non-source format to health analysis (repeatable)
    #[arg(long = "health-include", value_enum)]
    pub health_includes: Vec<HealthInclude>,

    /// Exclude a repository-relative path glob from health analysis (repeatable)
    #[arg(long = "health-exclude", value_name = "GLOB")]
    pub health_excludes: Vec<String>,

    /// Suppress the progress bar
    #[arg(short, long)]
    pub quiet: bool,
}

#[derive(ValueEnum, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ExecutionProfile {
    #[default]
    Full,
    Agent,
    Safe,
}

impl ExecutionProfile {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Agent => "agent",
            Self::Safe => "safe",
        }
    }
}

#[derive(Args, Debug, Clone)]
pub struct ExplainArgs {
    /// File to explain
    pub file: PathBuf,

    #[command(flatten)]
    pub common: CommonArgs,
}

#[derive(Args, Debug, Clone)]
pub struct LocateArgs {
    /// Qualified or simple symbol name to find
    pub symbol: String,

    /// Repository, directory, or file to search
    #[arg(default_value = ".")]
    pub path: PathBuf,

    #[command(flatten)]
    pub common: CommonArgs,

    /// Require an exact qualified or simple name match
    #[arg(long)]
    pub exact: bool,

    /// Restrict matches to one declaration kind (class, method, trait, ...)
    #[arg(long)]
    pub kind: Option<String>,

    /// Restrict matches to one detected language
    #[arg(long)]
    pub language: Option<String>,

    /// Maximum returned matches (1..=100)
    #[arg(long, default_value_t = 20)]
    pub limit: usize,
}

#[derive(Args, Debug, Clone)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "Clap presence-only switches are independent boolean CLI inputs, not a programmatic options interface"
)]
pub struct ScanArgs {
    /// Path to scan (repo root, subdirectory, or file)
    #[arg(default_value = ".")]
    pub path: PathBuf,

    #[command(flatten)]
    pub common: CommonArgs,

    /// Restrict to a subset of analyzers (comma-separated):
    /// tokens,complexity,imports,markers,dup,churn
    #[arg(long, value_delimiter = ',')]
    pub only: Vec<String>,

    /// Length of top-N lists
    #[arg(long)]
    pub top: Option<usize>,

    /// CI gate expression, e.g. "max-cyclomatic>30,duplicated-pct>5,min-mi<50"
    #[arg(long = "fail-on")]
    pub fail_on: Option<String>,

    /// Emit only the aggregate summary in JSON (omit the per-file and
    /// duplicate arrays). Ideal for agents scouting a path cheaply.
    #[arg(long)]
    pub summary: bool,

    /// Emit a bounded, change-focused decision report. Requires exactly one
    /// diff scope, defaults to the agent profile, and implies context/impact.
    #[arg(long = "change-summary", conflicts_with = "no_context")]
    pub change_summary: bool,

    /// Emit compact JSON suitable for complete aggregate and finding-level
    /// baseline comparisons.
    #[arg(long = "baseline-ready")]
    pub baseline_ready: bool,

    /// Include bounded source snippets in full duplicate findings
    #[arg(long = "dup-snippets")]
    pub duplication_report_snippets: bool,

    /// Show precise pair-oriented duplicate findings in table/Markdown output
    #[arg(long = "dup-details")]
    pub duplication_details: bool,

    /// Group results by directory; optionally set the directory depth (default 1),
    /// e.g. --by-dir or --by-dir=2.
    #[arg(
        long = "by-dir",
        value_name = "DEPTH",
        num_args = 0..=1,
        default_missing_value = "1",
        require_equals = true
    )]
    pub by_dir: Option<usize>,

    /// Restrict the scan to files changed since this git ref (commit/branch/tag)
    #[arg(long, value_name = "REF", conflicts_with_all = ["staged", "working"])]
    pub since: Option<String>,

    /// Restrict the scan to staged (index) changes
    #[arg(long, conflicts_with_all = ["since", "working"])]
    pub staged: bool,

    /// Restrict the scan to uncommitted working-tree changes
    #[arg(long, conflicts_with_all = ["since", "staged"])]
    pub working: bool,

    /// Compare against a previously saved JSON report and show deltas
    #[arg(long, value_name = "FILE")]
    pub baseline: Option<PathBuf>,

    /// Exit with code 2 if metrics regressed versus the baseline
    #[arg(long = "fail-on-regression", requires = "baseline")]
    pub fail_on_regression: bool,

    /// Build an import and explicit type-relationship graph for every first-class
    /// language, with fan-in/out, cycles, and orphan candidate detection
    #[arg(long)]
    pub graph: bool,

    /// Restrict graph output to a file/directory and its bounded neighborhood;
    /// repeatable and enables graph analysis
    #[arg(long = "graph-focus", value_name = "PATH")]
    pub graph_focus: Vec<PathBuf>,

    /// Maximum graph hops from each focus path (0 selects only focus files)
    #[arg(long = "graph-depth", value_name = "N")]
    pub graph_depth: Option<usize>,

    /// Traverse imports, reverse dependents, or both from graph focus paths
    #[arg(long = "graph-direction", value_enum)]
    pub graph_direction: Option<GraphDirection>,

    /// Build a deterministic, structural, token-budgeted reading plan for agents;
    /// a diff scope automatically seeds it from changed paths
    #[arg(long)]
    pub context: bool,

    /// Disable context planning enabled by a configuration file
    #[arg(long = "no-context", conflicts_with = "context")]
    pub no_context: bool,

    /// Token budget for the context plan (also enables context planning)
    #[arg(
        long = "context-budget",
        value_name = "TOKENS",
        conflicts_with = "no_context"
    )]
    pub context_budget: Option<usize>,

    /// Maximum selected files in the context plan
    #[arg(
        long = "context-max-files",
        value_name = "N",
        conflicts_with = "no_context"
    )]
    pub context_max_files: Option<usize>,

    /// Prioritize a repo-relative file or directory and its graph neighborhood
    #[arg(long, value_name = "PATH", conflicts_with = "no_context")]
    pub focus: Vec<PathBuf>,

    /// For a diff-scoped scan, report direct and transitive internal dependents
    /// of changed files supported by the first-class-language graph
    #[arg(long)]
    pub impact: bool,

    /// Review actionable findings that intersect changed lines. Bare
    /// `--review` uses `lines`; `--review=deep` compares both Git snapshots.
    #[arg(
        long,
        value_enum,
        num_args = 0..=1,
        default_missing_value = "lines",
        require_equals = true
    )]
    pub review: Option<ReviewMode>,

    /// Exit with code 2 when review finds current/new/worsened issues.
    #[arg(long = "fail-on-review", requires = "review")]
    pub fail_on_review: bool,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Table,
    Json,
    Markdown,
    Sarif,
    Ndjson,
    Dot,
    Mermaid,
}

#[derive(ValueEnum, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GraphDirection {
    Dependencies,
    Dependents,
    #[default]
    Both,
}

impl GraphDirection {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dependencies => "dependencies",
            Self::Dependents => "dependents",
            Self::Both => "both",
        }
    }
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewMode {
    Lines,
    Deep,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_by_dir_does_not_consume_the_scan_path() {
        let cli = Cli::try_parse_from(["reposcout", "--by-dir", "src"]).unwrap();
        assert_eq!(cli.args.by_dir, Some(1));
        assert_eq!(cli.args.path, PathBuf::from("src"));

        let cli = Cli::try_parse_from(["reposcout", "--by-dir=2", "src"]).unwrap();
        assert_eq!(cli.args.by_dir, Some(2));
        assert_eq!(cli.args.path, PathBuf::from("src"));
    }

    #[test]
    fn fail_on_regression_requires_a_baseline() {
        assert!(Cli::try_parse_from(["reposcout", "--fail-on-regression"]).is_err());
        assert!(
            Cli::try_parse_from([
                "reposcout",
                "--fail-on-regression",
                "--baseline",
                "baseline.json"
            ])
            .is_ok()
        );
    }

    #[test]
    fn scan_and_explain_share_common_options() {
        let scan = Cli::try_parse_from([
            "reposcout",
            "--encoding",
            "cl100k_base",
            "--exclude",
            "vendor/**",
            "src",
        ])
        .unwrap();
        assert_eq!(scan.args.common.encoding.as_deref(), Some("cl100k_base"));
        assert_eq!(scan.args.common.exclude, ["vendor/**"]);

        let explain = Cli::try_parse_from([
            "reposcout",
            "explain",
            "src/main.rs",
            "--encoding",
            "cl100k_base",
            "--exclude",
            "vendor/**",
        ])
        .unwrap();
        let Command::Explain(explain) = explain.command.unwrap() else {
            panic!("expected explain command");
        };
        assert_eq!(explain.common.encoding.as_deref(), Some("cl100k_base"));
        assert_eq!(explain.common.exclude, ["vendor/**"]);
    }

    #[test]
    fn debug_log_is_a_global_option() {
        let scan = Cli::try_parse_from([
            "reposcout",
            "src",
            "--debug-log",
            "/tmp/reposcout-debug.jsonl",
        ])
        .unwrap();
        assert_eq!(
            scan.debug_log,
            Some(PathBuf::from("/tmp/reposcout-debug.jsonl"))
        );

        let locate = Cli::try_parse_from([
            "reposcout",
            "locate",
            "HttpClient",
            "src",
            "--debug-log",
            "/tmp/reposcout-locate.jsonl",
        ])
        .unwrap();
        assert_eq!(
            locate.debug_log,
            Some(PathBuf::from("/tmp/reposcout-locate.jsonl"))
        );
    }

    #[test]
    fn pretty_is_a_global_option() {
        let scan = Cli::try_parse_from(["reposcout", "--pretty", "-f", "json", "src"]).unwrap();
        assert!(scan.pretty);

        let locate = Cli::try_parse_from([
            "reposcout",
            "locate",
            "HttpClient",
            "src",
            "-f",
            "json",
            "--pretty",
        ])
        .unwrap();
        assert!(locate.pretty);
    }

    #[test]
    fn locate_accepts_ranked_symbol_filters() {
        let cli = Cli::try_parse_from([
            "reposcout",
            "locate",
            "HttpClient.request",
            "src",
            "--kind",
            "method",
            "--language",
            "PHP",
            "--limit",
            "7",
            "-f",
            "json",
        ])
        .unwrap();
        let Command::Locate(locate) = cli.command.unwrap() else {
            panic!("expected locate command");
        };
        assert_eq!(locate.symbol, "HttpClient.request");
        assert_eq!(locate.path, PathBuf::from("src"));
        assert_eq!(locate.kind.as_deref(), Some("method"));
        assert_eq!(locate.language.as_deref(), Some("PHP"));
        assert_eq!(locate.limit, 7);
        assert_eq!(locate.common.format, Some(OutputFormat::Json));
    }

    #[test]
    fn cache_clear_defaults_to_the_current_path_and_requires_explicit_all_scope() {
        let cli = Cli::try_parse_from(["reposcout", "cache", "clear"]).unwrap();
        let Command::Cache(cache) = cli.command.unwrap() else {
            panic!("expected cache command");
        };
        let CacheCommand::Clear(clear) = cache.command;
        assert_eq!(clear.path, None);
        assert!(!clear.all);

        let cli = Cli::try_parse_from(["reposcout", "cache", "clear", "src"]).unwrap();
        let Command::Cache(cache) = cli.command.unwrap() else {
            panic!("expected cache command");
        };
        let CacheCommand::Clear(clear) = cache.command;
        assert_eq!(clear.path, Some(PathBuf::from("src")));
        assert!(!clear.all);

        let cli = Cli::try_parse_from(["reposcout", "cache", "clear", "--all"]).unwrap();
        let Command::Cache(cache) = cli.command.unwrap() else {
            panic!("expected cache command");
        };
        let CacheCommand::Clear(clear) = cache.command;
        assert_eq!(clear.path, None);
        assert!(clear.all);

        assert!(Cli::try_parse_from(["reposcout", "cache", "clear", "src", "--all"]).is_err());
    }

    #[test]
    fn daemon_defaults_to_localhost_and_accepts_overrides() {
        let cli = Cli::try_parse_from(["reposcout", "daemon"]).unwrap();
        let Command::Daemon(daemon) = cli.command.unwrap() else {
            panic!("expected daemon command");
        };
        assert_eq!(daemon.host.to_string(), "127.0.0.1");
        assert_eq!(daemon.port, 7331);
        assert_eq!(daemon.debounce_ms, 300);
        assert_eq!(daemon.profile, DaemonProfile::Full);
        assert_eq!(daemon.path, PathBuf::from("."));
        assert!(!daemon.no_project_config);
        assert!(!daemon.unsafe_no_auth);
        assert!(!daemon.allow_insecure_remote);

        let cli = Cli::try_parse_from([
            "reposcout",
            "daemon",
            "src",
            "--host",
            "0.0.0.0",
            "--port",
            "9000",
            "--debounce-ms",
            "75",
            "--profile",
            "lite",
            "--no-project-config",
            "--unsafe-no-auth",
            "--allow-insecure-remote",
        ])
        .unwrap();
        let Command::Daemon(daemon) = cli.command.unwrap() else {
            panic!("expected daemon command");
        };
        assert_eq!(daemon.host.to_string(), "0.0.0.0");
        assert_eq!(daemon.port, 9000);
        assert_eq!(daemon.debounce_ms, 75);
        assert_eq!(daemon.profile, DaemonProfile::Lite);
        assert_eq!(daemon.path, PathBuf::from("src"));
        assert!(daemon.no_project_config);
        assert!(daemon.unsafe_no_auth);
        assert!(daemon.allow_insecure_remote);

        let cli = Cli::try_parse_from(["reposcout", "daemon", "--profile", "safe"]).unwrap();
        let Command::Daemon(daemon) = cli.command.unwrap() else {
            panic!("expected daemon command");
        };
        assert_eq!(daemon.profile, DaemonProfile::Safe);
    }

    #[test]
    fn config_accepts_a_discovery_path_and_machine_format() {
        let cli = Cli::try_parse_from(["reposcout", "config", "src", "-f", "json"]).unwrap();
        let Command::Config(config) = cli.command.unwrap() else {
            panic!("expected config command");
        };
        assert_eq!(config.path, PathBuf::from("src"));
        assert_eq!(config.format, Some(ConfigOutputFormat::Json));
    }

    #[test]
    fn graph_query_and_export_options_parse() {
        let cli = Cli::try_parse_from([
            "reposcout",
            "--graph-focus",
            "src/main.ts",
            "--graph-depth",
            "2",
            "--graph-direction",
            "dependents",
            "-f",
            "mermaid",
        ])
        .unwrap();

        assert_eq!(cli.args.graph_focus, [PathBuf::from("src/main.ts")]);
        assert_eq!(cli.args.graph_depth, Some(2));
        assert_eq!(cli.args.graph_direction, Some(GraphDirection::Dependents));
        assert_eq!(cli.args.common.format, Some(OutputFormat::Mermaid));
    }
}
