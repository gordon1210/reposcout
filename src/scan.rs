//! Scan orchestrator: discover files, analyze each in parallel, attach git
//! churn, run duplication detection, and aggregate a [`ScanReport`].

use crate::cache::{self, Cache};
use crate::config::{Config, HealthPolicy};
use crate::debug_log;
use crate::dup::{self, DupInput, DuplicateCoverage};
use crate::git;
use crate::lang;
use crate::metrics::tokens::TokenCounter;
use crate::metrics::{classify, complexity, imports, lines, markers, risk, symbols, testcov};
use crate::model::{
    AnalyzerProfile, Assessment, BaselineDelta, CloneGroup, CloneInstance, DirSummary,
    DuplicateBlock, DuplicateFindingSummary, Duplication, DuplicationProfile, DuplicationSummary,
    ExecutionMetadata, FileDuplication, FileRef, FileReport, FindingCatalog, FindingProfile,
    FunctionHotspot, HealthProfile, Hotspot, LanguageDuplication, LanguageStat, LineRange,
    MetricDelta, ProductionDuplication, ResourceProfile, ReviewChangedFile, RiskEntry,
    SCHEMA_VERSION, ScanDiagnostics, ScanProfile, ScanReport, SkipCandidate, Summary,
    SymbolOutline, TestPresence,
};
use crate::numeric::{u64_to_f64, usize_to_f64};
use crate::parse;
use crate::walk;
use anyhow::Result;
use ignore::overrides::OverrideBuilder;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use std::collections::{BTreeMap, HashSet};
use std::io::IsTerminal;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct AnalyzedFile {
    report: FileReport,
    content: String,
    duplication_artifact: bool,
    test_regions: Vec<LineRange>,
    symbol_outlines: Option<Vec<SymbolOutline>>,
    graph_facts: Option<crate::graph::SourceFacts>,
}

struct SourceAnalysis {
    report: FileReport,
    duplication_artifact: bool,
    test_regions: Vec<LineRange>,
    symbol_outlines: Option<Vec<SymbolOutline>>,
    graph_facts: Option<crate::graph::SourceFacts>,
}

#[derive(serde::Deserialize)]
struct BaselineInput {
    schema_version: String,
    generated_at: String,
    encoding: String,
    root: PathBuf,
    target: PathBuf,
    summary: Summary,
    #[serde(default)]
    finding_catalog: FindingCatalog,
    #[serde(default)]
    analysis_profile: Option<ScanProfile>,
}

enum AnalysisOutcome {
    Analyzed(Box<AnalyzedFile>),
    Unsupported(PathBuf),
    Unreadable,
    Oversized(u64),
    DurationLimit,
}

impl AnalysisOutcome {
    fn status(&self) -> &'static str {
        match self {
            Self::Analyzed(_) => "analyzed",
            Self::Unsupported(_) => "unsupported",
            Self::Unreadable => "unreadable",
            Self::Oversized(_) => "oversized",
            Self::DurationLimit => "duration_limit",
        }
    }
}

struct ScanProgress {
    bar: ProgressBar,
    visible: bool,
}

struct PreparedScan {
    discovered: walk::Discovered,
    planning_discovered: Option<walk::Discovered>,
    test_frameworks: Vec<crate::model::TestFramework>,
    planning_test_frameworks: Vec<crate::model::TestFramework>,
    root: PathBuf,
    diff_base: Option<String>,
    analysis_profile: ScanProfile,
    effective_exclusions: Vec<PathBuf>,
    review_changed_files: Option<Vec<ReviewChangedFile>>,
    scoped_changed_files: HashSet<PathBuf>,
    impact_changed_files: HashSet<PathBuf>,
    context_changes: Option<crate::context::ChangeSeeds>,
    all_report_paths: Vec<PathBuf>,
    deadline: Option<Instant>,
    health_policy: HealthPolicy,
}

struct AnalyzedScan {
    files: Vec<FileReport>,
    test_regions: BTreeMap<PathBuf, Vec<LineRange>>,
    symbol_outlines: BTreeMap<PathBuf, Vec<SymbolOutline>>,
    graph_facts: BTreeMap<PathBuf, crate::graph::SourceFacts>,
    duplication: Duplication,
    duplicate_coverage: DuplicateCoverage,
    diagnostics: ScanDiagnostics,
    encoding_name: String,
    cache_stats: cache::CacheStats,
}

struct FileAnalysis {
    analyzed: Vec<AnalyzedFile>,
    diagnostics: ScanDiagnostics,
    encoding_name: String,
    cache: Cache,
}

struct PlanningAnalysis {
    files: Vec<FileReport>,
    symbol_outlines: BTreeMap<PathBuf, Vec<SymbolOutline>>,
    graph_facts: BTreeMap<PathBuf, crate::graph::SourceFacts>,
    risks: Vec<RiskEntry>,
    diagnostics: ScanDiagnostics,
    cache_stats: cache::CacheStats,
}

pub struct ScanArtifacts {
    pub report: ScanReport,
    pub symbol_outlines: BTreeMap<PathBuf, Vec<SymbolOutline>>,
    pub graph_facts: BTreeMap<PathBuf, crate::graph::SourceFacts>,
    pub resolver_configs: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ArtifactRequirements {
    pub symbol_outlines: bool,
    pub graph_facts: bool,
}

impl ScanProgress {
    fn new(files: usize, visible: bool) -> Self {
        let bar = if visible {
            let bar = ProgressBar::new(files as u64);
            bar.set_style(progress_style(
                "{spinner} scanning files [{bar:30}] {pos}/{len} [{elapsed_precise}]",
            ));
            bar.enable_steady_tick(Duration::from_millis(120));
            bar
        } else {
            ProgressBar::hidden()
        };
        Self { bar, visible }
    }

    fn file_completed(&self) {
        self.bar.inc(1);
    }

    fn files_stage(&self, message: &str, files: usize) {
        debug_log::event(
            "scan_stage",
            || serde_json::json!({ "stage": message, "files": files }),
        );
        if !self.visible {
            return;
        }
        self.bar.set_length(files as u64);
        self.bar.set_position(0);
        self.bar.set_style(progress_style(
            "{spinner} {msg} [{bar:30}] {pos}/{len} [{elapsed_precise}]",
        ));
        self.bar.set_message(message.to_string());
        self.bar.enable_steady_tick(Duration::from_millis(120));
    }

    fn stage(&self, message: impl Into<String>) {
        let message = message.into();
        debug_log::event(
            "scan_stage",
            || serde_json::json!({ "stage": message.as_str() }),
        );
        if !self.visible {
            return;
        }
        self.bar
            .set_style(progress_style("{spinner} {msg} [{elapsed_precise}]"));
        self.bar.set_message(message);
        self.bar.enable_steady_tick(Duration::from_millis(120));
        self.bar.tick();
    }
}

fn progress_style(template: &str) -> ProgressStyle {
    ProgressStyle::with_template(template)
        .unwrap_or_else(|_| ProgressStyle::default_bar())
        .progress_chars("=> ")
}

impl Drop for ScanProgress {
    fn drop(&mut self) {
        self.bar.finish_and_clear();
    }
}

const DEFAULT_CONTEXT_BUDGET: usize = 200_000;

/// Scan one repository path using the effective configuration.
///
/// # Errors
///
/// Returns an error when discovery, configured analysis, Git inspection,
/// baseline loading, or report assembly fails.
pub fn run(target: &Path, cfg: &Config) -> Result<ScanReport> {
    run_with_exclusions(target, cfg, &[])
}

/// Run a scan while omitting exact filesystem paths such as a CLI output file.
///
/// # Errors
///
/// Returns an error when discovery, configured analysis, Git inspection,
/// baseline loading, or report assembly fails.
pub fn run_with_exclusions(
    target: &Path,
    cfg: &Config,
    exclusions: &[PathBuf],
) -> Result<ScanReport> {
    run_with_artifacts(
        target,
        cfg,
        exclusions,
        ArtifactRequirements {
            symbol_outlines: cfg.context,
            graph_facts: cfg.graph || cfg.context || cfg.impact,
        },
    )
    .map(|artifacts| artifacts.report)
}

/// Scan a path and retain explicitly requested internal analysis artifacts.
///
/// # Errors
///
/// Returns an error when discovery, configured analysis, Git inspection,
/// baseline loading, or report assembly fails.
pub fn run_with_artifacts(
    target: &Path,
    cfg: &Config,
    exclusions: &[PathBuf],
    requirements: ArtifactRequirements,
) -> Result<ScanArtifacts> {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(cfg.jobs.max(1))
        .build()?;
    pool.install(|| run_with_artifacts_in_pool(target, cfg, exclusions, requirements))
}

fn run_with_artifacts_in_pool(
    target: &Path,
    cfg: &Config,
    exclusions: &[PathBuf],
    requirements: ArtifactRequirements,
) -> Result<ScanArtifacts> {
    let total_started = start_stage("total");
    let deadline = Some(
        total_started
            .checked_add(Duration::from_secs(cfg.max_scan_seconds))
            .unwrap_or(total_started),
    );
    let mut stage_ms = BTreeMap::new();
    let stage_started = start_stage("discovery");
    let prepared = prepare_scan(target, cfg, exclusions, deadline)?;
    record_stage(&mut stage_ms, "discovery", stage_started.elapsed());
    debug_log::event("discovery_summary", || {
        serde_json::json!({
            "root": prepared.root.to_string_lossy(),
            "target": prepared.discovered.target.to_string_lossy(),
            "files": prepared.discovered.files.len(),
            "walker_errors": prepared.discovered.walker_errors,
            "planning_files": prepared
                .planning_discovered
                .as_ref()
                .map(|discovered| discovered.files.len()),
            "exclusions": prepared.effective_exclusions.len(),
        })
    });
    let progress_files = prepared
        .planning_discovered
        .as_ref()
        .map_or(prepared.discovered.files.len(), |discovered| {
            discovered.files.len()
        })
        .max(prepared.discovered.files.len());
    let show_progress =
        !cfg.quiet_progress && std::io::stderr().is_terminal() && progress_files > 50;
    let progress = ScanProgress::new(prepared.discovered.files.len(), show_progress);
    let stage_started = start_stage("file_analysis");
    let file_analysis = analyze_discovered_files(&prepared, cfg, &progress, requirements)?;
    record_stage(&mut stage_ms, "file_analysis", stage_started.elapsed());
    let stage_started = start_stage("cross_file");
    let analyzed = analyze_cross_file_metrics(&prepared, file_analysis, cfg, &progress);
    record_stage(&mut stage_ms, "cross_file", stage_started.elapsed());
    let context_started = cfg.context.then(Instant::now);
    let stage_started = start_stage("planning_universe");
    let planning = analyze_planning_universe(&prepared, cfg, &progress, requirements)?;
    record_stage(&mut stage_ms, "planning_universe", stage_started.elapsed());
    let context_planning_elapsed = context_started.map(|started| started.elapsed());
    let stage_started = start_stage("report_assembly");
    let mut artifacts = assemble_report(
        prepared,
        analyzed,
        planning.as_ref(),
        context_planning_elapsed,
        cfg,
        &progress,
    )?;
    record_stage(&mut stage_ms, "report_assembly", stage_started.elapsed());
    record_stage(&mut stage_ms, "total", total_started.elapsed());
    artifacts.report.execution.stage_ms = stage_ms;
    if deadline_reached(deadline) {
        mark_duration_limit(&mut artifacts.report.diagnostics, 0);
    }
    Ok(artifacts)
}

fn start_stage(name: &'static str) -> Instant {
    debug_log::event("stage_start", || serde_json::json!({ "stage": name }));
    Instant::now()
}

fn record_stage(stages: &mut BTreeMap<String, usize>, name: &str, elapsed: Duration) {
    let duration_ms = usize::try_from(elapsed.as_millis()).unwrap_or(usize::MAX);
    stages.insert(name.to_string(), duration_ms);
    debug_log::event(
        "stage_end",
        || serde_json::json!({ "stage": name, "duration_ms": duration_ms }),
    );
}

fn deadline_reached(deadline: Option<Instant>) -> bool {
    deadline.is_some_and(|deadline| Instant::now() >= deadline)
}

fn mark_duration_limit(diagnostics: &mut ScanDiagnostics, omitted_files: usize) {
    diagnostics.duration_limit_reached = true;
    diagnostics.scan_truncated = true;
    diagnostics.files_omitted_by_limit = diagnostics
        .files_omitted_by_limit
        .saturating_add(omitted_files);
}

fn prepare_scan(
    target: &Path,
    cfg: &Config,
    exclusions: &[PathBuf],
    deadline: Option<Instant>,
) -> Result<PreparedScan> {
    let health_policy = cfg.health_policy()?;
    let effective_exclusions = scan_exclusions(cfg, exclusions);
    let target_missing = !target.exists();
    let mut discovered =
        if target_missing && (cfg.impact || cfg.context) && cfg.diff_scope.is_some() {
            walk::discover_missing_file(target)?
        } else {
            walk::discover_with_exclusions_until(target, cfg, &effective_exclusions, deadline)?
        };
    let test_frameworks =
        detect_test_frameworks(&discovered, cfg, &effective_exclusions, deadline)?;
    let root = discovered.root.clone();
    let diff_base = match cfg.diff_scope.as_ref() {
        Some(scope) => git::diff_base_tree_id(&root, scope)?,
        None => None,
    };
    let analysis_profile = scan_profile(cfg, diff_base.clone());
    let excluded_report_paths = excluded_report_paths(&root, &effective_exclusions)?;
    let changed_files =
        changed_files_for_scope(cfg, &root, diff_base.as_deref(), &excluded_report_paths)?;
    validate_review_configuration(cfg)?;
    let review_changed_files = review_changed_files_for_scope(
        cfg,
        &root,
        &discovered.target,
        diff_base.as_deref(),
        &excluded_report_paths,
    )?;
    let scoped_changed_files = changed_files
        .as_ref()
        .map(|changed| {
            changed
                .iter()
                .filter(|path| path_is_within_target(path, &root, &discovered.target))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let impact_changed_files =
        impact_changed_files(cfg, changed_files.as_ref(), &root, &discovered.target)?;
    let context_changes =
        context_change_seeds(cfg, changed_files.as_ref(), &root, &discovered.target)?;
    if target_missing
        && ((cfg.impact && impact_changed_files.is_empty())
            || (cfg.context
                && context_changes
                    .as_ref()
                    .is_none_or(|changes| changes.paths.is_empty())))
    {
        return Err(anyhow::anyhow!("path not found: {}", target.display()));
    }
    let needs_planning_universe = cfg.context && cfg.diff_scope.is_some();
    let full_discovered = if cfg.impact || needs_planning_universe {
        Some(walk::discover_with_exclusions_until(
            &root,
            cfg,
            &effective_exclusions,
            deadline,
        )?)
    } else {
        None
    };
    let all_report_paths = full_discovered
        .as_ref()
        .map(|discovered| {
            discovered
                .files
                .iter()
                .map(|file| file.report_path.clone())
                .collect()
        })
        .unwrap_or_default();
    let planning_discovered = needs_planning_universe.then_some(full_discovered).flatten();
    let planning_test_frameworks = match planning_discovered.as_ref() {
        Some(files) => detect_test_frameworks(files, cfg, &effective_exclusions, deadline)?,
        None => Vec::new(),
    };
    if let Some(changed) = &changed_files {
        discovered
            .files
            .retain(|file| changed.contains(&file.report_path));
    }

    Ok(PreparedScan {
        discovered,
        planning_discovered,
        test_frameworks,
        planning_test_frameworks,
        root,
        diff_base,
        analysis_profile,
        effective_exclusions,
        review_changed_files,
        scoped_changed_files,
        impact_changed_files,
        context_changes,
        all_report_paths,
        deadline,
        health_policy,
    })
}

fn detect_test_frameworks(
    discovered: &walk::Discovered,
    cfg: &Config,
    exclusions: &[PathBuf],
    deadline: Option<Instant>,
) -> Result<Vec<crate::model::TestFramework>> {
    let mut frameworks = testcov::detect_frameworks(
        discovered
            .files
            .iter()
            .map(|file| (file.absolute_path.as_path(), file.report_path.as_path())),
        cfg.max_file_bytes,
    );
    let ancestor_evidence = ancestor_runner_evidence(discovered, cfg, exclusions, deadline)?;
    frameworks.extend(testcov::detect_frameworks(
        ancestor_evidence
            .iter()
            .map(|file| (file.absolute_path.as_path(), file.report_path.as_path())),
        cfg.max_file_bytes,
    ));
    frameworks
        .sort_by(|left, right| (&left.name, &left.evidence).cmp(&(&right.name, &right.evidence)));
    frameworks.dedup_by(|left, right| left.name == right.name && left.evidence == right.evidence);
    Ok(frameworks)
}

fn ancestor_runner_evidence(
    discovered: &walk::Discovered,
    cfg: &Config,
    exclusions: &[PathBuf],
    deadline: Option<Instant>,
) -> Result<Vec<walk::DiscoveredFile>> {
    let discovered_paths = discovered
        .files
        .iter()
        .map(|file| file.absolute_path.as_path())
        .collect::<HashSet<_>>();
    let exact_exclusions = exclusions
        .iter()
        .map(|path| walk::exact_path_identity(path))
        .collect::<Result<HashSet<_>>>()?;
    let mut override_builder = OverrideBuilder::new(&discovered.root);
    for pattern in &cfg.extra_excludes {
        override_builder.add(&format!("!{pattern}"))?;
    }
    let overrides = override_builder.build()?;
    let mut evidence = Vec::new();
    let mut ancestor = discovered.target.parent();

    while let Some(directory) = ancestor.filter(|directory| directory.starts_with(&discovered.root))
    {
        for name in testcov::RUNNER_EVIDENCE_FILE_NAMES {
            if deadline_reached(deadline) {
                return Ok(evidence);
            }
            let candidate = directory.join(name);
            if discovered_paths.contains(candidate.as_path())
                || exact_exclusions.contains(&candidate)
                || walk::override_ignored(&overrides, &candidate, &discovered.root)
            {
                continue;
            }
            let Ok(metadata) = std::fs::symlink_metadata(&candidate) else {
                continue;
            };
            if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
                continue;
            }
            let Ok(report_path) = candidate
                .strip_prefix(&discovered.root)
                .map(Path::to_path_buf)
            else {
                continue;
            };
            evidence.push(walk::DiscoveredFile {
                absolute_path: candidate,
                report_path,
            });
        }

        if directory == discovered.root {
            break;
        }
        ancestor = directory.parent();
    }

    Ok(evidence)
}
fn scan_exclusions(cfg: &Config, exclusions: &[PathBuf]) -> Vec<PathBuf> {
    let mut effective = exclusions.to_vec();
    if let Some(path) = &cfg.baseline_path {
        effective.push(path.clone());
    }
    effective
}

fn excluded_report_paths(root: &Path, exclusions: &[PathBuf]) -> Result<HashSet<PathBuf>> {
    Ok(exclusions
        .iter()
        .map(|path| walk::exact_path_identity(path))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter_map(|path| {
            path.strip_prefix(root)
                .ok()
                .filter(|path| !path.as_os_str().is_empty())
                .map(Path::to_path_buf)
        })
        .collect())
}

fn changed_files_for_scope(
    cfg: &Config,
    root: &Path,
    diff_base: Option<&str>,
    excluded: &HashSet<PathBuf>,
) -> Result<Option<HashSet<PathBuf>>> {
    let Some(scope) = cfg.diff_scope.as_ref() else {
        return Ok(None);
    };
    let mut files = git::changed_files_with_base(root, scope, diff_base)?;
    files.retain(|path| !excluded.contains(path));
    Ok(Some(files))
}

fn validate_review_configuration(cfg: &Config) -> Result<()> {
    if cfg.review.is_some() && cfg.diff_scope.is_none() {
        return Err(anyhow::anyhow!(
            "--review requires one of --since, --staged, or --working"
        ));
    }
    if cfg.review.is_some()
        && !(cfg.enabled.complexity || cfg.enabled.markers || cfg.enabled.duplication)
    {
        return Err(anyhow::anyhow!(
            "--review requires complexity, markers, or duplication analysis"
        ));
    }
    Ok(())
}

fn review_changed_files_for_scope(
    cfg: &Config,
    root: &Path,
    target: &Path,
    diff_base: Option<&str>,
    excluded: &HashSet<PathBuf>,
) -> Result<Option<Vec<ReviewChangedFile>>> {
    let (Some(_), Some(scope)) = (&cfg.review, &cfg.diff_scope) else {
        return Ok(None);
    };
    let mut files = git::changed_lines_with_base(root, scope, diff_base)?;
    files.retain(|file| {
        file.path.as_deref().is_some_and(|path| {
            !excluded.contains(path) && path_is_within_target(path, root, target)
        }) || file.old_path.as_deref().is_some_and(|path| {
            !excluded.contains(path) && path_is_within_target(path, root, target)
        })
    });
    Ok(Some(files))
}

fn impact_changed_files(
    cfg: &Config,
    changed_files: Option<&HashSet<PathBuf>>,
    root: &Path,
    target: &Path,
) -> Result<HashSet<PathBuf>> {
    if !cfg.impact {
        return Ok(HashSet::new());
    }
    let Some(changed_files) = changed_files else {
        return Err(anyhow::anyhow!(
            "--impact requires one of --since, --staged, or --working"
        ));
    };
    Ok(changed_files
        .iter()
        .filter(|path| path_is_within_target(path, root, target))
        .cloned()
        .collect())
}

fn context_change_seeds(
    cfg: &Config,
    changed_files: Option<&HashSet<PathBuf>>,
    root: &Path,
    target: &Path,
) -> Result<Option<crate::context::ChangeSeeds>> {
    if !cfg.context || cfg.diff_scope.is_none() {
        return Ok(None);
    }
    let Some(changed_files) = changed_files else {
        return Err(anyhow::anyhow!(
            "change-aware context requires one of --since, --staged, or --working"
        ));
    };
    let paths = changed_files
        .iter()
        .filter(|path| path_is_within_target(path, root, target))
        .cloned()
        .collect();
    let scope = match cfg.diff_scope.as_ref() {
        Some(git::DiffScope::Since(_)) => "since",
        Some(git::DiffScope::Staged) => "staged",
        Some(git::DiffScope::Working) => "working",
        None => unreachable!("checked above"),
    };
    Ok(Some(crate::context::ChangeSeeds {
        scope: scope.to_string(),
        paths,
    }))
}

fn analyze_discovered_files(
    prepared: &PreparedScan,
    cfg: &Config,
    progress: &ScanProgress,
    requirements: ArtifactRequirements,
) -> Result<FileAnalysis> {
    analyze_files(
        &prepared.discovered,
        cfg,
        &prepared.health_policy,
        progress,
        requirements,
        "primary",
        prepared.deadline,
    )
}

mod report;
use report::{assemble_report, path_is_within_target, scan_profile};

mod baseline;
use baseline::compute_baseline_delta;

mod file_analysis;
pub(crate) use file_analysis::analyze_source;
use file_analysis::{
    analyze_cross_file_metrics, analyze_files, analyze_planning_universe, percentage,
};

mod aggregate;
use aggregate::{aggregate, production_duplication_is_complete};

mod duplicates;
use duplicates::{
    instance_has_production_lines, ranked_duplicate_candidates, top_duplicate_blocks_where,
    top_duplicate_findings,
};

mod rollup;
use rollup::rollup_by_dir;

#[cfg(test)]
mod tests;
