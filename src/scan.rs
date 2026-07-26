//! Scan orchestrator: discover files, analyze each in parallel, attach git
//! churn, run duplication detection, and aggregate a [`ScanReport`].

use crate::cache::{self, Cache};
use crate::config::Config;
use crate::debug_log;
use crate::dup::{self, DupInput, DuplicateCoverage};
use crate::git;
use crate::lang;
use crate::metrics::tokens::TokenCounter;
use crate::metrics::{classify, complexity, imports, lines, markers, risk, symbols, testcov};
use crate::model::*;
use crate::parse;
use crate::walk;
use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use std::collections::{BTreeMap, HashSet};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct AnalyzedFile {
    report: FileReport,
    content: String,
    symbol_outlines: Option<Vec<SymbolOutline>>,
    graph_facts: Option<crate::graph::SourceFacts>,
}

struct SourceAnalysis {
    report: FileReport,
    symbol_outlines: Option<Vec<SymbolOutline>>,
    graph_facts: Option<crate::graph::SourceFacts>,
}

enum AnalysisOutcome {
    Analyzed(Box<AnalyzedFile>),
    Unsupported,
    Unreadable,
    Oversized(u64),
    DurationLimit,
}

impl AnalysisOutcome {
    fn status(&self) -> &'static str {
        match self {
            Self::Analyzed(_) => "analyzed",
            Self::Unsupported => "unsupported",
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
    root: PathBuf,
    diff_base: Option<String>,
    analysis_profile: ScanProfile,
    effective_exclusions: Vec<PathBuf>,
    review_changed_files: Option<Vec<ReviewChangedFile>>,
    impact_changed_files: HashSet<PathBuf>,
    context_changes: Option<crate::context::ChangeSeeds>,
    all_report_paths: Vec<PathBuf>,
    deadline: Option<Instant>,
}

struct AnalyzedScan {
    files: Vec<FileReport>,
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

pub(crate) struct ScanArtifacts {
    pub report: ScanReport,
    pub symbol_outlines: BTreeMap<PathBuf, Vec<SymbolOutline>>,
    pub graph_facts: BTreeMap<PathBuf, crate::graph::SourceFacts>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ArtifactRequirements {
    pub symbol_outlines: bool,
    pub graph_facts: bool,
}

impl ScanProgress {
    fn new(files: usize, visible: bool) -> Self {
        let bar = if visible {
            let bar = ProgressBar::new(files as u64);
            bar.set_style(
                ProgressStyle::with_template(
                    "{spinner} scanning files [{bar:30}] {pos}/{len} [{elapsed_precise}]",
                )
                .expect("valid scan progress template")
                .progress_chars("=> "),
            );
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
        self.bar.set_style(
            ProgressStyle::with_template(
                "{spinner} {msg} [{bar:30}] {pos}/{len} [{elapsed_precise}]",
            )
            .expect("valid scan progress template")
            .progress_chars("=> "),
        );
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
        self.bar.set_style(
            ProgressStyle::with_template("{spinner} {msg} [{elapsed_precise}]")
                .expect("valid stage progress template"),
        );
        self.bar.set_message(message);
        self.bar.enable_steady_tick(Duration::from_millis(120));
        self.bar.tick();
    }
}

impl Drop for ScanProgress {
    fn drop(&mut self) {
        self.bar.finish_and_clear();
    }
}

const DEFAULT_CONTEXT_BUDGET: usize = 200_000;

pub fn run(target: &Path, cfg: &Config) -> Result<ScanReport> {
    run_with_exclusions(target, cfg, &[])
}

/// Run a scan while omitting exact filesystem paths such as a CLI output file.
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

pub(crate) fn run_with_artifacts(
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
        .map(|discovered| discovered.files.len())
        .unwrap_or(prepared.discovered.files.len())
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
        planning,
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
    let effective_exclusions = scan_exclusions(cfg, exclusions);
    let target_missing = !target.exists();
    let mut discovered =
        if target_missing && (cfg.impact || cfg.context) && cfg.diff_scope.is_some() {
            walk::discover_missing_file(target)?
        } else {
            walk::discover_with_exclusions_until(target, cfg, &effective_exclusions, deadline)?
        };
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
    if let Some(changed) = &changed_files {
        discovered
            .files
            .retain(|file| changed.contains(&file.report_path));
    }

    Ok(PreparedScan {
        discovered,
        planning_discovered,
        root,
        diff_base,
        analysis_profile,
        effective_exclusions,
        review_changed_files,
        impact_changed_files,
        context_changes,
        all_report_paths,
        deadline,
    })
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
        &prepared.root,
        &prepared.discovered,
        cfg,
        progress,
        requirements,
        "primary",
        prepared.deadline,
    )
}

fn analyze_files(
    root: &Path,
    discovered: &walk::Discovered,
    cfg: &Config,
    progress: &ScanProgress,
    requirements: ArtifactRequirements,
    batch: &'static str,
    deadline: Option<Instant>,
) -> Result<FileAnalysis> {
    let counter = if cfg.enabled.tokens {
        Some(Arc::new(TokenCounter::new(&cfg.encoding)?))
    } else {
        None
    };
    let encoding_name = counter
        .as_ref()
        .map(|c| c.name().to_string())
        .unwrap_or_else(|| cfg.encoding.clone());

    let cache = Cache::open(
        root,
        cfg.use_cache,
        cache::AnalysisProfile::from_config(cfg),
    );

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(cfg.jobs.max(1))
        .build()?;

    let counter_ref = counter.as_deref();
    let files = &discovered.files;
    debug_log::event("file_batch_start", || {
        serde_json::json!({
            "batch": batch,
            "files": files.len(),
            "jobs": cfg.jobs.max(1),
        })
    });
    let outcomes: Vec<AnalysisOutcome> = pool.install(|| {
        files
            .par_iter()
            .map(|file| {
                let debug_started = debug_log::enabled().then(Instant::now);
                debug_log::event("file_start", || {
                    serde_json::json!({
                        "batch": batch,
                        "path": file.report_path.to_string_lossy(),
                    })
                });
                let outcome = if deadline_reached(deadline) {
                    AnalysisOutcome::DurationLimit
                } else {
                    analyze_file(
                        &file.absolute_path,
                        &file.report_path,
                        cfg,
                        counter_ref,
                        &cache,
                        requirements,
                    )
                };
                if let Some(started) = debug_started {
                    debug_log::event("file_end", || {
                        serde_json::json!({
                            "batch": batch,
                            "path": file.report_path.to_string_lossy(),
                            "status": outcome.status(),
                            "duration_ms": u64::try_from(started.elapsed().as_millis())
                                .unwrap_or(u64::MAX),
                        })
                    });
                }
                progress.file_completed();
                outcome
            })
            .collect()
    });
    progress.stage("processing analyzed files");

    let mut diagnostics = ScanDiagnostics {
        discovered_files: discovered.observed_files,
        walker_errors: discovered.walker_errors,
        oversized_files: discovered.oversized_files,
        oversized_bytes: discovered.oversized_bytes,
        files_omitted_by_limit: discovered.files_omitted_by_limit,
        files_omitted_count_incomplete: discovered.files_omitted_count_incomplete,
        bytes_omitted_by_limit: discovered.bytes_omitted_by_limit,
        scan_truncated: discovered.scan_truncated,
        duration_limit_reached: discovered.duration_limit_reached,
        ..ScanDiagnostics::default()
    };
    let mut analyzed = Vec::new();
    for outcome in outcomes {
        match outcome {
            AnalysisOutcome::Analyzed(file) => analyzed.push(*file),
            AnalysisOutcome::Unsupported => diagnostics.unsupported_files += 1,
            AnalysisOutcome::Unreadable => diagnostics.unreadable_files += 1,
            AnalysisOutcome::Oversized(bytes) => {
                diagnostics.oversized_files = diagnostics.oversized_files.saturating_add(1);
                diagnostics.oversized_bytes = diagnostics.oversized_bytes.saturating_add(bytes);
                diagnostics.scan_truncated = true;
            }
            AnalysisOutcome::DurationLimit => {
                mark_duration_limit(&mut diagnostics, 1);
            }
        }
    }
    diagnostics.analyzed_files = analyzed.len();
    debug_log::event("file_batch_end", || {
        serde_json::json!({
            "batch": batch,
            "discovered": diagnostics.discovered_files,
            "analyzed": diagnostics.analyzed_files,
            "unsupported": diagnostics.unsupported_files,
            "unreadable": diagnostics.unreadable_files,
            "walker_errors": diagnostics.walker_errors,
        })
    });

    Ok(FileAnalysis {
        analyzed,
        diagnostics,
        encoding_name,
        cache,
    })
}

fn analyze_cross_file_metrics(
    prepared: &PreparedScan,
    mut file_analysis: FileAnalysis,
    cfg: &Config,
    progress: &ScanProgress,
) -> AnalyzedScan {
    let analyzed = &mut file_analysis.analyzed;

    if deadline_reached(prepared.deadline) {
        mark_duration_limit(&mut file_analysis.diagnostics, 0);
    } else {
        attach_churn(
            &prepared.root,
            analyzed,
            cfg,
            progress,
            "analyzing git history",
        );
    }

    let (
        mut duplication,
        duplicate_coverage,
        duplication_token_counts,
        duplication_formats,
        type2_diagnostics,
    ) = if cfg.enabled.duplication && !deadline_reached(prepared.deadline) {
        let inputs: Vec<DupInput> = analyzed
            .iter()
            .filter(|file| {
                lang::detect(&file.report.path).is_some_and(|info| cfg.includes_in_health(info))
            })
            .map(|a| DupInput {
                path: a.report.path.clone(),
                content: a.content.clone(),
            })
            .collect();
        let mut record_type2_progress = |detail: dup::fuzzy::Type2Progress| {
            debug_log::event("type2_progress", move || serde_json::json!(detail));
        };
        let type2_progress: Option<&mut dyn FnMut(dup::fuzzy::Type2Progress)> =
            debug_log::enabled().then_some(&mut record_type2_progress);
        let detection = dup::analyze_with_diagnostics(
            &inputs,
            cfg.min_dup_tokens,
            cfg.min_dup_lines,
            cfg.near_dup_min_similarity,
            dup::DetectionOptions {
                mode: cfg.duplication_mode,
                format_scope: cfg.duplication_format_scope,
                report_snippets: cfg.duplication_report_snippets,
            },
            |stage| progress.stage(stage.message()),
            type2_progress,
        );
        (
            detection.duplication,
            detection.coverage,
            detection.token_counts,
            detection.formats,
            detection.type2_diagnostics,
        )
    } else {
        (
            Duplication::default(),
            DuplicateCoverage::default(),
            BTreeMap::new(),
            BTreeMap::new(),
            dup::fuzzy::Type2Diagnostics::default(),
        )
    };
    if deadline_reached(prepared.deadline) {
        mark_duration_limit(&mut file_analysis.diagnostics, 0);
    }
    apply_type2_diagnostics(&mut file_analysis.diagnostics, type2_diagnostics);
    if type2_diagnostics.truncated {
        debug_log::event("type2_analysis_partial", || {
            serde_json::json!({
                "pools_truncated": type2_diagnostics.pools_truncated,
                "candidate_buckets_skipped": type2_diagnostics.candidate_buckets_skipped,
                "candidate_buckets_partially_selected": type2_diagnostics
                    .candidate_buckets_partially_selected,
                "seed_pairs_skipped": type2_diagnostics.seed_pairs_skipped,
                "match_limit_reached": type2_diagnostics.match_limit_reached,
                "suppression_limit_reached": type2_diagnostics.suppression_limit_reached,
                "matches_skipped_during_suppression": type2_diagnostics
                    .matches_skipped_during_suppression,
            })
        });
    }

    progress.stage("saving incremental cache");
    let complete_root_scan =
        cfg.diff_scope.is_none() && prepared.discovered.target == prepared.root;
    if let Err(error) = file_analysis.cache.save(complete_root_scan) {
        debug_log::event(
            "cache_save_error",
            || serde_json::json!({ "batch": "primary", "message": error.to_string() }),
        );
    }
    let cache_stats = file_analysis.cache.stats();

    let symbol_outlines = file_analysis
        .analyzed
        .iter()
        .filter_map(|analysis| {
            analysis
                .symbol_outlines
                .as_ref()
                .filter(|outlines| !outlines.is_empty())
                .map(|outlines| (analysis.report.path.clone(), outlines.clone()))
        })
        .collect();
    let graph_facts = file_analysis
        .analyzed
        .iter()
        .filter_map(|analysis| {
            analysis
                .graph_facts
                .clone()
                .map(|facts| (analysis.report.path.clone(), facts))
        })
        .collect();
    let mut files: Vec<FileReport> = file_analysis
        .analyzed
        .into_iter()
        .map(|a| a.report)
        .collect();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    if cfg.enabled.duplication {
        duplication.file_coverage = build_file_duplication_coverage(
            &files,
            &duplicate_coverage,
            &duplication_token_counts,
            &duplication_formats,
        );
    }

    AnalyzedScan {
        files,
        symbol_outlines,
        graph_facts,
        duplication,
        duplicate_coverage,
        diagnostics: file_analysis.diagnostics,
        encoding_name: file_analysis.encoding_name,
        cache_stats,
    }
}

fn apply_type2_diagnostics(diagnostics: &mut ScanDiagnostics, type2: dup::fuzzy::Type2Diagnostics) {
    diagnostics.type2_analysis_partial = type2.truncated;
    diagnostics.type2_pools_truncated = type2.pools_truncated;
    diagnostics.type2_candidate_buckets_skipped = type2.candidate_buckets_skipped;
    diagnostics.type2_candidate_buckets_partially_selected =
        type2.candidate_buckets_partially_selected;
    diagnostics.type2_seed_pairs_skipped = type2.seed_pairs_skipped;
    diagnostics.type2_match_limit_reached = type2.match_limit_reached;
    diagnostics.type2_suppression_limit_reached = type2.suppression_limit_reached;
    diagnostics.type2_matches_skipped_during_suppression = type2.matches_skipped_during_suppression;
}

fn analyze_planning_universe(
    prepared: &PreparedScan,
    cfg: &Config,
    progress: &ScanProgress,
    requirements: ArtifactRequirements,
) -> Result<Option<PlanningAnalysis>> {
    let Some(discovered) = prepared.planning_discovered.as_ref() else {
        return Ok(None);
    };
    progress.files_stage(
        "analyzing change-aware planning universe",
        discovered.files.len(),
    );
    let mut analysis = analyze_files(
        &prepared.root,
        discovered,
        cfg,
        progress,
        requirements,
        "planning_universe",
        prepared.deadline,
    )?;
    if deadline_reached(prepared.deadline) {
        mark_duration_limit(&mut analysis.diagnostics, 0);
    } else {
        attach_churn(
            &prepared.root,
            &mut analysis.analyzed,
            cfg,
            progress,
            "analyzing planning-universe history",
        );
    }
    progress.stage("saving planning-universe cache");
    if let Err(error) = analysis.cache.save(true) {
        debug_log::event("cache_save_error", || {
            serde_json::json!({
                "batch": "planning_universe",
                "message": error.to_string(),
            })
        });
    }
    let cache_stats = analysis.cache.stats();

    let symbol_outlines = analysis
        .analyzed
        .iter()
        .filter_map(|file| {
            file.symbol_outlines
                .as_ref()
                .filter(|outlines| !outlines.is_empty())
                .map(|outlines| (file.report.path.clone(), outlines.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    let graph_facts = analysis
        .analyzed
        .iter()
        .filter_map(|file| {
            file.graph_facts
                .clone()
                .map(|facts| (file.report.path.clone(), facts))
        })
        .collect::<BTreeMap<_, _>>();
    let mut files = analysis
        .analyzed
        .into_iter()
        .map(|file| file.report)
        .collect::<Vec<_>>();
    files.sort_by(|left, right| left.path.cmp(&right.path));
    let (_, risks) = aggregate(
        &files,
        &Duplication::default(),
        &DuplicateCoverage::default(),
        cfg,
    );
    Ok(Some(PlanningAnalysis {
        files,
        symbol_outlines,
        graph_facts,
        risks,
        diagnostics: analysis.diagnostics,
        cache_stats,
    }))
}

fn attach_churn(
    root: &Path,
    analyzed: &mut [AnalyzedFile],
    cfg: &Config,
    progress: &ScanProgress,
    stage: &str,
) {
    if !cfg.enabled.churn {
        return;
    }
    progress.stage(stage);
    let paths = analyzed
        .iter()
        .map(|file| file.report.path.clone())
        .collect::<Vec<_>>();
    let churn = git::collect_with_cache(root, &paths, cfg.churn_max_commits, cfg.use_cache);
    for file in analyzed {
        if let Some(value) = churn.get(&file.report.path) {
            file.report.churn = Some(value.clone());
        }
    }
}

fn assemble_report(
    prepared: PreparedScan,
    analyzed: AnalyzedScan,
    planning: Option<PlanningAnalysis>,
    context_planning_elapsed: Option<Duration>,
    cfg: &Config,
    progress: &ScanProgress,
) -> Result<ScanArtifacts> {
    progress.stage("aggregating report");
    let (summary, risk_entries) = aggregate(
        &analyzed.files,
        &analyzed.duplication,
        &analyzed.duplicate_coverage,
        cfg,
    );
    let finding_catalog =
        crate::findings::build(&analyzed.files, &analyzed.duplication, &risk_entries, cfg);
    let review = match (prepared.review_changed_files, cfg.diff_scope.as_ref()) {
        (Some(changed), Some(scope)) => {
            progress.stage("reviewing changed lines");
            Some(crate::review::run(
                &prepared.root,
                cfg,
                scope,
                prepared.diff_base.as_deref(),
                changed,
                &prepared.effective_exclusions,
                prepared.deadline,
            )?)
        }
        _ => None,
    };

    let directories = match cfg.by_dir {
        Some(depth) => {
            progress.stage("rolling up directory summaries");
            rollup_by_dir(&analyzed.files, &analyzed.duplicate_coverage, depth.max(1))
        }
        None => Vec::new(),
    };

    let baseline = match &cfg.baseline_path {
        Some(p) => {
            progress.stage("comparing baseline report");
            Some(compute_baseline_delta(
                p,
                &summary,
                &finding_catalog,
                &prepared.analysis_profile,
                &analyzed.encoding_name,
                &prepared.root,
                &prepared.discovered.target,
            )?)
        }
        None => None,
    };

    let context_assembly_started = cfg.context.then(Instant::now);
    let scoped_graph_analysis = if cfg.graph || (cfg.context && planning.is_none()) {
        progress.stage(if cfg.graph {
            "building dependency graph"
        } else {
            "building context dependency signals"
        });
        Some(crate::graph::analyze_with_query_facts(
            &analyzed.files,
            &prepared.root,
            &analyzed.graph_facts,
            &cfg.graph_focus,
            cfg.graph_direction,
            cfg.graph_depth,
        ))
    } else {
        None
    };
    let planning_graph_analysis = if let Some(planning) = &planning {
        progress.stage("building change-aware planning topology");
        let existing = prepared
            .all_report_paths
            .iter()
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .collect::<HashSet<_>>();
        let virtual_paths = prepared
            .context_changes
            .iter()
            .flat_map(|changes| changes.paths.iter())
            .filter(|path| !prepared.root.join(path).exists())
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .filter(|path| !existing.contains(path))
            .collect::<HashSet<_>>();
        let mut paths = prepared.all_report_paths.clone();
        paths.extend(virtual_paths.iter().map(PathBuf::from));
        Some(crate::graph::analyze_paths_with_facts(
            &paths,
            &prepared.root,
            &virtual_paths,
            &planning.graph_facts,
        ))
    } else {
        None
    };
    let mut context = if cfg.context {
        progress.stage("planning agent context");
        let (files, risks, outlines) = planning.as_ref().map_or(
            (
                analyzed.files.as_slice(),
                risk_entries.as_slice(),
                &analyzed.symbol_outlines,
            ),
            |planning| {
                (
                    planning.files.as_slice(),
                    planning.risks.as_slice(),
                    &planning.symbol_outlines,
                )
            },
        );
        Some(crate::context::build_for_target(
            files,
            risks,
            outlines,
            planning_graph_analysis
                .as_ref()
                .or(scoped_graph_analysis.as_ref())
                .map(|analysis| &analysis.signals),
            crate::context::PlanningPaths {
                root: &prepared.root,
                target: &prepared.discovered.target,
            },
            cfg,
            prepared.context_changes.as_ref(),
        )?)
    } else {
        None
    };
    if let (Some(context), Some(planning)) = (context.as_mut(), planning.as_ref()) {
        context.planning_diagnostics = Some(planning.diagnostics.clone());
    }
    if let (Some(context), Some(analysis_elapsed), Some(assembly_started)) = (
        context.as_mut(),
        context_planning_elapsed,
        context_assembly_started,
    ) {
        let elapsed = analysis_elapsed + assembly_started.elapsed();
        context.planning_ms = usize::try_from(elapsed.as_millis()).unwrap_or(usize::MAX);
    }
    let graph = if cfg.graph {
        scoped_graph_analysis.map(|analysis| analysis.report)
    } else {
        None
    };

    let impact = if cfg.impact {
        progress.stage("analyzing change impact");
        Some(if let Some(analysis) = planning_graph_analysis.as_ref() {
            crate::graph::impact_from_analysis(analysis, &prepared.impact_changed_files)
        } else {
            crate::graph::impact(
                &prepared.all_report_paths,
                &prepared.root,
                &prepared.impact_changed_files,
            )
        })
    } else {
        None
    };

    let symbol_outlines = analyzed.symbol_outlines;
    let graph_facts = analyzed.graph_facts;
    let cache_stats =
        planning
            .as_ref()
            .map_or(analyzed.cache_stats, |planning| cache::CacheStats {
                enabled: analyzed.cache_stats.enabled || planning.cache_stats.enabled,
                hits: analyzed
                    .cache_stats
                    .hits
                    .saturating_add(planning.cache_stats.hits),
                misses: analyzed
                    .cache_stats
                    .misses
                    .saturating_add(planning.cache_stats.misses),
                enrichments: analyzed
                    .cache_stats
                    .enrichments
                    .saturating_add(planning.cache_stats.enrichments),
            });
    let graph_fact_files = planning
        .as_ref()
        .map_or(graph_facts.len(), |planning| planning.graph_facts.len());
    Ok(ScanArtifacts {
        report: ScanReport {
            schema_version: SCHEMA_VERSION.to_string(),
            root: prepared.root,
            target: prepared.discovered.target,
            generated_at: chrono::Utc::now().to_rfc3339(),
            encoding: analyzed.encoding_name,
            analysis_profile: Some(prepared.analysis_profile),
            execution: crate::model::ExecutionMetadata {
                profile: cfg.execution_profile.clone(),
                config_mode: cfg.config_mode.clone(),
                global_config: cfg.global_config_path.clone(),
                project_config: cfg.project_config_path.clone(),
                safety_limits: cfg.safety_limits.clone(),
                stage_ms: BTreeMap::new(),
                cache_enabled: cache_stats.enabled,
                cache_hits: cache_stats.hits,
                cache_misses: cache_stats.misses,
                cache_enrichments: cache_stats.enrichments,
                graph_fact_files,
            },
            finding_catalog,
            summary,
            files: analyzed.files,
            duplicates: analyzed.duplication,
            directories,
            baseline,
            graph,
            context,
            diagnostics: analyzed.diagnostics,
            impact,
            review,
        },
        symbol_outlines,
        graph_facts,
    })
}

fn scan_profile(cfg: &Config, diff_base: Option<String>) -> ScanProfile {
    let diff_scope = match cfg.diff_scope.as_ref() {
        None => "full",
        Some(git::DiffScope::Since(_)) => "since",
        Some(git::DiffScope::Staged) => "staged",
        Some(git::DiffScope::Working) => "working",
    }
    .to_string();
    let duplication = cfg.enabled.duplication.then(|| DuplicationProfile {
        min_tokens: cfg.min_dup_tokens,
        min_lines: cfg.min_dup_lines,
        min_similarity: effective_duplication_similarity(cfg.near_dup_min_similarity),
        mode: cfg.duplication_mode.to_string(),
        format_scope: match cfg.duplication_format_scope {
            dup::DuplicationFormatScope::Exact => "exact",
            dup::DuplicationFormatScope::Compatible => "compatible",
            dup::DuplicationFormatScope::All => "all",
        }
        .to_string(),
    });
    let health = (cfg.enabled.markers || cfg.enabled.duplication).then(|| {
        let mut includes = if cfg.health_scope == lang::HealthScope::Source {
            cfg.health_includes
                .iter()
                .map(|format| format.language_name().to_string())
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        includes.sort();
        includes.dedup();
        HealthProfile {
            scope: cfg.health_scope.to_string(),
            includes,
        }
    });

    let mut finding_markers = if cfg.enabled.markers {
        cfg.markers
            .iter()
            .filter(|marker| !marker.is_empty())
            .cloned()
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    finding_markers.sort();
    finding_markers.dedup();

    ScanProfile {
        analyzers: AnalyzerProfile {
            tokens: cfg.enabled.tokens,
            complexity: cfg.enabled.complexity,
            imports: cfg.enabled.imports,
            markers: cfg.enabled.markers,
            duplication: cfg.enabled.duplication,
            churn: cfg.enabled.churn,
        },
        diff_scope,
        diff_base,
        duplication,
        health,
        findings: Some(FindingProfile {
            catalog_version: crate::findings::CATALOG_VERSION,
            max_complexity: cfg.max_complexity,
            markers: finding_markers,
            risk_algorithm_version: crate::findings::RISK_ALGORITHM_VERSION,
            risk_threshold: crate::findings::RISK_THRESHOLD,
        }),
        resources: Some(ResourceProfile {
            max_file_bytes: cfg.max_file_bytes,
            max_total_bytes: cfg.max_total_bytes,
            max_files: cfg.max_files,
            max_git_blob_bytes: cfg.max_git_blob_bytes,
            max_scan_seconds: cfg.max_scan_seconds,
        }),
    }
}

fn effective_duplication_similarity(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn path_is_within_target(path: &Path, root: &Path, target: &Path) -> bool {
    let Ok(target_relative) = target.strip_prefix(root) else {
        return false;
    };
    target_relative.as_os_str().is_empty() || path.starts_with(target_relative)
}

/// Compare two summaries and produce deltas + regression flags. Pure/testable.
fn baseline_delta(
    baseline: &Summary,
    baseline_generated_at: &str,
    current: &Summary,
    profile: &ScanProfile,
) -> BaselineDelta {
    let mut metrics = vec![MetricDelta {
        metric: "files".to_string(),
        baseline: baseline.files as f64,
        current: current.files as f64,
        delta: current.files as f64 - baseline.files as f64,
    }];
    if profile.analyzers.tokens {
        metrics.push(MetricDelta {
            metric: "tokens".to_string(),
            baseline: baseline.tokens as f64,
            current: current.tokens as f64,
            delta: current.tokens as f64 - baseline.tokens as f64,
        });
    }
    metrics.push(MetricDelta {
        metric: "sloc".to_string(),
        baseline: baseline.sloc as f64,
        current: current.sloc as f64,
        delta: current.sloc as f64 - baseline.sloc as f64,
    });
    if profile.analyzers.duplication {
        metrics.push(MetricDelta {
            metric: "duplicated_pct".to_string(),
            baseline: baseline.duplication.duplicated_pct,
            current: current.duplication.duplicated_pct,
            delta: current.duplication.duplicated_pct - baseline.duplication.duplicated_pct,
        });
    }
    if profile.analyzers.complexity {
        metrics.extend([
            MetricDelta {
                metric: "cyclomatic_avg".to_string(),
                baseline: baseline.complexity.cyclomatic_avg,
                current: current.complexity.cyclomatic_avg,
                delta: current.complexity.cyclomatic_avg - baseline.complexity.cyclomatic_avg,
            },
            MetricDelta {
                metric: "cyclomatic_max".to_string(),
                baseline: baseline.complexity.cyclomatic_max as f64,
                current: current.complexity.cyclomatic_max as f64,
                delta: current.complexity.cyclomatic_max as f64
                    - baseline.complexity.cyclomatic_max as f64,
            },
            MetricDelta {
                metric: "mi_avg".to_string(),
                baseline: baseline.complexity.mi_avg,
                current: current.complexity.mi_avg,
                delta: current.complexity.mi_avg - baseline.complexity.mi_avg,
            },
            MetricDelta {
                metric: "mi_min".to_string(),
                baseline: baseline.complexity.mi_min,
                current: current.complexity.mi_min,
                delta: current.complexity.mi_min - baseline.complexity.mi_min,
            },
        ]);
    }
    metrics.push(MetricDelta {
        metric: "untested_source_files".to_string(),
        baseline: baseline.test_presence.untested_source_files as f64,
        current: current.test_presence.untested_source_files as f64,
        delta: current.test_presence.untested_source_files as f64
            - baseline.test_presence.untested_source_files as f64,
    });

    let mut regressions = Vec::new();

    let dup_base = baseline.duplication.duplicated_pct;
    let dup_cur = current.duplication.duplicated_pct;
    if profile.analyzers.duplication && dup_cur > dup_base + 0.01 {
        regressions.push(format!(
            "duplication +{:.1}% (now {:.1}%)",
            dup_cur - dup_base,
            dup_cur
        ));
    }

    let mi_avg_base = baseline.complexity.mi_avg;
    let mi_avg_cur = current.complexity.mi_avg;
    if profile.analyzers.complexity && mi_avg_cur < mi_avg_base - 0.01 {
        regressions.push(format!(
            "maintainability avg -{:.0} (now {:.0})",
            mi_avg_base - mi_avg_cur,
            mi_avg_cur
        ));
    }

    let mi_min_base = baseline.complexity.mi_min;
    let mi_min_cur = current.complexity.mi_min;
    if profile.analyzers.complexity && mi_min_cur < mi_min_base - 0.01 {
        regressions.push(format!(
            "maintainability min -{:.0} (now {:.0})",
            mi_min_base - mi_min_cur,
            mi_min_cur
        ));
    }

    let cycmax_base = baseline.complexity.cyclomatic_max;
    let cycmax_cur = current.complexity.cyclomatic_max;
    if profile.analyzers.complexity && cycmax_cur > cycmax_base {
        regressions.push(format!(
            "max cyclomatic +{} (now {})",
            cycmax_cur as i64 - cycmax_base as i64,
            cycmax_cur as i64
        ));
    }

    let untested_base = baseline.test_presence.untested_source_files;
    let untested_cur = current.test_presence.untested_source_files;
    if untested_cur > untested_base {
        regressions.push(format!(
            "sources without matching tests +{} (now {})",
            untested_cur as i64 - untested_base as i64,
            untested_cur as i64
        ));
    }

    let regressed = !regressions.is_empty();
    BaselineDelta {
        baseline_generated_at: baseline_generated_at.to_string(),
        metrics,
        regressions,
        regressed,
        finding_changes: crate::findings::unavailable(
            "baseline does not contain a compatible finding catalog",
        ),
    }
}

fn compute_baseline_delta(
    path: &Path,
    current: &Summary,
    current_catalog: &FindingCatalog,
    current_profile: &ScanProfile,
    current_encoding: &str,
    current_root: &Path,
    current_target: &Path,
) -> Result<BaselineDelta> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read baseline {}: {e}", path.display()))?;
    #[derive(serde::Deserialize)]
    struct BaselineInput {
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

    let prior: BaselineInput = serde_json::from_str(&text).map_err(|e| {
        anyhow::anyhow!(
            "baseline {} is not a reposcout JSON report: {e}",
            path.display()
        )
    })?;
    match prior.analysis_profile.as_ref() {
        Some(profile)
            if scan_profiles_compatible_except_base(profile, current_profile)
                && profile.diff_base != current_profile.diff_base =>
        {
            return Err(anyhow::anyhow!(
                "baseline diff base tree does not match the current scan"
            ));
        }
        Some(profile) if !scan_profiles_compatible(profile, current_profile) => {
            return Err(anyhow::anyhow!(
                "baseline analyzer profile does not match the current scan"
            ));
        }
        None => {
            return Err(anyhow::anyhow!(
                "baseline lacks analyzer profile metadata; regenerate it with the current reposcout"
            ));
        }
        _ => {}
    }
    if current_profile.analyzers.tokens && prior.encoding != current_encoding {
        return Err(anyhow::anyhow!(
            "baseline token encoding does not match the current scan"
        ));
    }
    if target_scope(&prior.root, &prior.target) != target_scope(current_root, current_target) {
        return Err(anyhow::anyhow!(
            "baseline target scope does not match the current scan"
        ));
    }

    let mut delta = baseline_delta(
        &prior.summary,
        &prior.generated_at,
        current,
        current_profile,
    );
    delta.finding_changes = match prior.analysis_profile.as_ref() {
        Some(profile)
            if profile.findings == current_profile.findings
                && prior.finding_catalog.version > 0 =>
        {
            crate::findings::compare(&prior.finding_catalog, current_catalog)
        }
        Some(_) if prior.finding_catalog.version > 0 => {
            crate::findings::unavailable("baseline finding profile does not match the current scan")
        }
        _ => crate::findings::unavailable("baseline does not contain a finding catalog"),
    };
    let new = delta.finding_changes.counts.new;
    let worsened = delta.finding_changes.counts.worsened;
    if new > 0 || worsened > 0 {
        delta.regressions.push(format!(
            "finding regressions: {new} new, {worsened} worsened"
        ));
        delta.regressed = true;
    }
    Ok(delta)
}

fn scan_profiles_compatible(left: &ScanProfile, right: &ScanProfile) -> bool {
    scan_profiles_compatible_except_base(left, right) && left.diff_base == right.diff_base
}

fn scan_profiles_compatible_except_base(left: &ScanProfile, right: &ScanProfile) -> bool {
    left.analyzers == right.analyzers
        && left.diff_scope == right.diff_scope
        && left.duplication == right.duplication
        && left.health == right.health
        && left.resources == right.resources
}

fn target_scope(root: &Path, target: &Path) -> String {
    target
        .strip_prefix(root)
        .ok()
        .filter(|relative| !relative.as_os_str().is_empty())
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|| ".".to_string())
}

fn analyze_file(
    path: &Path,
    report_path: &Path,
    cfg: &Config,
    counter: Option<&TokenCounter>,
    cache: &Cache,
    requirements: ArtifactRequirements,
) -> AnalysisOutcome {
    if lang::detect(path).is_none() {
        return AnalysisOutcome::Unsupported;
    }
    let content = match walk::read_text_bounded(path, cfg.max_file_bytes) {
        walk::BoundedText::Content(content) => content,
        walk::BoundedText::Oversized(bytes) => return AnalysisOutcome::Oversized(bytes),
        walk::BoundedText::Unreadable => return AnalysisOutcome::Unreadable,
    };
    let rel = report_path.to_path_buf();
    let rel_str = rel.to_string_lossy().to_string();

    let hash = xxhash_rust::xxh3::xxh3_64(content.as_bytes());
    if let Some(mut cached) = cache.get(&rel_str, hash) {
        let needs_outlines = requirements.symbol_outlines && cached.symbol_outlines.is_none();
        let needs_graph = requirements.graph_facts && cached.graph_facts.is_none();
        let mut enriched = false;
        if needs_outlines || needs_graph {
            let first_class = lang::detect(report_path).and_then(|info| info.first_class);
            let tree = first_class.and_then(|fc| parse::parse(fc, &content));
            if needs_outlines {
                cached.symbol_outlines = Some(match (first_class, tree.as_ref()) {
                    (Some(fc), Some(tree)) => symbols::analyze(fc, &content, tree).outlines,
                    _ => Vec::new(),
                });
                enriched = true;
            }
            if needs_graph && let Some(fc) = first_class {
                cached.graph_facts = Some(match tree.as_ref() {
                    Some(tree) => crate::graph::extract_source_facts_from_tree(
                        fc,
                        &rel_str,
                        &content,
                        tree.root_node(),
                    ),
                    None => crate::graph::SourceFacts::parse_error(),
                });
                enriched = true;
            }
        }
        if enriched {
            cache.record_enrichment();
            cache.put(
                &rel_str,
                hash,
                &cached.report,
                cached.symbol_outlines.as_deref(),
                cached.graph_facts.as_ref(),
            );
        }
        return AnalysisOutcome::Analyzed(Box::new(AnalyzedFile {
            report: cached.report,
            content,
            symbol_outlines: cached.symbol_outlines,
            graph_facts: cached.graph_facts,
        }));
    }

    let analysis = analyze_source_details(report_path, &content, cfg, counter, requirements)
        .expect("filesystem path and report path use the same recognized language");

    cache.put(
        &rel_str,
        hash,
        &analysis.report,
        analysis.symbol_outlines.as_deref(),
        analysis.graph_facts.as_ref(),
    );
    AnalysisOutcome::Analyzed(Box::new(AnalyzedFile {
        report: analysis.report,
        content,
        symbol_outlines: analysis.symbol_outlines,
        graph_facts: analysis.graph_facts,
    }))
}

/// Analyze already-loaded UTF-8 source without filesystem or cache coupling.
/// Git review snapshots use the same analyzer implementation through this seam.
pub(crate) fn analyze_source(
    report_path: &Path,
    content: &str,
    cfg: &Config,
    counter: Option<&TokenCounter>,
) -> Option<FileReport> {
    analyze_source_details(
        report_path,
        content,
        cfg,
        counter,
        ArtifactRequirements::default(),
    )
    .map(|analysis| analysis.report)
}

fn analyze_source_details(
    report_path: &Path,
    content: &str,
    cfg: &Config,
    counter: Option<&TokenCounter>,
    requirements: ArtifactRequirements,
) -> Option<SourceAnalysis> {
    let info = lang::detect(report_path)?;
    let rel = report_path.to_path_buf();
    let rel_str = rel.to_string_lossy().to_string();
    let tokens = counter.map(|c| c.count(content)).unwrap_or(0);

    // First-class line metrics also consume the syntax tree, so parse even
    // when structural analyzers are disabled.
    let tree = info.first_class.and_then(|fc| parse::parse(fc, content));
    let line_stats = lines::measure(info, content, tree.as_ref());
    let marker_scan = if cfg.enabled.markers && cfg.includes_in_health(info) {
        match (info.first_class, tree.as_ref()) {
            (Some(_), Some(tree)) => markers::scan_detailed_in_tree(content, &cfg.markers, tree),
            _ => markers::scan_detailed(content, &cfg.markers),
        }
    } else {
        markers::MarkerScan::default()
    };
    let inline_test_tree =
        if matches!(info.first_class, Some(lang::FirstClass::Rust)) && tree.is_none() {
            parse::parse(lang::FirstClass::Rust, content)
        } else {
            None
        };

    let (complexity_opt, approximate) = if cfg.enabled.complexity && info.is_code() {
        let (c, approx) = complexity::analyze(info, content, tree.as_ref(), &line_stats);
        (Some(c), approx)
    } else {
        (None, false)
    };

    let import_list = if cfg.enabled.imports {
        match (info.first_class, tree.as_ref()) {
            (Some(fc), Some(t)) => imports::extract(fc, content, t),
            _ => Vec::new(),
        }
    } else {
        Vec::new()
    };

    let needs_symbol_counts = cfg.enabled.complexity || cfg.enabled.imports;
    let (sym, symbol_outlines) = match (info.first_class, tree.as_ref()) {
        (Some(fc), Some(tree)) if requirements.symbol_outlines => {
            let analysis = symbols::analyze(fc, content, tree);
            (
                needs_symbol_counts.then_some(analysis.counts),
                Some(analysis.outlines),
            )
        }
        (Some(fc), Some(tree)) if needs_symbol_counts => {
            (Some(symbols::count(fc, content, tree)), None)
        }
        (_, _) if requirements.symbol_outlines => (None, Some(Vec::new())),
        _ => (None, None),
    };
    let graph_facts = if requirements.graph_facts {
        match (info.first_class, tree.as_ref()) {
            (Some(fc), Some(tree)) => Some(crate::graph::extract_source_facts_from_tree(
                fc,
                &rel_str,
                content,
                tree.root_node(),
            )),
            (Some(_), None) => Some(crate::graph::SourceFacts::parse_error()),
            (None, _) => None,
        }
    } else {
        None
    };

    let skip = classify::skip_hint(&rel_str, content);

    let has_inline_tests = matches!(
        (info.first_class, tree.as_ref().or(inline_test_tree.as_ref())),
        (Some(lang::FirstClass::Rust), Some(tree))
            if testcov::has_inline_rust_tests(content, tree)
    );

    let comment_ratio = if line_stats.loc > 0 {
        line_stats.comment_lines as f64 / line_stats.loc as f64
    } else {
        0.0
    };

    Some(SourceAnalysis {
        report: FileReport {
            path: rel,
            language: info.name.to_string(),
            bytes: content.len() as u64,
            tokens,
            loc: line_stats.loc,
            sloc: line_stats.sloc,
            comment_lines: line_stats.comment_lines,
            comment_ratio,
            line_metrics_approximate: line_stats.approximate,
            complexity: complexity_opt,
            imports: import_list,
            markers: marker_scan.counts,
            marker_occurrences: marker_scan.occurrences,
            churn: None,
            approximate,
            symbols: sym,
            skip_hint: skip,
            has_inline_tests,
        },
        symbol_outlines,
        graph_facts,
    })
}

fn build_file_duplication_coverage(
    files: &[FileReport],
    coverage: &DuplicateCoverage,
    token_counts: &BTreeMap<PathBuf, usize>,
    formats: &BTreeMap<PathBuf, String>,
) -> Vec<FileDuplication> {
    files
        .iter()
        .filter(|file| token_counts.contains_key(&file.path))
        .map(|file| {
            let tokens = token_counts.get(&file.path).copied().unwrap_or(0);
            let duplicated_lines = coverage.covered_lines(&file.path);
            let duplicated_tokens = coverage.covered_tokens(&file.path);
            FileDuplication {
                path: file.path.clone(),
                format: formats
                    .get(&file.path)
                    .cloned()
                    .unwrap_or_else(|| file.language.clone()),
                lines: file.loc,
                tokens,
                duplicated_lines,
                duplicated_tokens,
                duplicated_lines_pct: percentage(duplicated_lines, file.loc),
                duplicated_tokens_pct: percentage(duplicated_tokens, tokens),
            }
        })
        .collect()
}

fn percentage(part: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        part as f64 / total as f64 * 100.0
    }
}

struct AggregateAccum {
    languages: BTreeMap<String, LanguageStat>,
    cyclomatic_total: u64,
    cognitive_total: u64,
    function_count: usize,
    mi_sum: f64,
    mi_count: usize,
    mi_min: f64,
    function_hotspots: Vec<FunctionHotspot>,
}

fn aggregate(
    files: &[FileReport],
    dup: &Duplication,
    duplicate_coverage: &DuplicateCoverage,
    cfg: &Config,
) -> (Summary, Vec<RiskEntry>) {
    let mut s = Summary::default();
    let accumulated = accumulate_file_metrics(files, &mut s);
    finish_complexity_and_languages(&mut s, accumulated, cfg);
    summarize_duplication(&mut s, dup, duplicate_coverage, cfg);
    populate_file_rankings(&mut s, files, cfg);
    let (test_presence, top_risks, all_risk_entries) = test_and_risk_summary(files, cfg);
    s.test_presence = test_presence;
    s.top_risks = top_risks;
    let source_duplication = source_duplication_pct(files, duplicate_coverage);
    s.assessment = build_assessment(&s, source_duplication, cfg.enabled);
    (s, all_risk_entries)
}

fn accumulate_file_metrics(files: &[FileReport], summary: &mut Summary) -> AggregateAccum {
    let mut accumulated = AggregateAccum {
        languages: BTreeMap::new(),
        cyclomatic_total: 0,
        cognitive_total: 0,
        function_count: 0,
        mi_sum: 0.0,
        mi_count: 0,
        mi_min: f64::INFINITY,
        function_hotspots: Vec::new(),
    };
    for f in files {
        summary.files += 1;
        summary.bytes += f.bytes;
        summary.tokens += f.tokens;
        summary.loc += f.loc;
        summary.sloc += f.sloc;
        summary.comment_lines += f.comment_lines;
        if lang::is_source_name(&f.language) {
            summary.source.files += 1;
            summary.source.bytes += f.bytes;
            summary.source.tokens += f.tokens;
            summary.source.loc += f.loc;
            summary.source.sloc += f.sloc;
            summary.source.comment_lines += f.comment_lines;
        }
        if f.line_metrics_approximate {
            summary.line_metrics_approximate_files += 1;
        }

        let e = accumulated.languages.entry(f.language.clone()).or_default();
        e.name = f.language.clone();
        e.source = lang::is_source_name(&f.language);
        e.files += 1;
        e.bytes += f.bytes;
        e.loc += f.loc;
        e.sloc += f.sloc;
        e.comment_lines += f.comment_lines;
        e.tokens += f.tokens;

        for (k, v) in &f.markers {
            *summary.markers.entry(k.clone()).or_insert(0) += v;
        }

        if let Some(sym) = &f.symbols {
            summary.symbols.functions += sym.functions;
            summary.symbols.types += sym.types;
            summary.symbols.exports += sym.exports;
        }

        if let Some(c) = &f.complexity {
            accumulated.mi_sum += c.maintainability_index;
            accumulated.mi_min = accumulated.mi_min.min(c.maintainability_index);
            accumulated.mi_count += 1;
            if f.approximate {
                summary.complexity.approximate_files += 1;
            }
            for func in &c.functions {
                accumulated.cyclomatic_total += func.cyclomatic as u64;
                accumulated.cognitive_total += func.cognitive as u64;
                accumulated.function_count += 1;
                summary.complexity.cyclomatic_max =
                    summary.complexity.cyclomatic_max.max(func.cyclomatic);
                summary.complexity.cognitive_max =
                    summary.complexity.cognitive_max.max(func.cognitive);
                accumulated.function_hotspots.push(FunctionHotspot {
                    path: f.path.clone(),
                    name: func.name.clone(),
                    line: func.line,
                    cyclomatic: func.cyclomatic,
                    cognitive: func.cognitive,
                    max_nesting: func.max_nesting,
                });
            }
        }
    }
    accumulated
}

fn finish_complexity_and_languages(
    summary: &mut Summary,
    mut accumulated: AggregateAccum,
    cfg: &Config,
) {
    summary.comment_ratio = if summary.loc > 0 {
        summary.comment_lines as f64 / summary.loc as f64
    } else {
        0.0
    };

    summary.complexity.cyclomatic_total = accumulated.cyclomatic_total;
    summary.complexity.cognitive_total = accumulated.cognitive_total;
    summary.complexity.functions = accumulated.function_count;
    summary.complexity.cyclomatic_threshold = cfg.max_complexity;
    if accumulated.function_count > 0 {
        summary.complexity.cyclomatic_avg =
            accumulated.cyclomatic_total as f64 / accumulated.function_count as f64;
        summary.complexity.cognitive_avg =
            accumulated.cognitive_total as f64 / accumulated.function_count as f64;
    }
    if accumulated.mi_count > 0 {
        summary.complexity.mi_avg = accumulated.mi_sum / accumulated.mi_count as f64;
        summary.complexity.mi_min = accumulated.mi_min;
    }

    accumulated.function_hotspots.sort_by(|a, b| {
        b.cyclomatic
            .cmp(&a.cyclomatic)
            .then(b.cognitive.cmp(&a.cognitive))
            .then(b.max_nesting.cmp(&a.max_nesting))
            .then(a.path.cmp(&b.path))
            .then(a.line.cmp(&b.line))
    });
    summary.complexity.functions_over_threshold = accumulated
        .function_hotspots
        .iter()
        .filter(|function| function.cyclomatic > cfg.max_complexity)
        .count();
    summary.complexity_violations = accumulated
        .function_hotspots
        .iter()
        .filter(|function| function.cyclomatic > cfg.max_complexity)
        .take(cfg.top)
        .cloned()
        .collect();
    accumulated.function_hotspots.truncate(cfg.top);
    summary.top_functions = accumulated.function_hotspots;

    let mut languages: Vec<LanguageStat> = accumulated.languages.into_values().collect();
    languages.sort_by(|a, b| b.sloc.cmp(&a.sloc).then(b.tokens.cmp(&a.tokens)));
    summary.languages = languages;
}

fn summarize_duplication(
    summary: &mut Summary,
    dup: &Duplication,
    duplicate_coverage: &DuplicateCoverage,
    cfg: &Config,
) {
    let duplicated_lines = duplicate_coverage.total_lines();
    let duplicated_tokens = duplicate_coverage.total_tokens();
    let analyzed_lines = dup.file_coverage.iter().map(|file| file.lines).sum();
    let analyzed_tokens = dup.file_coverage.iter().map(|file| file.tokens).sum();
    let mut duplication_by_language: BTreeMap<String, LanguageDuplication> = BTreeMap::new();
    for file in &dup.file_coverage {
        let language = duplication_by_language
            .entry(file.format.clone())
            .or_insert_with(|| LanguageDuplication {
                name: file.format.clone(),
                ..LanguageDuplication::default()
            });
        language.files += 1;
        language.lines += file.lines;
        language.tokens += file.tokens;
        language.duplicated_lines += file.duplicated_lines;
        language.duplicated_tokens += file.duplicated_tokens;
    }
    let path_formats = dup
        .file_coverage
        .iter()
        .map(|file| (&file.path, file.format.as_str()))
        .collect::<BTreeMap<_, _>>();
    for (groups, exact) in [(&dup.exact, true), (&dup.near, false)] {
        for group in groups {
            let involved = group
                .instances
                .iter()
                .filter_map(|instance| path_formats.get(&instance.path).copied())
                .collect::<HashSet<_>>();
            for format in involved {
                if let Some(language) = duplication_by_language.get_mut(format) {
                    if exact {
                        language.exact_groups += 1;
                    } else {
                        language.near_groups += 1;
                    }
                }
            }
        }
    }
    let mut duplication_by_language = duplication_by_language
        .into_values()
        .map(|mut language| {
            language.duplicated_lines_pct = percentage(language.duplicated_lines, language.lines);
            language.duplicated_tokens_pct =
                percentage(language.duplicated_tokens, language.tokens);
            language
        })
        .collect::<Vec<_>>();
    duplication_by_language.sort_by(|a, b| {
        b.duplicated_lines
            .cmp(&a.duplicated_lines)
            .then_with(|| b.duplicated_tokens.cmp(&a.duplicated_tokens))
            .then_with(|| a.name.cmp(&b.name))
    });
    summary.duplication = DuplicationSummary {
        exact_groups: dup.exact.len(),
        near_groups: dup.near.len(),
        duplicated_lines,
        duplicated_pct: percentage(duplicated_lines, analyzed_lines),
        analyzed_lines,
        duplicated_tokens,
        analyzed_tokens,
        duplicated_tokens_pct: percentage(duplicated_tokens, analyzed_tokens),
        by_language: duplication_by_language,
    };
    summary.top_duplicates = top_duplicate_blocks(dup, cfg.top);
    summary.top_duplicate_findings = top_duplicate_findings(dup, cfg.top);
}

fn populate_file_rankings(summary: &mut Summary, files: &[FileReport], cfg: &Config) {
    let mut by_tokens: Vec<FileRef> = files
        .iter()
        .map(|f| FileRef {
            path: f.path.clone(),
            tokens: f.tokens,
        })
        .collect();
    by_tokens.sort_by_key(|f| std::cmp::Reverse(f.tokens));
    by_tokens.truncate(cfg.top);
    summary.top_token_files = by_tokens;

    let mut source_by_tokens: Vec<FileRef> = files
        .iter()
        .filter(|file| lang::is_source_name(&file.language))
        .map(|file| FileRef {
            path: file.path.clone(),
            tokens: file.tokens,
        })
        .collect();
    source_by_tokens.sort_by_key(|file| std::cmp::Reverse(file.tokens));
    source_by_tokens.truncate(cfg.top);
    summary.top_source_token_files = source_by_tokens;

    let mut hotspots: Vec<Hotspot> = files
        .iter()
        .filter_map(|f| {
            let commits = f.churn.as_ref().map(|c| c.commits).unwrap_or(0);
            // Hotspots are "churn × complexity", so only code files with a
            // computed complexity qualify — this keeps docs/config (README,
            // package.json, …) out of the ranking even when they churn a lot.
            let cyclomatic = f.complexity.as_ref()?.cyclomatic;
            if commits == 0 {
                return None;
            }
            Some(Hotspot {
                path: f.path.clone(),
                commits,
                cyclomatic,
                score: commits as f64 * (cyclomatic as f64 + 1.0),
            })
        })
        .collect();
    hotspots.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    hotspots.truncate(cfg.top);
    summary.top_hotspots = hotspots;

    let mut skip_candidates: Vec<SkipCandidate> = files
        .iter()
        .filter_map(|f| {
            f.skip_hint.as_ref().map(|reason| SkipCandidate {
                path: f.path.to_string_lossy().into_owned(),
                reason: reason.clone(),
                tokens: f.tokens,
            })
        })
        .collect();
    skip_candidates.sort_by_key(|c| std::cmp::Reverse(c.tokens));
    skip_candidates.truncate(cfg.top);
    summary.skip_candidates = skip_candidates;
}

fn test_and_risk_summary(
    files: &[FileReport],
    cfg: &Config,
) -> (TestPresence, Vec<RiskEntry>, Vec<RiskEntry>) {
    let mut test_stem_set: HashSet<String> = HashSet::new();
    let mut test_file_count = 0usize;
    let mut source_files: Vec<&FileReport> = Vec::new();

    for f in files {
        if !lang::detect(&f.path).map(|i| i.is_code()).unwrap_or(false) {
            continue;
        }
        let path_str = f.path.to_string_lossy();
        if testcov::is_test_file(path_str.as_ref()) {
            test_file_count += 1;
            for key in testcov::test_stem_keys(path_str.as_ref()) {
                test_stem_set.insert(key);
            }
        } else {
            source_files.push(f);
        }
    }

    let mut untested_source_files = 0usize;
    let mut untested_samples: Vec<String> = Vec::new();
    for f in source_files.iter().copied() {
        if !has_matching_test(f, &test_stem_set) {
            untested_source_files += 1;
            if untested_samples.len() < cfg.top {
                untested_samples.push(f.path.to_string_lossy().into_owned());
            }
        }
    }

    let test_presence = TestPresence {
        test_files: test_file_count,
        source_files: source_files.len(),
        untested_source_files,
        untested_samples,
    };

    let mut risk_entries: Vec<RiskEntry> = source_files
        .iter()
        .copied()
        .filter_map(|file| risk::entry(file, !has_matching_test(file, &test_stem_set)))
        .collect();

    risk_entries.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.sloc.cmp(&a.sloc))
    });
    let all_risk_entries = risk_entries.clone();
    risk_entries.truncate(cfg.top);
    (test_presence, risk_entries, all_risk_entries)
}

fn has_matching_test(file: &FileReport, test_stem_set: &HashSet<String>) -> bool {
    if file.has_inline_tests {
        return true;
    }
    let path = file.path.to_string_lossy();
    test_stem_set.contains(&testcov::source_stem(path.as_ref()))
}

fn source_duplication_pct(files: &[FileReport], coverage: &DuplicateCoverage) -> f64 {
    let source_files = files.iter().filter(|file| {
        lang::detect(&file.path).is_some_and(|info| info.is_code())
            && !testcov::is_test_file(file.path.to_string_lossy().as_ref())
    });
    let (duplicated_lines, lines) = source_files.fold((0usize, 0usize), |totals, file| {
        (
            totals.0 + coverage.covered_lines(&file.path),
            totals.1 + file.loc,
        )
    });
    percentage(duplicated_lines, lines)
}

fn build_assessment(
    summary: &Summary,
    source_duplication: f64,
    enabled: crate::config::Enabled,
) -> Assessment {
    let token_budget = DEFAULT_CONTEXT_BUDGET;
    let fits_context_known = enabled.tokens;
    let fits_context = fits_context_known && summary.tokens <= token_budget;
    let cleanup_worth_complete = enabled.complexity && enabled.duplication && enabled.churn;
    let mut unavailable_signals = Vec::new();
    if !enabled.tokens {
        unavailable_signals.push("tokens".to_string());
    }
    if !enabled.complexity {
        unavailable_signals.push("complexity".to_string());
    }
    if !enabled.duplication {
        unavailable_signals.push("duplication".to_string());
    }
    if !enabled.churn {
        unavailable_signals.push("churn".to_string());
    }

    let mut cleanup_reasons: Vec<String> = Vec::new();

    if source_duplication > 15.0 {
        cleanup_reasons.push(format!(
            "high source duplication ({source_duplication:.1}%)"
        ));
    }
    let has_maintainability = summary.complexity.functions > 0
        || summary.complexity.approximate_files > 0
        || summary.complexity.mi_avg > 0.0;
    if has_maintainability && summary.complexity.mi_avg < 20.0 {
        let band = if summary.complexity.mi_avg < 10.0 {
            "low"
        } else {
            "moderate"
        };
        cleanup_reasons.push(format!(
            "{band} maintainability (MI avg {:.0})",
            summary.complexity.mi_avg
        ));
    }
    let high_risk_count = summary.top_risks.iter().filter(|r| r.score >= 0.7).count();
    if high_risk_count >= 3 {
        cleanup_reasons.push("several high-risk files".to_string());
    }
    let src_count = summary.test_presence.source_files;
    let untested_count = summary.test_presence.untested_source_files;
    if src_count > 0 && (untested_count as f64 / src_count as f64) > 0.5 {
        cleanup_reasons.push("many source files have no matching test file".to_string());
    }

    let signal_count = cleanup_reasons.len();
    let cleanup_worth = if signal_count >= 3 {
        "high"
    } else if signal_count >= 1 {
        "medium"
    } else {
        "low"
    }
    .to_string();

    let mut assessment_reasons: Vec<String> = Vec::new();
    if !fits_context_known {
        assessment_reasons.push("context fit unavailable (tokens analyzer disabled)".to_string());
    } else if fits_context {
        assessment_reasons.push(format!("fits in {token_budget}-token context"));
    } else {
        assessment_reasons.push(format!(
            "exceeds {token_budget}-token context budget ({} tokens)",
            summary.tokens
        ));
    }
    assessment_reasons.extend(cleanup_reasons);

    Assessment {
        fits_context_known,
        fits_context,
        token_budget,
        cleanup_worth,
        cleanup_worth_complete,
        unavailable_signals,
        reasons: assessment_reasons,
    }
}

/// Rank the highest-impact duplicate blocks across both detectors. Impact is
/// the number of lines removable by de-duplicating a block, `lines * (copies
/// - 1)`; ties break toward larger blocks and more copies. Locations are capped
/// so the list stays compact even in `--summary` output.
fn top_duplicate_blocks(dup: &Duplication, top: usize) -> Vec<DuplicateBlock> {
    const MAX_LOCATIONS: usize = 10;
    let mut blocks: Vec<DuplicateBlock> = dup
        .exact
        .iter()
        .chain(dup.near.iter())
        .filter_map(|g| {
            let copies = g.instances.len();
            if copies < 2 {
                return None;
            }
            let mut locations: Vec<String> = g
                .instances
                .iter()
                .map(|i| {
                    let ends_at_next_line_start =
                        i.end_column == 1 && i.end_line > i.start_line && i.end_byte > i.start_byte;
                    let end_line = if ends_at_next_line_start {
                        i.end_line - 1
                    } else {
                        i.end_line
                    };
                    format!("{}:{}-{end_line}", i.path.display(), i.start_line)
                })
                .collect();
            locations.sort();
            locations.truncate(MAX_LOCATIONS);
            Some(DuplicateBlock {
                lines: g.lines,
                tokens: g.tokens,
                similarity: g.similarity,
                copies,
                duplicated_lines: g.lines * (copies - 1),
                locations,
            })
        })
        .collect();

    blocks.sort_by(|a, b| {
        b.duplicated_lines
            .cmp(&a.duplicated_lines)
            .then_with(|| b.lines.cmp(&a.lines))
            .then_with(|| b.copies.cmp(&a.copies))
    });
    blocks.truncate(top);
    blocks
}

fn top_duplicate_findings(dup: &Duplication, top: usize) -> Vec<DuplicateFindingSummary> {
    let mut findings = dup
        .findings
        .iter()
        .map(|finding| DuplicateFindingSummary {
            id: finding.id.clone(),
            kind: finding.kind.clone(),
            format: finding.format.clone(),
            tokens: finding.tokens,
            lines: finding.lines_a.max(finding.lines_b),
            similarity: finding.similarity,
            removable_lines: finding.removable_lines,
            locations: vec![
                format!(
                    "{}:{}:{}-{}:{}",
                    finding.fragment_a.path.display(),
                    finding.fragment_a.start_line,
                    finding.fragment_a.start_column,
                    finding.fragment_a.end_line,
                    finding.fragment_a.end_column
                ),
                format!(
                    "{}:{}:{}-{}:{}",
                    finding.fragment_b.path.display(),
                    finding.fragment_b.start_line,
                    finding.fragment_b.start_column,
                    finding.fragment_b.end_line,
                    finding.fragment_b.end_column
                ),
            ],
        })
        .collect::<Vec<_>>();
    findings.sort_by(|a, b| {
        b.removable_lines
            .cmp(&a.removable_lines)
            .then_with(|| b.tokens.cmp(&a.tokens))
            .then_with(|| b.similarity.total_cmp(&a.similarity))
            .then_with(|| a.id.cmp(&b.id))
    });
    findings.truncate(top);
    findings
}

/// The directory bucket for a file's relative path at the given depth.
///
/// ```text
/// dir_bucket("src/metrics/x.rs", 1) == "src"
/// dir_bucket("src/metrics/x.rs", 2) == "src/metrics"
/// dir_bucket("README.md",         1) == "."   (file at repo root)
/// ```
///
/// Backslashes are normalised to `/` before splitting. If `depth` exceeds the
/// number of parent components the result is clamped to however many exist.
fn dir_bucket(rel: &str, depth: usize) -> String {
    let rel = rel.replace('\\', "/");
    let parts: Vec<&str> = rel.split('/').collect();
    // Drop the filename (last component).
    let parent = if parts.len() > 1 {
        &parts[..parts.len() - 1]
    } else {
        &[][..]
    };
    if parent.is_empty() {
        return ".".to_string();
    }
    let take = depth.min(parent.len());
    parent[..take].join("/")
}

fn rollup_by_dir(
    files: &[FileReport],
    duplicate_coverage: &DuplicateCoverage,
    depth: usize,
) -> Vec<DirSummary> {
    struct Accum {
        summary: DirSummary,
        cyc_sum: u64,
        cyc_count: usize,
        mi_sum: f64,
        mi_count: usize,
    }

    // Build the global test-stem set exactly as aggregate() does.
    let mut test_stem_set: HashSet<String> = HashSet::new();
    for f in files {
        if !lang::detect(&f.path).map(|i| i.is_code()).unwrap_or(false) {
            continue;
        }
        let path_str = f.path.to_string_lossy();
        if testcov::is_test_file(path_str.as_ref()) {
            for key in testcov::test_stem_keys(path_str.as_ref()) {
                test_stem_set.insert(key);
            }
        }
    }

    let mut buckets: BTreeMap<String, Accum> = BTreeMap::new();

    for f in files {
        let path_str = f.path.to_string_lossy();
        let bucket = dir_bucket(path_str.as_ref(), depth);
        let key = bucket.clone();
        let entry = buckets.entry(key).or_insert_with(move || Accum {
            summary: DirSummary {
                path: bucket,
                ..DirSummary::default()
            },
            cyc_sum: 0,
            cyc_count: 0,
            mi_sum: 0.0,
            mi_count: 0,
        });

        entry.summary.files += 1;
        entry.summary.tokens += f.tokens;
        entry.summary.loc += f.loc;
        entry.summary.sloc += f.sloc;
        entry.summary.duplicated_lines += duplicate_coverage.covered_lines(&f.path);

        let is_code = lang::detect(&f.path).map(|i| i.is_code()).unwrap_or(false);
        if is_code {
            if let Some(c) = &f.complexity {
                entry.mi_sum += c.maintainability_index;
                entry.mi_count += 1;
                for function in &c.functions {
                    entry.cyc_sum += function.cyclomatic as u64;
                    entry.cyc_count += 1;
                    entry.summary.cyclomatic_max =
                        entry.summary.cyclomatic_max.max(function.cyclomatic);
                }
            }

            if !testcov::is_test_file(path_str.as_ref()) {
                let stem = testcov::source_stem(path_str.as_ref());
                let tested = f.has_inline_tests || test_stem_set.contains(&stem);
                if !tested {
                    entry.summary.untested_source_files += 1;
                }
            }
        }
    }

    let mut result: Vec<DirSummary> = buckets
        .into_values()
        .map(|a| {
            let mut s = a.summary;
            if a.cyc_count > 0 {
                s.cyclomatic_avg = a.cyc_sum as f64 / a.cyc_count as f64;
            }
            if a.mi_count > 0 {
                s.mi_avg = a.mi_sum / a.mi_count as f64;
            }
            s
        })
        .collect();

    result.sort_by(|a, b| b.tokens.cmp(&a.tokens).then_with(|| a.path.cmp(&b.path)));
    result
}

#[cfg(test)]
mod tests {
    use super::{
        analyze_source, apply_type2_diagnostics, baseline_delta, build_assessment, dir_bucket,
        scan_profile, scan_profiles_compatible, source_duplication_pct, top_duplicate_blocks,
    };
    use crate::config::{Config, Enabled};
    use crate::dup::{DuplicateCoverage, fuzzy::Type2Diagnostics};
    use crate::git::DiffScope;
    use crate::model::{CloneGroup, CloneInstance, Duplication, ScanDiagnostics, Summary};
    use std::path::Path;

    #[test]
    fn type2_limits_mark_scan_diagnostics_partial() {
        let mut diagnostics = ScanDiagnostics::default();

        apply_type2_diagnostics(
            &mut diagnostics,
            Type2Diagnostics {
                truncated: true,
                pools_truncated: 2,
                candidate_buckets_skipped: 11,
                candidate_buckets_partially_selected: 1,
                seed_pairs_skipped: 99,
                match_limit_reached: true,
                suppression_limit_reached: true,
                matches_skipped_during_suppression: 7,
            },
        );

        assert!(diagnostics.type2_analysis_partial);
        assert_eq!(diagnostics.type2_pools_truncated, 2);
        assert_eq!(diagnostics.type2_candidate_buckets_skipped, 11);
        assert_eq!(diagnostics.type2_seed_pairs_skipped, 99);
        assert!(diagnostics.type2_match_limit_reached);
        assert!(diagnostics.type2_suppression_limit_reached);
        assert_eq!(diagnostics.type2_matches_skipped_during_suppression, 7);
    }

    #[test]
    fn dir_bucket_depth1_single_component() {
        assert_eq!(dir_bucket("src/model.rs", 1), "src");
    }

    #[test]
    fn dir_bucket_depth2_nested() {
        assert_eq!(dir_bucket("src/metrics/tokens.rs", 2), "src/metrics");
    }

    #[test]
    fn dir_bucket_root_file_is_dot() {
        assert_eq!(dir_bucket("README.md", 1), ".");
    }

    #[test]
    fn dir_bucket_depth_clamps_to_available_components() {
        // depth=5 but only 2 parent components → clamp to "src/metrics"
        assert_eq!(dir_bucket("src/metrics/tokens.rs", 5), "src/metrics");
    }

    #[test]
    fn dir_bucket_depth1_on_deep_path() {
        assert_eq!(dir_bucket("src/metrics/tokens.rs", 1), "src");
    }

    #[test]
    fn dir_bucket_normalises_backslash() {
        assert_eq!(dir_bucket("src\\metrics\\tokens.rs", 1), "src");
    }

    #[test]
    fn baseline_delta_detects_regression() {
        let mut base = Summary::default();
        let mut cur = Summary::default();
        base.duplication.duplicated_pct = 5.0;
        cur.duplication.duplicated_pct = 9.0;
        base.complexity.mi_avg = 70.0;
        cur.complexity.mi_avg = 60.0;

        let delta = baseline_delta(
            &base,
            "2020-01-01T00:00:00Z",
            &cur,
            &scan_profile(&Config::default(), None),
        );

        let dup_delta = delta
            .metrics
            .iter()
            .find(|m| m.metric == "duplicated_pct")
            .expect("duplicated_pct metric must be present");
        assert!(
            (dup_delta.delta - 4.0).abs() < f64::EPSILON,
            "expected delta 4.0, got {}",
            dup_delta.delta
        );

        assert!(delta.regressed, "expected regressed == true");
        assert!(
            delta.regressions.iter().any(|r| r.contains("duplication")),
            "expected a duplication regression message"
        );
        assert!(
            delta
                .regressions
                .iter()
                .any(|r| r.contains("maintainability")),
            "expected a maintainability regression message"
        );
    }

    #[test]
    fn baseline_delta_no_regression_on_identical() {
        let base = Summary::default();
        let cur = Summary::default();
        let delta = baseline_delta(
            &base,
            "2020-01-01T00:00:00Z",
            &cur,
            &scan_profile(&Config::default(), None),
        );
        assert!(!delta.regressed, "identical summaries must not regress");
    }

    #[test]
    fn diff_profiles_require_the_same_resolved_base_tree() {
        let cfg = Config {
            diff_scope: Some(DiffScope::Since("main".to_string())),
            ..Config::default()
        };
        let first = scan_profile(&cfg, Some("tree-a".to_string()));
        let alias = scan_profile(&cfg, Some("tree-a".to_string()));
        let different = scan_profile(&cfg, Some("tree-b".to_string()));

        assert!(scan_profiles_compatible(&first, &alias));
        assert!(!scan_profiles_compatible(&first, &different));
    }

    #[test]
    fn baseline_profiles_require_matching_resource_limits() {
        let first = scan_profile(&Config::default(), None);
        let changed = scan_profile(
            &Config {
                max_file_bytes: Config::default().max_file_bytes / 2,
                ..Config::default()
            },
            None,
        );

        assert!(!scan_profiles_compatible(&first, &changed));
    }

    #[test]
    fn assessment_uses_explicit_test_matching_and_source_duplication_wording() {
        let mut summary = Summary::default();
        summary.test_presence.source_files = 4;
        summary.test_presence.untested_source_files = 3;

        let at_threshold = build_assessment(&summary, 15.0, Enabled::default());
        assert!(
            !at_threshold
                .reasons
                .iter()
                .any(|reason| reason.contains("source duplication"))
        );

        let above_threshold = build_assessment(&summary, 15.1, Enabled::default());
        assert!(
            above_threshold
                .reasons
                .iter()
                .any(|reason| reason == "high source duplication (15.1%)")
        );
        assert!(
            above_threshold
                .reasons
                .iter()
                .any(|reason| reason == "many source files have no matching test file")
        );
    }

    #[test]
    fn assessment_uses_microsoft_maintainability_bands() {
        let mut summary = Summary::default();
        summary.complexity.functions = 1;

        summary.complexity.mi_avg = 20.0;
        assert!(
            build_assessment(&summary, 0.0, Enabled::default())
                .reasons
                .iter()
                .all(|reason| !reason.contains("maintainability"))
        );

        summary.complexity.mi_avg = 15.0;
        assert!(
            build_assessment(&summary, 0.0, Enabled::default())
                .reasons
                .iter()
                .any(|reason| reason == "moderate maintainability (MI avg 15)")
        );

        summary.complexity.mi_avg = 5.0;
        assert!(
            build_assessment(&summary, 0.0, Enabled::default())
                .reasons
                .iter()
                .any(|reason| reason == "low maintainability (MI avg 5)")
        );
    }

    #[test]
    fn assessment_does_not_claim_context_fit_when_tokens_are_disabled() {
        let assessment = build_assessment(
            &Summary::default(),
            0.0,
            Enabled {
                tokens: false,
                complexity: true,
                imports: false,
                markers: false,
                duplication: false,
                churn: false,
                lines: true,
            },
        );

        assert!(!assessment.fits_context_known);
        assert!(!assessment.fits_context);
        assert!(!assessment.cleanup_worth_complete);
        assert_eq!(
            assessment.unavailable_signals,
            ["tokens", "duplication", "churn"]
        );
        assert!(
            assessment
                .reasons
                .iter()
                .any(|reason| reason.contains("context fit unavailable"))
        );
    }

    #[test]
    fn source_duplication_excludes_test_files() {
        let cfg = Config::default();
        let content = "fn example() {}\n".repeat(10);
        let source = analyze_source(Path::new("src/example.rs"), &content, &cfg, None).unwrap();
        let test = analyze_source(Path::new("tests/example.rs"), &content, &cfg, None).unwrap();
        let instance = |path: &str, end_line: usize| CloneInstance {
            path: path.into(),
            start_line: 1,
            end_line,
            start_column: 1,
            end_column: 2,
            ..CloneInstance::default()
        };
        let duplication = Duplication {
            exact: vec![CloneGroup {
                instances: vec![
                    instance("src/example.rs", 2),
                    instance("tests/example.rs", 10),
                ],
                ..CloneGroup::default()
            }],
            ..Duplication::default()
        };
        let coverage = DuplicateCoverage::from_duplication(&duplication);

        assert_eq!(source_duplication_pct(&[source, test], &coverage), 20.0);
    }

    #[test]
    fn source_analysis_limits_first_class_markers_to_comments() {
        let cfg = Config::default();
        let report = analyze_source(
            Path::new("src/example.rs"),
            "const TODO: &str = \"TODO\";\n// TODO real work\n",
            &cfg,
            None,
        )
        .unwrap();

        assert_eq!(report.markers.get("TODO"), Some(&1));
        assert_eq!(report.marker_occurrences.len(), 1);
        assert_eq!(report.marker_occurrences[0].line, 2);
    }

    #[test]
    fn baseline_regression_describes_test_matching_heuristic() {
        let baseline = Summary::default();
        let mut current = Summary::default();
        current.test_presence.untested_source_files = 1;

        let delta = baseline_delta(
            &baseline,
            "2020-01-01T00:00:00Z",
            &current,
            &scan_profile(&Config::default(), None),
        );

        assert!(
            delta
                .regressions
                .iter()
                .any(|reason| reason == "sources without matching tests +1 (now 1)")
        );
    }

    #[test]
    fn top_duplicate_locations_use_occupied_end_lines() {
        let instance = |path: &str| CloneInstance {
            path: path.into(),
            start_line: 1,
            end_line: 2,
            start_column: 1,
            end_column: 1,
            start_byte: 0,
            end_byte: 7,
            ..CloneInstance::default()
        };
        let duplication = Duplication {
            exact: vec![CloneGroup {
                lines: 1,
                tokens: 3,
                similarity: 1.0,
                instances: vec![instance("a.rs"), instance("b.rs")],
                ..CloneGroup::default()
            }],
            ..Duplication::default()
        };

        let blocks = top_duplicate_blocks(&duplication, 10);
        assert_eq!(blocks[0].locations, ["a.rs:1-1", "b.rs:1-1"]);
    }
}
