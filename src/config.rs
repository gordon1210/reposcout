//! Runtime configuration: built-in defaults, an optional OS-level global file,
//! the nearest project file, and the set of enabled analyzers. Later layers
//! override only the fields they explicitly define; CLI flags are applied by
//! `main` after file resolution.

use crate::dup::{DuplicationFormatScope, DuplicationMode};
use crate::lang::{self, HealthInclude, HealthScope, LangInfo};
use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const GLOBAL_CONFIG_ENV: &str = "REPOSCOUT_GLOBAL_CONFIG";

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
    /// Formats eligible for source-health analyzers such as markers and
    /// duplication. Inventory metrics always retain every recognized format.
    pub health_scope: HealthScope,
    /// Non-source formats added to the default source-health corpus.
    pub health_includes: Vec<HealthInclude>,
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
    /// Cap on commits walked for churn (0 = unlimited).
    pub churn_max_commits: usize,
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
            min_dup_tokens: 50,
            min_dup_lines: 3,
            near_dup_min_similarity: 0.85,
            duplication_mode: DuplicationMode::Mild,
            duplication_format_scope: DuplicationFormatScope::Exact,
            duplication_report_snippets: false,
            churn_max_commits: 5000,
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
    min_dup_tokens: Option<usize>,
    min_dup_lines: Option<usize>,
    near_dup_min_similarity: Option<f64>,
    duplication_mode: Option<DuplicationMode>,
    duplication_format_scope: Option<DuplicationFormatScope>,
    duplication_report_snippets: Option<bool>,
    churn_max_commits: Option<usize>,
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
    pub min_dup_tokens: usize,
    pub min_dup_lines: usize,
    pub near_dup_min_similarity: f64,
    pub duplication_mode: DuplicationMode,
    pub duplication_format_scope: DuplicationFormatScope,
    pub duplication_report_snippets: bool,
    pub churn_max_commits: usize,
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
            min_dup_tokens: config.min_dup_tokens,
            min_dup_lines: config.min_dup_lines,
            near_dup_min_similarity: config.near_dup_min_similarity,
            duplication_mode: config.duplication_mode,
            duplication_format_scope: config.duplication_format_scope,
            duplication_report_snippets: config.duplication_report_snippets,
            churn_max_commits: config.churn_max_commits,
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

    /// Whether a recognized format participates in the source-health corpus.
    /// This is the sole configuration seam used by per-file markers and
    /// scan-wide duplication eligibility.
    pub fn includes_in_health(&self, info: &LangInfo) -> bool {
        lang::included_in_health(info, self.health_scope, &self.health_includes)
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
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read config {}", path.display()))?;
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
        key!(min_dup_tokens);
        key!(min_dup_lines);
        key!(near_dup_min_similarity);
        key!(duplication_mode);
        key!(duplication_format_scope);
        key!(duplication_report_snippets);
        key!(churn_max_commits);
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
            "health_scope = \"all\"\nhealth_includes = [\"json\", \"css\"]\n",
        )
        .unwrap();

        let loaded = load_project(dir.path());

        assert_eq!(loaded.health_scope, HealthScope::All);
        assert_eq!(
            loaded.health_includes,
            vec![HealthInclude::Json, HealthInclude::Css]
        );
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
