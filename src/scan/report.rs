use super::{
    AnalyzedScan, AnalyzerProfile, BTreeMap, Config, DuplicationProfile, Duration,
    ExecutionMetadata, FindingProfile, HashSet, HealthProfile, Instant, Path, PathBuf,
    PlanningAnalysis, PreparedScan, ResourceProfile, Result, SCHEMA_VERSION, ScanArtifacts,
    ScanProfile, ScanProgress, ScanReport, aggregate, cache, compute_baseline_delta, dup, git,
    lang, production_duplication_is_complete, rollup_by_dir,
};
use crate::model::{
    BaselineDelta, ChangeSummary, ContextPlan, DirSummary, FindingCatalog, ImpactAnalysis,
    ReviewReport, RiskEntry, Summary,
};

pub(super) fn assemble_report(
    mut prepared: PreparedScan,
    analyzed: AnalyzedScan,
    planning: Option<&PlanningAnalysis>,
    context_planning_elapsed: Option<Duration>,
    cfg: &Config,
    progress: &ScanProgress,
) -> Result<ScanArtifacts> {
    progress.stage("aggregating report");
    let foundation = build_foundation(&mut prepared, &analyzed, cfg, progress)?;
    let context_assembly_started = cfg.context.then(Instant::now);
    let graphs = build_graphs(&prepared, &analyzed, planning, cfg, progress);
    let context = build_context(ContextAssembly {
        prepared: &prepared,
        analyzed: &analyzed,
        planning,
        risks: &foundation.risk_entries,
        graphs: &graphs,
        cfg,
        progress,
        analysis_elapsed: context_planning_elapsed,
        assembly_started: context_assembly_started,
    })?;
    let impact = build_impact(&prepared, &graphs, cfg, progress);
    let diff_scope = diff_scope_name(cfg);
    let change_summary = build_change_summary(
        &prepared,
        &analyzed,
        planning,
        context.as_ref(),
        impact.as_ref(),
        &graphs,
        cfg,
    );
    let work_scope = crate::work_scope::build(&crate::work_scope::Inputs {
        summary: &foundation.summary,
        diagnostics: &analyzed.diagnostics,
        context: context.as_ref(),
        graph: graphs
            .planning
            .as_ref()
            .or(graphs.scoped.as_ref())
            .map(|analysis| &analysis.signals),
        impact: impact.as_ref(),
        diff_scope,
        changed: &prepared.scoped_changed_files,
    });
    let graph = cfg
        .graph
        .then(|| graphs.scoped.map(|analysis| analysis.report))
        .flatten();
    let symbol_outlines = analyzed.symbol_outlines;
    let graph_facts = analyzed.graph_facts;
    let cache_stats = combined_cache_stats(analyzed.cache_stats, planning);
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
            execution: ExecutionMetadata {
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
            finding_catalog: foundation.finding_catalog,
            summary: foundation.summary,
            work_scope: Some(work_scope),
            files: analyzed.files,
            duplicates: analyzed.duplication,
            directories: foundation.directories,
            baseline: foundation.baseline,
            graph,
            context,
            diagnostics: analyzed.diagnostics,
            impact,
            change_summary,
            review: foundation.review,
        },
        symbol_outlines,
        graph_facts,
        resolver_configs: graphs.resolver_configs,
    })
}

struct ReportFoundation {
    summary: Summary,
    risk_entries: Vec<RiskEntry>,
    finding_catalog: FindingCatalog,
    review: Option<ReviewReport>,
    directories: Vec<DirSummary>,
    baseline: Option<BaselineDelta>,
}

fn build_foundation(
    prepared: &mut PreparedScan,
    analyzed: &AnalyzedScan,
    cfg: &Config,
    progress: &ScanProgress,
) -> Result<ReportFoundation> {
    let (summary, risk_entries) = aggregate(
        &analyzed.files,
        &analyzed.duplication,
        &analyzed.duplicate_coverage,
        &analyzed.test_regions,
        cfg,
        &prepared.health_policy,
        production_duplication_is_complete(cfg.enabled.duplication, &analyzed.diagnostics),
    );
    let finding_catalog =
        crate::findings::build(&analyzed.files, &analyzed.duplication, &risk_entries, cfg);
    let review = build_review(prepared, cfg, progress)?;
    let directories = build_directories(analyzed, cfg, progress);
    let baseline = build_baseline(
        prepared,
        analyzed,
        &summary,
        &finding_catalog,
        cfg,
        progress,
    )?;
    Ok(ReportFoundation {
        summary,
        risk_entries,
        finding_catalog,
        review,
        directories,
        baseline,
    })
}

fn build_review(
    prepared: &mut PreparedScan,
    cfg: &Config,
    progress: &ScanProgress,
) -> Result<Option<ReviewReport>> {
    let (Some(changed), Some(scope)) = (
        prepared.review_changed_files.take(),
        cfg.diff_scope.as_ref(),
    ) else {
        return Ok(None);
    };
    progress.stage("reviewing changed lines");
    crate::review::run(
        &prepared.root,
        cfg,
        scope,
        prepared.diff_base.as_deref(),
        changed,
        &prepared.effective_exclusions,
        prepared.deadline,
    )
    .map(Some)
}

fn build_directories(
    analyzed: &AnalyzedScan,
    cfg: &Config,
    progress: &ScanProgress,
) -> Vec<DirSummary> {
    cfg.by_dir.map_or_else(Vec::new, |depth| {
        progress.stage("rolling up directory summaries");
        rollup_by_dir(&analyzed.files, &analyzed.duplicate_coverage, depth.max(1))
    })
}

fn build_baseline(
    prepared: &PreparedScan,
    analyzed: &AnalyzedScan,
    summary: &Summary,
    finding_catalog: &FindingCatalog,
    cfg: &Config,
    progress: &ScanProgress,
) -> Result<Option<BaselineDelta>> {
    let Some(path) = &cfg.baseline_path else {
        return Ok(None);
    };
    progress.stage("comparing baseline report");
    compute_baseline_delta(
        path,
        summary,
        finding_catalog,
        &prepared.analysis_profile,
        &analyzed.encoding_name,
        &prepared.root,
        &prepared.discovered.target,
    )
    .map(Some)
}

struct GraphAssembly {
    resolver_configs: BTreeMap<String, String>,
    scoped: Option<crate::graph::GraphAnalysis>,
    planning: Option<crate::graph::GraphAnalysis>,
}

fn build_graphs(
    prepared: &PreparedScan,
    analyzed: &AnalyzedScan,
    planning: Option<&PlanningAnalysis>,
    cfg: &Config,
    progress: &ScanProgress,
) -> GraphAssembly {
    let limits = crate::graph::GraphReadLimits {
        deadline: prepared.deadline,
        ..crate::graph::GraphReadLimits::from_config(cfg)
    };
    let resolver_configs = collect_resolver_configs(prepared, analyzed, planning, cfg, &limits);
    let scoped = build_scoped_graph(
        prepared,
        analyzed,
        planning,
        cfg,
        progress,
        &resolver_configs,
        limits,
    );
    let planning = build_planning_graph(prepared, planning, progress, &resolver_configs, limits);
    GraphAssembly {
        resolver_configs,
        scoped,
        planning,
    }
}

fn collect_resolver_configs(
    prepared: &PreparedScan,
    analyzed: &AnalyzedScan,
    planning: Option<&PlanningAnalysis>,
    cfg: &Config,
    limits: &crate::graph::GraphReadLimits,
) -> BTreeMap<String, String> {
    let needed = cfg.graph
        || cfg.context
        || cfg.impact
        || !analyzed.graph_facts.is_empty()
        || planning.is_some_and(|analysis| !analysis.graph_facts.is_empty());
    if !needed {
        return BTreeMap::new();
    }
    let paths = planning.map_or_else(
        || {
            analyzed
                .files
                .iter()
                .map(|file| file.path.clone())
                .collect()
        },
        |analysis| {
            let mut paths = analysis
                .files
                .iter()
                .map(|file| file.path.clone())
                .collect::<Vec<_>>();
            paths.extend(prepared.all_report_paths.iter().cloned());
            paths.sort();
            paths.dedup();
            paths
        },
    );
    crate::graph::collect_resolver_configs(&prepared.root, &paths, limits)
}

fn build_scoped_graph(
    prepared: &PreparedScan,
    analyzed: &AnalyzedScan,
    planning: Option<&PlanningAnalysis>,
    cfg: &Config,
    progress: &ScanProgress,
    resolver_configs: &BTreeMap<String, String>,
    limits: crate::graph::GraphReadLimits,
) -> Option<crate::graph::GraphAnalysis> {
    if !cfg.graph && (!cfg.context || planning.is_some()) {
        return None;
    }
    progress.stage(if cfg.graph {
        "building dependency graph"
    } else {
        "building context dependency signals"
    });
    Some(crate::graph::analyze_with_query_facts(
        &analyzed.files,
        &prepared.root,
        &analyzed.graph_facts,
        Some(resolver_configs),
        limits,
        &cfg.graph_focus,
        cfg.graph_direction,
        cfg.graph_depth,
    ))
}

fn build_planning_graph(
    prepared: &PreparedScan,
    planning: Option<&PlanningAnalysis>,
    progress: &ScanProgress,
    resolver_configs: &BTreeMap<String, String>,
    limits: crate::graph::GraphReadLimits,
) -> Option<crate::graph::GraphAnalysis> {
    let planning = planning?;
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
        Some(resolver_configs),
        limits,
    ))
}

#[derive(Clone, Copy)]
struct ContextAssembly<'a> {
    prepared: &'a PreparedScan,
    analyzed: &'a AnalyzedScan,
    planning: Option<&'a PlanningAnalysis>,
    risks: &'a [RiskEntry],
    graphs: &'a GraphAssembly,
    cfg: &'a Config,
    progress: &'a ScanProgress,
    analysis_elapsed: Option<Duration>,
    assembly_started: Option<Instant>,
}

fn build_context(inputs: ContextAssembly<'_>) -> Result<Option<ContextPlan>> {
    if !inputs.cfg.context {
        return Ok(None);
    }
    inputs.progress.stage("planning agent context");
    let (files, risks, outlines) = inputs.planning.map_or(
        (
            inputs.analyzed.files.as_slice(),
            inputs.risks,
            &inputs.analyzed.symbol_outlines,
        ),
        |planning| {
            (
                planning.files.as_slice(),
                planning.risks.as_slice(),
                &planning.symbol_outlines,
            )
        },
    );
    let mut context = crate::context::build_for_target(
        files,
        risks,
        outlines,
        inputs
            .graphs
            .planning
            .as_ref()
            .or(inputs.graphs.scoped.as_ref())
            .map(|analysis| &analysis.signals),
        crate::context::PlanningPaths {
            root: &inputs.prepared.root,
            target: &inputs.prepared.discovered.target,
        },
        inputs.cfg,
        inputs.prepared.context_changes.as_ref(),
    )?;
    if let Some(planning) = inputs.planning {
        context.planning_diagnostics = Some(planning.diagnostics.clone());
    }
    if let (Some(analysis_elapsed), Some(assembly_started)) =
        (inputs.analysis_elapsed, inputs.assembly_started)
    {
        let elapsed = analysis_elapsed + assembly_started.elapsed();
        context.planning_ms = usize::try_from(elapsed.as_millis()).unwrap_or(usize::MAX);
    }
    Ok(Some(context))
}

fn build_impact(
    prepared: &PreparedScan,
    graphs: &GraphAssembly,
    cfg: &Config,
    progress: &ScanProgress,
) -> Option<ImpactAnalysis> {
    if !cfg.impact {
        return None;
    }
    progress.stage("analyzing change impact");
    Some(graphs.planning.as_ref().map_or_else(
        || {
            crate::graph::impact(
                &prepared.all_report_paths,
                &prepared.root,
                &prepared.impact_changed_files,
            )
        },
        |analysis| crate::graph::impact_from_analysis(analysis, &prepared.impact_changed_files),
    ))
}

fn diff_scope_name(cfg: &Config) -> Option<&'static str> {
    match cfg.diff_scope.as_ref() {
        Some(git::DiffScope::Since(_)) => Some("since"),
        Some(git::DiffScope::Staged) => Some("staged"),
        Some(git::DiffScope::Working) => Some("working"),
        None => None,
    }
}

fn build_change_summary(
    prepared: &PreparedScan,
    analyzed: &AnalyzedScan,
    planning: Option<&PlanningAnalysis>,
    context: Option<&ContextPlan>,
    impact: Option<&ImpactAnalysis>,
    graphs: &GraphAssembly,
    cfg: &Config,
) -> Option<ChangeSummary> {
    if !cfg.change_summary {
        return None;
    }
    let graph_diagnostics = graphs
        .planning
        .as_ref()
        .map(crate::graph::diagnostic_facts)
        .unwrap_or_default();
    Some(crate::change_summary::build(
        crate::change_summary::Inputs {
            scope: diff_scope_name(cfg).unwrap_or("full"),
            changed: &prepared.impact_changed_files,
            context,
            files: planning.map_or(analyzed.files.as_slice(), |analysis| {
                analysis.files.as_slice()
            }),
            impact,
            graph_diagnostics: &graph_diagnostics,
            scan_diagnostics: &analyzed.diagnostics,
            discovery_diagnostics: planning
                .map_or(&analyzed.diagnostics, |analysis| &analysis.diagnostics),
        },
    ))
}

fn combined_cache_stats(
    scan: cache::CacheStats,
    planning: Option<&PlanningAnalysis>,
) -> cache::CacheStats {
    planning.map_or(scan, |analysis| cache::CacheStats {
        enabled: scan.enabled || analysis.cache_stats.enabled,
        hits: scan.hits.saturating_add(analysis.cache_stats.hits),
        misses: scan.misses.saturating_add(analysis.cache_stats.misses),
        enrichments: scan
            .enrichments
            .saturating_add(analysis.cache_stats.enrichments),
    })
}

pub(super) fn scan_profile(cfg: &Config, diff_base: Option<String>) -> ScanProfile {
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
        artifact_policy: if cfg.duplication_include_artifacts {
            "include"
        } else {
            "exclude"
        }
        .to_string(),
    });
    let health = {
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
        let mut excludes = cfg.health_excludes.clone();
        excludes.sort();
        excludes.dedup();
        Some(HealthProfile {
            scope: cfg.health_scope.to_string(),
            includes,
            excludes,
        })
    };

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
            max_churn_deltas_per_commit: cfg.max_churn_deltas_per_commit,
            max_churn_total_deltas: cfg.max_churn_total_deltas,
            max_churn_output_bytes: cfg.max_churn_output_bytes,
            max_git_path_bytes: cfg.max_git_path_bytes,
            max_churn_cache_bytes: cfg.max_churn_cache_bytes,
            load_repository_ignores: Some(cfg.load_repository_ignores),
            max_ignore_file_bytes: cfg.max_ignore_file_bytes,
        }),
    }
}

pub(super) fn effective_duplication_similarity(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

pub(super) fn path_is_within_target(path: &Path, root: &Path, target: &Path) -> bool {
    let Ok(target_relative) = target.strip_prefix(root) else {
        return false;
    };
    target_relative.as_os_str().is_empty() || path.starts_with(target_relative)
}
