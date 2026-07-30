//! Runtime configuration: built-in defaults, an optional OS-level global file,
//! the nearest project file, and the set of enabled analyzers. Later layers
//! override only the fields they explicitly define; CLI flags are applied by
//! `main` after file resolution.

use crate::dup::{DuplicationFormatScope, DuplicationMode};
use crate::lang::{self, HealthInclude, HealthScope, LangInfo};
use anyhow::{Context, Result};
use directories::ProjectDirs;
use ignore::overrides::{Override, OverrideBuilder};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};

pub const GLOBAL_CONFIG_ENV: &str = "REPOSCOUT_GLOBAL_CONFIG";
pub const ABSOLUTE_MAX_JOBS: usize = 64;
pub const ABSOLUTE_MAX_TOP: usize = 1_000;
pub const ABSOLUTE_MAX_CHURN_COMMITS: usize = 100_000;
pub const ABSOLUTE_MIN_DUP_TOKENS: usize = 8;
pub const ABSOLUTE_MIN_DUP_SIMILARITY: f64 = 0.5;
pub const ABSOLUTE_MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
pub const ABSOLUTE_MAX_TOTAL_BYTES: u64 = 4 * 1024 * 1024 * 1024;
pub const ABSOLUTE_MAX_FILES: usize = 500_000;
pub const ABSOLUTE_MAX_GIT_BLOB_BYTES: u64 = 256 * 1024 * 1024;
pub const ABSOLUTE_MAX_SCAN_SECONDS: u64 = 7_200;
pub const ABSOLUTE_MAX_CONTEXT_TOKENS: usize = 5_000_000;
pub const ABSOLUTE_MAX_CONTEXT_FILES: usize = 10_000;
pub const ABSOLUTE_MAX_CHURN_DELTAS_PER_COMMIT: usize = 250_000;
pub const ABSOLUTE_MAX_CHURN_TOTAL_DELTAS: usize = 2_000_000;
pub const ABSOLUTE_MAX_CHURN_OUTPUT_BYTES: u64 = 256 * 1024 * 1024;
pub const ABSOLUTE_MAX_GIT_PATH_BYTES: usize = 16_384;
pub const ABSOLUTE_MAX_CHURN_CACHE_BYTES: u64 = 256 * 1024 * 1024;
pub const ABSOLUTE_MAX_IGNORE_FILE_BYTES: u64 = 4 * 1024 * 1024;
pub const ABSOLUTE_MAX_IGNORE_LINES: usize = 200_000;
pub const ABSOLUTE_MAX_IGNORE_LINE_BYTES: usize = 32_768;
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

fn is_false(value: &bool) -> bool {
    !*value
}

/// Which analyzers to run. Subcommands and `--only` narrow this set.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Enabled {
    pub tokens: bool,
    pub lines: bool,
    pub complexity: bool,
    pub imports: bool,
    pub markers: bool,
    pub duplication: bool,
    pub churn: bool,
}

impl Default for Enabled {
    fn default() -> Self {
        Enabled {
            tokens: true,
            lines: true,
            complexity: true,
            imports: true,
            markers: true,
            duplication: true,
            churn: true,
        }
    }
}

impl Enabled {
    pub fn none() -> Self {
        Enabled {
            tokens: false,
            lines: false,
            complexity: false,
            imports: false,
            markers: false,
            duplication: false,
            churn: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    /// tiktoken encoding name, e.g. "o200k_base" or "cl100k_base".
    pub encoding: String,
    pub jobs: usize,
    pub use_cache: bool,
    /// Length of "top N" lists (findings, hotspots, token-heavy files, clone groups).
    pub top: usize,
    /// Maximum cyclomatic complexity allowed per function before it is flagged.
    pub max_complexity: u32,
    pub include_hidden: bool,
    pub respect_gitignore: bool,
    /// Skip common dependency lockfiles (Cargo.lock, package-lock.json, ...).
    pub exclude_lockfiles: bool,
    pub extra_excludes: Vec<String>,
    pub markers: Vec<String>,
    /// Starting corpus for actionable health analysis. Inventory metrics
    /// always retain every recognized format.
    pub health_scope: HealthScope,
    /// Non-source formats added to the default source-health corpus.
    pub health_includes: Vec<HealthInclude>,
    /// Repository-relative path globs removed from health analysis after the
    /// scope and format includes have selected the corpus.
    pub health_excludes: Vec<String>,
    /// Minimum token run length for duplication detection.
    pub min_dup_tokens: usize,
    /// Minimum line span for a reported clone (filters single-line noise).
    pub min_dup_lines: usize,
    /// Minimum similarity [0,1] for a near-duplicate match.
    pub near_dup_min_similarity: f64,
    /// Trivia filtering for duplication tokens.
    pub duplication_mode: DuplicationMode,
    /// Which detected formats may share duplicate candidates.
    pub duplication_format_scope: DuplicationFormatScope,
    /// Include bounded source fragments in detailed duplicate findings.
    pub duplication_report_snippets: bool,
    /// Cap on commits walked for churn (0 selects the absolute ceiling).
    pub churn_max_commits: usize,
    /// Maximum tree deltas retained from one commit during churn collection.
    pub max_churn_deltas_per_commit: usize,
    /// Maximum tree deltas retained across the entire churn walk.
    pub max_churn_total_deltas: usize,
    /// Maximum native Git churn output bytes accepted for one collection.
    pub max_churn_output_bytes: u64,
    /// Maximum path length accepted from Git history events.
    pub max_git_path_bytes: usize,
    /// Maximum serialized churn-cache file size accepted on load or save.
    pub max_churn_cache_bytes: u64,
    /// Largest recognized worktree file accepted for analysis.
    pub max_file_bytes: u64,
    /// Aggregate recognized worktree bytes accepted for one discovery pass.
    pub max_total_bytes: u64,
    /// Maximum filesystem entries accepted by one discovery pass.
    pub max_files: usize,
    /// Largest Git blob accepted by deep review.
    pub max_git_blob_bytes: u64,
    /// Cooperative wall-clock budget for one scan.
    pub max_scan_seconds: u64,
    /// Whether repository-owned ignore files (`.gitignore`, `.ignore`,
    /// `.reposcoutignore`, and `.git/info/exclude`) are loaded.
    pub load_repository_ignores: bool,
    /// Maximum bytes accepted from one repository-owned ignore file.
    pub max_ignore_file_bytes: u64,
    /// Maximum non-empty pattern lines accepted from one ignore file.
    pub max_ignore_lines: usize,
    /// Maximum bytes accepted from one ignore pattern line.
    pub max_ignore_line_bytes: usize,
    /// Build an explainable, token-budgeted reading plan.
    pub context: bool,
    /// Maximum aggregate tokens in the reading plan.
    pub context_budget: usize,
    /// Maximum number of selected files in the reading plan.
    pub context_max_files: usize,
    /// Runtime focus paths supplied by the CLI.
    pub context_focus: Vec<PathBuf>,
    /// Suppress the progress bar.
    pub quiet_progress: bool,
    pub enabled: Enabled,
    /// When `Some(depth)`, produce a per-directory rollup at that depth.
    pub by_dir: Option<usize>,
    /// Restrict scan to files changed in a specific diff scope.
    pub diff_scope: Option<crate::git::DiffScope>,
    /// Compare current summary against this saved JSON report.
    pub baseline_path: Option<std::path::PathBuf>,
    /// Exit with code 2 when any metric regresses versus the baseline.
    pub fail_on_regression: bool,
    /// Build import and explicit type-relationship topology for every first-class
    /// language when true.
    pub graph: bool,
    /// Runtime focus files/directories for graph projection.
    pub graph_focus: Vec<PathBuf>,
    /// Maximum traversal distance from graph focus paths.
    pub graph_depth: usize,
    /// Edge direction followed by focused graph traversal.
    pub graph_direction: crate::cli::GraphDirection,
    /// Analyze the dependents of a diff-scoped change set.
    pub impact: bool,
    /// Assemble the bounded, change-focused decision report.
    pub change_summary: bool,
    /// Filter findings to changed lines, optionally comparing both snapshots.
    pub review: Option<crate::cli::ReviewMode>,
    /// Built-in runtime execution profile selected by the caller.
    pub execution_profile: String,
    /// `project` when repository configuration participates, otherwise `user`.
    pub config_mode: String,
    /// Loaded global configuration path, if any.
    pub global_config_path: Option<PathBuf>,
    /// Discovered repository configuration path, including when ignored.
    pub project_config_path: Option<PathBuf>,
    /// Human-readable guardrails applied by the safe profile.
    pub safety_limits: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            encoding: "o200k_base".to_string(),
            jobs: num_cpus::get(),
            use_cache: true,
            top: 10,
            max_complexity: 20,
            include_hidden: false,
            respect_gitignore: true,
            exclude_lockfiles: true,
            extra_excludes: Vec::new(),
            markers: ["TODO", "FIXME", "HACK", "XXX", "BUG"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            health_scope: HealthScope::Source,
            health_includes: Vec::new(),
            health_excludes: Vec::new(),
            min_dup_tokens: 50,
            min_dup_lines: 3,
            near_dup_min_similarity: 0.85,
            duplication_mode: DuplicationMode::Mild,
            duplication_format_scope: DuplicationFormatScope::Exact,
            duplication_report_snippets: false,
            churn_max_commits: 5000,
            max_churn_deltas_per_commit: 50_000,
            max_churn_total_deltas: 500_000,
            max_churn_output_bytes: 64 * 1024 * 1024,
            max_git_path_bytes: 4_096,
            max_churn_cache_bytes: 64 * 1024 * 1024,
            max_file_bytes: 32 * 1024 * 1024,
            max_total_bytes: 512 * 1024 * 1024,
            max_files: 100_000,
            max_git_blob_bytes: 32 * 1024 * 1024,
            max_scan_seconds: 1_800,
            load_repository_ignores: true,
            max_ignore_file_bytes: 1024 * 1024,
            max_ignore_lines: 50_000,
            max_ignore_line_bytes: 8_192,
            context: false,
            context_budget: 32_000,
            context_max_files: 25,
            context_focus: Vec::new(),
            quiet_progress: false,
            enabled: Enabled::default(),
            by_dir: None,
            diff_scope: None,
            baseline_path: None,
            fail_on_regression: false,
            graph: false,
            graph_focus: Vec::new(),
            graph_depth: 1,
            graph_direction: crate::cli::GraphDirection::Both,
            impact: false,
            change_summary: false,
            review: None,
            execution_profile: "full".to_string(),
            config_mode: "defaults".to_string(),
            global_config_path: None,
            project_config_path: None,
            safety_limits: Vec::new(),
        }
    }
}

/// Deserialized `reposcout.toml`. All fields optional; missing => default.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct FileConfig {
    encoding: Option<String>,
    jobs: Option<usize>,
    use_cache: Option<bool>,
    top: Option<usize>,
    max_complexity: Option<u32>,
    include_hidden: Option<bool>,
    respect_gitignore: Option<bool>,
    exclude_lockfiles: Option<bool>,
    excludes: Option<Vec<String>>,
    markers: Option<Vec<String>>,
    health_scope: Option<HealthScope>,
    health_includes: Option<Vec<HealthInclude>>,
    health_excludes: Option<Vec<String>>,
    min_dup_tokens: Option<usize>,
    min_dup_lines: Option<usize>,
    near_dup_min_similarity: Option<f64>,
    duplication_mode: Option<DuplicationMode>,
    duplication_format_scope: Option<DuplicationFormatScope>,
    duplication_report_snippets: Option<bool>,
    churn_max_commits: Option<usize>,
    max_churn_deltas_per_commit: Option<usize>,
    max_churn_total_deltas: Option<usize>,
    max_churn_output_bytes: Option<u64>,
    max_git_path_bytes: Option<usize>,
    max_churn_cache_bytes: Option<u64>,
    max_file_bytes: Option<u64>,
    max_total_bytes: Option<u64>,
    max_files: Option<usize>,
    max_git_blob_bytes: Option<u64>,
    max_scan_seconds: Option<u64>,
    load_repository_ignores: Option<bool>,
    max_ignore_file_bytes: Option<u64>,
    max_ignore_lines: Option<usize>,
    max_ignore_line_bytes: Option<usize>,
    context: Option<ContextFileConfig>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ContextFileConfig {
    enabled: Option<bool>,
    budget: Option<usize>,
    max_files: Option<usize>,
}

/// One configuration layer considered while resolving an effective config.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigSource {
    pub path: PathBuf,
    pub loaded: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub ignored: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<String>,
}

/// File sources ordered below CLI arguments and above built-in defaults.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigSources {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub global: Option<ConfigSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<ConfigSource>,
}

/// Serializable projection of settings accepted by `reposcout.toml`.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigValues {
    pub execution_profile: String,
    pub analyzers: Enabled,
    pub safety_limits: Vec<String>,
    pub encoding: String,
    pub jobs: usize,
    pub use_cache: bool,
    pub top: usize,
    pub max_complexity: u32,
    pub include_hidden: bool,
    pub respect_gitignore: bool,
    pub exclude_lockfiles: bool,
    pub excludes: Vec<String>,
    pub markers: Vec<String>,
    pub health_scope: HealthScope,
    pub health_includes: Vec<HealthInclude>,
    pub health_excludes: Vec<String>,
    pub min_dup_tokens: usize,
    pub min_dup_lines: usize,
    pub near_dup_min_similarity: f64,
    pub duplication_mode: DuplicationMode,
    pub duplication_format_scope: DuplicationFormatScope,
    pub duplication_report_snippets: bool,
    pub churn_max_commits: usize,
    pub max_churn_deltas_per_commit: usize,
    pub max_churn_total_deltas: usize,
    pub max_churn_output_bytes: u64,
    pub max_git_path_bytes: usize,
    pub max_churn_cache_bytes: u64,
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
    pub max_files: usize,
    pub max_git_blob_bytes: u64,
    pub max_scan_seconds: u64,
    pub load_repository_ignores: bool,
    pub max_ignore_file_bytes: u64,
    pub max_ignore_lines: usize,
    pub max_ignore_line_bytes: usize,
    pub context: bool,
    pub context_budget: usize,
    pub context_max_files: usize,
}

impl From<&Config> for ConfigValues {
    fn from(config: &Config) -> Self {
        Self {
            execution_profile: config.execution_profile.clone(),
            analyzers: config.enabled,
            safety_limits: config.safety_limits.clone(),
            encoding: config.encoding.clone(),
            jobs: config.jobs,
            use_cache: config.use_cache,
            top: config.top,
            max_complexity: config.max_complexity,
            include_hidden: config.include_hidden,
            respect_gitignore: config.respect_gitignore,
            exclude_lockfiles: config.exclude_lockfiles,
            excludes: config.extra_excludes.clone(),
            markers: config.markers.clone(),
            health_scope: config.health_scope,
            health_includes: config.health_includes.clone(),
            health_excludes: config.health_excludes.clone(),
            min_dup_tokens: config.min_dup_tokens,
            min_dup_lines: config.min_dup_lines,
            near_dup_min_similarity: config.near_dup_min_similarity,
            duplication_mode: config.duplication_mode,
            duplication_format_scope: config.duplication_format_scope,
            duplication_report_snippets: config.duplication_report_snippets,
            churn_max_commits: config.churn_max_commits,
            max_churn_deltas_per_commit: config.max_churn_deltas_per_commit,
            max_churn_total_deltas: config.max_churn_total_deltas,
            max_churn_output_bytes: config.max_churn_output_bytes,
            max_git_path_bytes: config.max_git_path_bytes,
            max_churn_cache_bytes: config.max_churn_cache_bytes,
            max_file_bytes: config.max_file_bytes,
            max_total_bytes: config.max_total_bytes,
            max_files: config.max_files,
            max_git_blob_bytes: config.max_git_blob_bytes,
            max_scan_seconds: config.max_scan_seconds,
            load_repository_ignores: config.load_repository_ignores,
            max_ignore_file_bytes: config.max_ignore_file_bytes,
            max_ignore_lines: config.max_ignore_lines,
            max_ignore_line_bytes: config.max_ignore_line_bytes,
            context: config.context,
            context_budget: config.context_budget,
            context_max_files: config.context_max_files,
        }
    }
}

/// Complete, inspectable result of resolving file-backed configuration.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigInspection {
    pub precedence: Vec<String>,
    pub config_mode: String,
    pub sources: ConfigSources,
    pub effective: ConfigValues,
}

#[derive(Debug, Clone)]
pub struct ConfigResolution {
    pub config: Config,
    pub sources: ConfigSources,
}

/// Compiled file policy for actionable health signals. Inventory, tokens,
/// lines, imports, symbols, and context discovery remain complete.
#[derive(Clone, Debug)]
pub struct HealthPolicy {
    scope: HealthScope,
    includes: Vec<HealthInclude>,
    excludes: Override,
}

impl HealthPolicy {
    fn new(config: &Config) -> Result<Self> {
        let mut builder = OverrideBuilder::new(".");
        for pattern in &config.health_excludes {
            builder
                .add(&format!("!{pattern}"))
                .with_context(|| format!("invalid health exclude glob: {pattern}"))?;
        }
        Ok(Self {
            scope: config.health_scope,
            includes: config.health_includes.clone(),
            excludes: builder
                .build()
                .context("building health exclude overrides")?,
        })
    }

    pub fn includes(&self, path: &Path, info: &LangInfo) -> bool {
        lang::included_in_health(info, self.scope, &self.includes)
            && !self.excludes.matched(path, false).is_ignore()
    }
}

impl ConfigResolution {
    pub fn inspection(&self) -> ConfigInspection {
        ConfigInspection {
            precedence: ["cli", "project", "global", "defaults"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            config_mode: self.config.config_mode.clone(),
            sources: self.sources.clone(),
            effective: ConfigValues::from(&self.config),
        }
    }
}

impl Config {
    /// Load built-in defaults, the OS-level global config, and the nearest
    /// project `reposcout.toml` or `.reposcout.toml` discovered from `start`.
    pub fn load(start: &Path) -> Result<Self> {
        Ok(Self::resolve(start)?.config)
    }

    /// Load defaults and global user policy while deliberately ignoring the
    /// nearest repository-owned configuration.
    pub fn load_without_project(start: &Path) -> Result<Self> {
        Ok(Self::resolve_without_project(start)?.config)
    }

    /// Resolve effective file-backed settings while retaining source metadata
    /// for `reposcout config` and other diagnostics.
    pub fn resolve(start: &Path) -> Result<ConfigResolution> {
        resolve_with_options(start, global_config_path(), true)
    }

    pub fn resolve_without_project(start: &Path) -> Result<ConfigResolution> {
        resolve_with_options(start, global_config_path(), false)
    }

    fn apply_file(&mut self, fc: FileConfig) {
        if let Some(v) = fc.encoding {
            self.encoding = v;
        }
        if let Some(v) = fc.jobs {
            self.jobs = v.max(1);
        }
        if let Some(v) = fc.use_cache {
            self.use_cache = v;
        }
        if let Some(v) = fc.top {
            self.top = v;
        }
        if let Some(v) = fc.max_complexity {
            self.max_complexity = v;
        }
        if let Some(v) = fc.include_hidden {
            self.include_hidden = v;
        }
        if let Some(v) = fc.respect_gitignore {
            self.respect_gitignore = v;
        }
        if let Some(v) = fc.exclude_lockfiles {
            self.exclude_lockfiles = v;
        }
        if let Some(v) = fc.excludes {
            self.extra_excludes = v;
        }
        if let Some(v) = fc.markers {
            self.markers = v;
        }
        if let Some(v) = fc.health_scope {
            self.health_scope = v;
        }
        if let Some(v) = fc.health_includes {
            self.health_includes = v;
        }
        if let Some(v) = fc.health_excludes {
            self.health_excludes = v;
        }
        if let Some(v) = fc.min_dup_tokens {
            self.min_dup_tokens = v;
        }
        if let Some(v) = fc.min_dup_lines {
            self.min_dup_lines = v;
        }
        if let Some(v) = fc.near_dup_min_similarity {
            self.near_dup_min_similarity = v;
        }
        if let Some(v) = fc.duplication_mode {
            self.duplication_mode = v;
        }
        if let Some(v) = fc.duplication_format_scope {
            self.duplication_format_scope = v;
        }
        if let Some(v) = fc.duplication_report_snippets {
            self.duplication_report_snippets = v;
        }
        if let Some(v) = fc.churn_max_commits {
            self.churn_max_commits = v;
        }
        if let Some(v) = fc.max_churn_deltas_per_commit {
            self.max_churn_deltas_per_commit = v;
        }
        if let Some(v) = fc.max_churn_total_deltas {
            self.max_churn_total_deltas = v;
        }
        if let Some(v) = fc.max_churn_output_bytes {
            self.max_churn_output_bytes = v;
        }
        if let Some(v) = fc.max_git_path_bytes {
            self.max_git_path_bytes = v;
        }
        if let Some(v) = fc.max_churn_cache_bytes {
            self.max_churn_cache_bytes = v;
        }
        if let Some(v) = fc.max_file_bytes {
            self.max_file_bytes = v;
        }
        if let Some(v) = fc.max_total_bytes {
            self.max_total_bytes = v;
        }
        if let Some(v) = fc.max_files {
            self.max_files = v;
        }
        if let Some(v) = fc.load_repository_ignores {
            self.load_repository_ignores = v;
        }
        if let Some(v) = fc.max_ignore_file_bytes {
            self.max_ignore_file_bytes = v;
        }
        if let Some(v) = fc.max_ignore_lines {
            self.max_ignore_lines = v;
        }
        if let Some(v) = fc.max_ignore_line_bytes {
            self.max_ignore_line_bytes = v;
        }
        if let Some(v) = fc.max_git_blob_bytes {
            self.max_git_blob_bytes = v;
        }
        if let Some(v) = fc.max_scan_seconds {
            self.max_scan_seconds = v;
        }
        if let Some(context) = fc.context {
            if let Some(v) = context.enabled {
                self.context = v;
            }
            if let Some(v) = context.budget {
                self.context_budget = v;
            }
            if let Some(v) = context.max_files {
                self.context_max_files = v;
            }
        }
    }

    /// Compile the effective health policy. Scope selects the default corpus,
    /// format includes add to it, and path excludes are applied last.
    pub fn health_policy(&self) -> Result<HealthPolicy> {
        HealthPolicy::new(self)
    }

    pub fn enforce_absolute_limits(&mut self) {
        self.jobs = self.jobs.clamp(1, ABSOLUTE_MAX_JOBS);
        self.top = self.top.min(ABSOLUTE_MAX_TOP);
        if self.churn_max_commits == 0 || self.churn_max_commits > ABSOLUTE_MAX_CHURN_COMMITS {
            self.churn_max_commits = ABSOLUTE_MAX_CHURN_COMMITS;
        }
        self.min_dup_tokens = self.min_dup_tokens.max(ABSOLUTE_MIN_DUP_TOKENS);
        self.min_dup_lines = self.min_dup_lines.max(1);
        self.near_dup_min_similarity = if self.near_dup_min_similarity.is_finite() {
            self.near_dup_min_similarity
                .clamp(ABSOLUTE_MIN_DUP_SIMILARITY, 1.0)
        } else {
            Config::default().near_dup_min_similarity
        };
        self.max_file_bytes = self.max_file_bytes.clamp(1, ABSOLUTE_MAX_FILE_BYTES);
        self.max_total_bytes = self.max_total_bytes.clamp(1, ABSOLUTE_MAX_TOTAL_BYTES);
        self.max_files = self.max_files.clamp(1, ABSOLUTE_MAX_FILES);
        self.max_git_blob_bytes = self
            .max_git_blob_bytes
            .clamp(1, ABSOLUTE_MAX_GIT_BLOB_BYTES);
        self.max_scan_seconds = self.max_scan_seconds.clamp(1, ABSOLUTE_MAX_SCAN_SECONDS);
        self.max_churn_deltas_per_commit = self
            .max_churn_deltas_per_commit
            .clamp(1, ABSOLUTE_MAX_CHURN_DELTAS_PER_COMMIT);
        self.max_churn_total_deltas = self
            .max_churn_total_deltas
            .clamp(1, ABSOLUTE_MAX_CHURN_TOTAL_DELTAS);
        self.max_churn_output_bytes = self
            .max_churn_output_bytes
            .clamp(1, ABSOLUTE_MAX_CHURN_OUTPUT_BYTES);
        self.max_git_path_bytes = self
            .max_git_path_bytes
            .clamp(1, ABSOLUTE_MAX_GIT_PATH_BYTES);
        self.max_churn_cache_bytes = self
            .max_churn_cache_bytes
            .clamp(1, ABSOLUTE_MAX_CHURN_CACHE_BYTES);
        self.max_ignore_file_bytes = self
            .max_ignore_file_bytes
            .clamp(1, ABSOLUTE_MAX_IGNORE_FILE_BYTES);
        self.max_ignore_lines = self.max_ignore_lines.clamp(1, ABSOLUTE_MAX_IGNORE_LINES);
        self.max_ignore_line_bytes = self
            .max_ignore_line_bytes
            .clamp(1, ABSOLUTE_MAX_IGNORE_LINE_BYTES);
        self.context_budget = self.context_budget.min(ABSOLUTE_MAX_CONTEXT_TOKENS);
        self.context_max_files = self.context_max_files.min(ABSOLUTE_MAX_CONTEXT_FILES);
    }
}

/// Return the OS-appropriate global configuration path. The environment
/// override is primarily useful for hermetic automation and test harnesses.
pub fn global_config_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(GLOBAL_CONFIG_ENV) {
        return Some(PathBuf::from(path));
    }
    ProjectDirs::from("", "", "reposcout").map(|dirs| dirs.config_dir().join("reposcout.toml"))
}

#[cfg(test)]
fn resolve_with_global(start: &Path, global_path: Option<PathBuf>) -> Result<ConfigResolution> {
    resolve_with_options(start, global_path, true)
}

fn resolve_with_options(
    start: &Path,
    global_path: Option<PathBuf>,
    include_project: bool,
) -> Result<ConfigResolution> {
    let mut config = Config::default();
    let global = match global_path {
        Some(path) => {
            let file = read_config_if_present(&path)?;
            let source = ConfigSource {
                path: path.clone(),
                loaded: file.is_some(),
                ignored: false,
                keys: file
                    .as_ref()
                    .map(FileConfig::defined_keys)
                    .unwrap_or_default(),
            };
            if let Some(file) = file {
                config.apply_file(file);
                config.global_config_path = Some(path);
            }
            Some(source)
        }
        None => None,
    };

    let project = match find_project_config(start) {
        Some(path) => {
            config.project_config_path = Some(path.clone());
            let source = if include_project {
                let file = read_config(&path)?;
                let source = ConfigSource {
                    path,
                    loaded: true,
                    ignored: false,
                    keys: file.defined_keys(),
                };
                config.apply_file(file);
                source
            } else {
                ConfigSource {
                    path,
                    loaded: false,
                    ignored: true,
                    keys: Vec::new(),
                }
            };
            Some(source)
        }
        None => None,
    };

    config.config_mode = if project.as_ref().is_some_and(|source| source.loaded) {
        "project"
    } else if !include_project || global.as_ref().is_some_and(|source| source.loaded) {
        "user"
    } else {
        "defaults"
    }
    .to_string();

    config.enforce_absolute_limits();
    Ok(ConfigResolution {
        config,
        sources: ConfigSources { global, project },
    })
}

fn find_project_config(start: &Path) -> Option<PathBuf> {
    let mut dir = if start.is_file() {
        let parent = start.parent()?;
        parent.to_path_buf()
    } else {
        start.to_path_buf()
    };
    loop {
        for name in ["reposcout.toml", ".reposcout.toml"] {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate.canonicalize().unwrap_or(candidate));
            }
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn read_config_if_present(path: &Path) -> Result<Option<FileConfig>> {
    path.is_file().then(|| read_config(path)).transpose()
}

fn read_config(path: &Path) -> Result<FileConfig> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("failed to read config {}", path.display()))?;
    if file
        .metadata()
        .with_context(|| format!("failed to inspect config {}", path.display()))?
        .len()
        > MAX_CONFIG_BYTES
    {
        return Err(anyhow::anyhow!(
            "config {} exceeds the 1 MiB size limit",
            path.display()
        ));
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read config {}", path.display()))?;
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return Err(anyhow::anyhow!(
            "config {} exceeds the 1 MiB size limit",
            path.display()
        ));
    }
    let text = String::from_utf8(bytes)
        .with_context(|| format!("config {} is not UTF-8", path.display()))?;
    toml::from_str::<FileConfig>(&text)
        .map_err(|error| anyhow::anyhow!("failed to parse config {}: {error}", path.display()))
}

impl FileConfig {
    fn defined_keys(&self) -> Vec<String> {
        let mut keys = Vec::new();
        macro_rules! key {
            ($field:ident) => {
                if self.$field.is_some() {
                    keys.push(stringify!($field).to_string());
                }
            };
        }
        key!(encoding);
        key!(jobs);
        key!(use_cache);
        key!(top);
        key!(max_complexity);
        key!(include_hidden);
        key!(respect_gitignore);
        key!(exclude_lockfiles);
        key!(excludes);
        key!(markers);
        key!(health_scope);
        key!(health_includes);
        key!(health_excludes);
        key!(min_dup_tokens);
        key!(min_dup_lines);
        key!(near_dup_min_similarity);
        key!(duplication_mode);
        key!(duplication_format_scope);
        key!(duplication_report_snippets);
        key!(churn_max_commits);
        key!(max_churn_deltas_per_commit);
        key!(max_churn_total_deltas);
        key!(max_churn_output_bytes);
        key!(max_git_path_bytes);
        key!(max_churn_cache_bytes);
        key!(max_file_bytes);
        key!(load_repository_ignores);
        key!(max_ignore_file_bytes);
        key!(max_ignore_lines);
        key!(max_ignore_line_bytes);
        key!(max_total_bytes);
        key!(max_files);
        key!(max_git_blob_bytes);
        key!(max_scan_seconds);
        if let Some(context) = &self.context {
            if context.enabled.is_some() {
                keys.push("context.enabled".to_string());
            }
            if context.budget.is_some() {
                keys.push("context.budget".to_string());
            }
            if context.max_files.is_some() {
                keys.push("context.max_files".to_string());
            }
        }
        keys
    }
}

#[cfg(test)]
mod tests {
    use super::{Config, resolve_with_global, resolve_with_options};
    use crate::dup::{DuplicationFormatScope, DuplicationMode};
    use crate::lang::{HealthInclude, HealthScope};
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    fn load_project(path: &Path) -> Config {
        resolve_with_global(path, None).unwrap().config
    }

    #[test]
    fn invalid_config_is_reported_instead_of_silently_ignored() {
        let dir = tempdir().unwrap();
        let config = dir.path().join("reposcout.toml");
        fs::write(&config, "unknown_setting = true\n").unwrap();

        let error = resolve_with_global(dir.path(), None)
            .unwrap_err()
            .to_string();

        assert!(error.contains("failed to parse config"));
        assert!(error.contains("reposcout.toml"));
    }

    #[test]
    fn project_configuration_cannot_disable_absolute_limits() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("reposcout.toml"),
            concat!(
                "jobs = 999999\n",
                "top = 999999\n",
                "churn_max_commits = 0\n",
                "min_dup_tokens = 0\n",
                "near_dup_min_similarity = 0.01\n",
                "max_file_bytes = 999999999999\n",
                "max_total_bytes = 999999999999\n",
                "max_files = 999999999\n",
                "max_git_blob_bytes = 999999999999\n",
                "max_scan_seconds = 999999999\n",
                "[context]\n",
                "budget = 999999999\n",
                "max_files = 999999999\n",
            ),
        )
        .unwrap();

        let config = load_project(dir.path());

        assert_eq!(config.jobs, 64);
        assert_eq!(config.top, 1_000);
        assert_eq!(config.churn_max_commits, 100_000);
        assert_eq!(config.min_dup_tokens, 8);
        assert_eq!(config.near_dup_min_similarity, 0.5);
        assert_eq!(config.max_file_bytes, 256 * 1024 * 1024);
        assert_eq!(config.max_total_bytes, 4 * 1024 * 1024 * 1024);
        assert_eq!(config.max_files, 500_000);
        assert_eq!(config.max_git_blob_bytes, 256 * 1024 * 1024);
        assert_eq!(config.max_scan_seconds, 7_200);
        assert_eq!(config.context_budget, 5_000_000);
        assert_eq!(config.context_max_files, 10_000);
    }

    #[test]
    fn configuration_files_have_a_size_limit() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("reposcout.toml"),
            vec![b' '; 1024 * 1024 + 1],
        )
        .unwrap();

        let error = resolve_with_global(dir.path(), None)
            .unwrap_err()
            .to_string();

        assert!(error.contains("1 MiB size limit"), "error was: {error}");
    }

    #[test]
    fn duplication_options_load_from_config() {
        let dir = tempdir().unwrap();
        let config = dir.path().join("reposcout.toml");
        fs::write(
            &config,
            "duplication_mode = \"weak\"\nduplication_format_scope = \"compatible\"\nduplication_report_snippets = true\n",
        )
        .unwrap();

        let loaded = load_project(dir.path());

        assert_eq!(loaded.duplication_mode, DuplicationMode::Weak);
        assert_eq!(
            loaded.duplication_format_scope,
            DuplicationFormatScope::Compatible
        );
        assert!(loaded.duplication_report_snippets);
    }

    #[test]
    fn health_file_policy_loads_from_config() {
        let dir = tempdir().unwrap();
        let config = dir.path().join("reposcout.toml");
        fs::write(
            &config,
            concat!(
                "health_scope = \"all\"\n",
                "health_includes = [\"json\", \"css\"]\n",
                "health_excludes = [\"vendor/**\", \"generated/**\"]\n",
            ),
        )
        .unwrap();

        let loaded = load_project(dir.path());

        assert_eq!(loaded.health_scope, HealthScope::All);
        assert_eq!(
            loaded.health_includes,
            vec![HealthInclude::Json, HealthInclude::Css]
        );
        assert_eq!(loaded.health_excludes, vec!["vendor/**", "generated/**"]);
    }

    #[test]
    fn function_complexity_maximum_loads_from_config() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("reposcout.toml"), "max_complexity = 12\n").unwrap();

        let loaded = load_project(dir.path());

        assert_eq!(loaded.max_complexity, 12);
    }

    #[test]
    fn project_explicit_fields_override_global_and_omissions_inherit() {
        let dir = tempdir().unwrap();
        let global = dir.path().join("global.toml");
        let project = dir.path().join("project");
        fs::create_dir(&project).unwrap();
        fs::write(
            &global,
            "jobs = 3\ntop = 20\nmarkers = [\"GLOBAL\"]\nexcludes = [\"global/**\"]\n\n[context]\nenabled = true\nbudget = 9000\n",
        )
        .unwrap();
        fs::write(
            project.join("reposcout.toml"),
            "top = 7\nmarkers = [\"PROJECT\"]\n\n[context]\nmax_files = 8\n",
        )
        .unwrap();

        let resolved = resolve_with_global(&project, Some(global)).unwrap();

        assert_eq!(resolved.config.jobs, 3, "omitted project field inherits");
        assert_eq!(resolved.config.top, 7, "project field wins");
        assert_eq!(resolved.config.markers, ["PROJECT"]);
        assert_eq!(resolved.config.extra_excludes, ["global/**"]);
        assert!(resolved.config.context);
        assert_eq!(resolved.config.context_budget, 9000);
        assert_eq!(resolved.config.context_max_files, 8);
    }

    #[test]
    fn explicitly_defined_project_lists_replace_global_lists() {
        let dir = tempdir().unwrap();
        let global = dir.path().join("global.toml");
        fs::write(&global, "excludes = [\"global/**\"]\n").unwrap();
        fs::write(
            dir.path().join("reposcout.toml"),
            "excludes = [\"project/**\"]\n",
        )
        .unwrap();

        let loaded = resolve_with_global(dir.path(), Some(global))
            .unwrap()
            .config;

        assert_eq!(loaded.extra_excludes, ["project/**"]);
    }

    #[test]
    fn nearest_project_config_is_discovered_from_a_nested_path() {
        let dir = tempdir().unwrap();
        let nested = dir.path().join("workspace/crate/src");
        fs::create_dir_all(&nested).unwrap();
        fs::write(dir.path().join("reposcout.toml"), "top = 30\n").unwrap();
        fs::write(dir.path().join("workspace/reposcout.toml"), "top = 12\n").unwrap();

        let resolved = resolve_with_global(&nested, None).unwrap();

        assert_eq!(resolved.config.top, 12);
        assert_eq!(
            resolved.sources.project.unwrap().path,
            dir.path()
                .join("workspace/reposcout.toml")
                .canonicalize()
                .unwrap()
        );
    }

    #[test]
    fn ignored_project_config_is_discovered_but_never_parsed_or_applied() {
        let dir = tempdir().unwrap();
        let global = dir.path().join("global.toml");
        fs::write(&global, "jobs = 2\n").unwrap();
        let project = dir.path().join("reposcout.toml");
        fs::write(&project, "this is deliberately invalid TOML = [\n").unwrap();

        let resolved = resolve_with_options(dir.path(), Some(global), false).unwrap();

        assert_eq!(resolved.config.jobs, 2);
        assert_eq!(resolved.config.config_mode, "user");
        let canonical_project = project.canonicalize().unwrap();
        assert_eq!(
            resolved.config.project_config_path.as_deref(),
            Some(canonical_project.as_path())
        );
        let source = resolved.sources.project.unwrap();
        assert!(source.ignored);
        assert!(!source.loaded);
        assert!(source.keys.is_empty());
    }

    #[test]
    fn config_mode_distinguishes_defaults_user_and_project_sources() {
        let defaults = tempdir().unwrap();
        assert_eq!(
            resolve_with_options(defaults.path(), None, true)
                .unwrap()
                .config
                .config_mode,
            "defaults"
        );

        let user = tempdir().unwrap();
        let global = user.path().join("global.toml");
        fs::write(&global, "jobs = 2\n").unwrap();
        assert_eq!(
            resolve_with_options(user.path(), Some(global), true)
                .unwrap()
                .config
                .config_mode,
            "user"
        );

        let project = tempdir().unwrap();
        fs::write(project.path().join("reposcout.toml"), "jobs = 2\n").unwrap();
        assert_eq!(
            resolve_with_options(project.path(), None, true)
                .unwrap()
                .config
                .config_mode,
            "project"
        );
    }

    #[test]
    fn invalid_global_config_identifies_the_file_and_setting() {
        let dir = tempdir().unwrap();
        let global = dir.path().join("global.toml");
        fs::write(&global, "unknown_global_setting = true\n").unwrap();

        let error = resolve_with_global(dir.path(), Some(global.clone()))
            .unwrap_err()
            .to_string();

        assert!(error.contains(global.to_string_lossy().as_ref()));
        assert!(error.contains("unknown_global_setting"));
    }

    #[test]
    fn missing_global_config_is_reported_but_not_an_error() {
        let dir = tempdir().unwrap();
        let global = dir.path().join("missing-global.toml");

        let resolved = resolve_with_global(dir.path(), Some(global.clone())).unwrap();
        let source = resolved.sources.global.unwrap();

        assert_eq!(source.path, global);
        assert!(!source.loaded);
        assert!(source.keys.is_empty());
        assert_eq!(resolved.config.context_budget, 32_000);
    }

    #[test]
    fn invalid_nested_setting_identifies_its_name() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("reposcout.toml"),
            "[context]\nbudegt = 1000\n",
        )
        .unwrap();

        let error = resolve_with_global(dir.path(), None)
            .unwrap_err()
            .to_string();

        assert!(error.contains("budegt"), "error was: {error}");
        assert!(error.contains("reposcout.toml"), "error was: {error}");
    }

    #[test]
    fn inspection_reports_loaded_layers_and_defined_keys() {
        let dir = tempdir().unwrap();
        let global = dir.path().join("global.toml");
        fs::write(&global, "jobs = 2\n").unwrap();
        fs::write(dir.path().join("reposcout.toml"), "top = 4\n").unwrap();

        let inspection = resolve_with_global(dir.path(), Some(global))
            .unwrap()
            .inspection();

        assert_eq!(
            inspection.precedence,
            ["cli", "project", "global", "defaults"]
        );
        assert_eq!(inspection.sources.global.unwrap().keys, ["jobs"]);
        assert_eq!(inspection.sources.project.unwrap().keys, ["top"]);
        assert_eq!(inspection.effective.jobs, 2);
        assert_eq!(inspection.effective.top, 4);
    }
}
