use super::{
    AnalysisOutcome, AnalyzedFile, AnalyzedScan, Arc, ArtifactRequirements, BTreeMap, Cache,
    Config, DupInput, DuplicateCoverage, Duplication, FileAnalysis, FileDuplication, FileReport,
    HealthPolicy, Instant, IntoParallelRefIterator, ParallelIterator, Path, PathBuf,
    PlanningAnalysis, PreparedScan, Result, ScanDiagnostics, ScanProgress, SourceAnalysis,
    TokenCounter, aggregate, cache, classify, complexity, deadline_reached, debug_log, dup, git,
    imports, lang, lines, mark_duration_limit, markers, parse, symbols, testcov, usize_to_f64,
    walk,
};
use crate::model::{Complexity, LineRange, SymbolCounts, SymbolOutline};
use tree_sitter::Tree;

#[expect(
    clippy::too_many_lines,
    reason = "parallel file analysis owns one bounded lifecycle from cache lookup through deterministic outcome aggregation"
)]
pub(super) fn analyze_files(
    discovered: &walk::Discovered,
    cfg: &Config,
    health_policy: &HealthPolicy,
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
        .map_or_else(|| cfg.encoding.clone(), |c| c.name().to_string());

    let cache = Cache::open(
        &discovered.root,
        cfg.use_cache,
        &cache::AnalysisProfile::from_config(cfg),
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
                        health_policy,
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
            AnalysisOutcome::Unsupported(path) => {
                diagnostics.unsupported_files += 1;
                if diagnostics.unsupported_samples.len() < 10 {
                    diagnostics
                        .unsupported_samples
                        .push(path.to_string_lossy().into_owned());
                }
            }
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

#[expect(
    clippy::too_many_lines,
    reason = "cross-file analysis coordinates duplication, coverage, diagnostics, and cleanup as one bounded phase"
)]
pub(super) fn analyze_cross_file_metrics(
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
            prepared.deadline,
            &mut file_analysis.diagnostics,
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
                lang::detect(&file.report.path)
                    .is_some_and(|info| prepared.health_policy.includes(&file.report.path, info))
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
            dup::DetectionThresholds::new(
                cfg.min_dup_tokens,
                cfg.min_dup_lines,
                cfg.near_dup_min_similarity,
            ),
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
    let test_regions = file_analysis
        .analyzed
        .iter()
        .filter(|analysis| !analysis.test_regions.is_empty())
        .map(|analysis| (analysis.report.path.clone(), analysis.test_regions.clone()))
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
        test_regions,
        symbol_outlines,
        graph_facts,
        duplication,
        duplicate_coverage,
        diagnostics: file_analysis.diagnostics,
        encoding_name: file_analysis.encoding_name,
        cache_stats,
    }
}

pub(super) fn apply_type2_diagnostics(
    diagnostics: &mut ScanDiagnostics,
    type2: dup::fuzzy::Type2Diagnostics,
) {
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

pub(super) fn analyze_planning_universe(
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
        discovered,
        cfg,
        &prepared.health_policy,
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
            prepared.deadline,
            &mut analysis.diagnostics,
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
        &BTreeMap::new(),
        cfg,
        &prepared.health_policy,
        false,
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

pub(super) fn attach_churn(
    root: &Path,
    analyzed: &mut [AnalyzedFile],
    cfg: &Config,
    progress: &ScanProgress,
    stage: &str,
    deadline: Option<Instant>,
    diagnostics: &mut ScanDiagnostics,
) {
    if !cfg.enabled.churn {
        return;
    }
    progress.stage(stage);
    let paths = analyzed
        .iter()
        .map(|file| file.report.path.clone())
        .collect::<Vec<_>>();
    let collection = git::collect_with_diagnostics(
        root,
        &paths,
        &git::ChurnLimits {
            max_commits: cfg.churn_max_commits,
            max_deltas_per_commit: cfg.max_churn_deltas_per_commit,
            max_total_deltas: cfg.max_churn_total_deltas,
            max_output_bytes: cfg.max_churn_output_bytes,
            max_path_bytes: cfg.max_git_path_bytes,
            max_cache_bytes: cfg.max_churn_cache_bytes,
            deadline,
            skip_libgit2_fallback: cfg.execution_profile == "safe",
        },
        cfg.use_cache,
    );
    for file in analyzed {
        if let Some(value) = collection.churn.get(&file.report.path) {
            file.report.churn = Some(value.clone());
        }
    }
    if collection.partial {
        diagnostics.churn_analysis_partial = true;
        diagnostics.churn_deltas_omitted = diagnostics
            .churn_deltas_omitted
            .saturating_add(collection.deltas_omitted);
        diagnostics.scan_truncated = true;
    }
}

pub(super) fn analyze_file(
    path: &Path,
    report_path: &Path,
    cfg: &Config,
    health_policy: &HealthPolicy,
    counter: Option<&TokenCounter>,
    cache: &Cache,
    requirements: ArtifactRequirements,
) -> AnalysisOutcome {
    if lang::detect(path).is_none() {
        return AnalysisOutcome::Unsupported(report_path.to_path_buf());
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
                &cached.test_regions,
                cached.symbol_outlines.as_deref(),
                cached.graph_facts.as_ref(),
            );
        }
        return AnalysisOutcome::Analyzed(Box::new(AnalyzedFile {
            report: cached.report,
            content,
            test_regions: cached.test_regions,
            symbol_outlines: cached.symbol_outlines,
            graph_facts: cached.graph_facts,
        }));
    }

    let Some(analysis) = analyze_source_details(
        report_path,
        &content,
        cfg,
        health_policy,
        counter,
        requirements,
    ) else {
        return AnalysisOutcome::Unsupported(report_path.to_path_buf());
    };

    cache.put(
        &rel_str,
        hash,
        &analysis.report,
        &analysis.test_regions,
        analysis.symbol_outlines.as_deref(),
        analysis.graph_facts.as_ref(),
    );
    AnalysisOutcome::Analyzed(Box::new(AnalyzedFile {
        report: analysis.report,
        content,
        test_regions: analysis.test_regions,
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
    health_policy: &HealthPolicy,
    counter: Option<&TokenCounter>,
) -> Option<FileReport> {
    analyze_source_details(
        report_path,
        content,
        cfg,
        health_policy,
        counter,
        ArtifactRequirements::default(),
    )
    .map(|analysis| analysis.report)
}

pub(super) fn analyze_source_details(
    report_path: &Path,
    content: &str,
    cfg: &Config,
    health_policy: &HealthPolicy,
    counter: Option<&TokenCounter>,
    requirements: ArtifactRequirements,
) -> Option<SourceAnalysis> {
    let info = lang::detect(report_path)?;
    let health_eligible = health_policy.includes(report_path, info);
    let rel = report_path.to_path_buf();
    let rel_str = rel.to_string_lossy().to_string();
    let tokens = counter.map_or(0, |c| c.count(content));

    // First-class line metrics also consume the syntax tree, so parse even
    // when structural analyzers are disabled.
    let tree = info.first_class.and_then(|fc| parse::parse(fc, content));
    let line_stats = lines::measure(info, content, tree.as_ref());
    let marker_scan = marker_facts(info, content, tree.as_ref(), cfg, health_eligible);
    let (complexity_opt, approximate) = complexity_facts(
        info,
        content,
        tree.as_ref(),
        &line_stats,
        cfg,
        health_eligible,
    );
    let import_list = import_facts(info, content, tree.as_ref(), cfg);
    let (sym, symbol_outlines) = symbol_facts(info, content, tree.as_ref(), cfg, requirements);
    let graph_facts = graph_facts(info, &rel_str, content, tree.as_ref(), requirements);
    let skip = classify::skip_hint(&rel_str, content);
    let test_facts = rust_test_facts(info, content, tree.as_ref());
    let comment_ratio = percentage_ratio(line_stats.comment_lines, line_stats.loc);

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
            has_inline_tests: test_facts.has_inline_tests,
        },
        test_regions: test_facts.regions,
        symbol_outlines,
        graph_facts,
    })
}

fn marker_facts(
    info: &lang::LangInfo,
    content: &str,
    tree: Option<&Tree>,
    cfg: &Config,
    health_eligible: bool,
) -> markers::MarkerScan {
    if !cfg.enabled.markers || !health_eligible {
        return markers::MarkerScan::default();
    }
    match (info.first_class, tree) {
        (Some(_), Some(tree)) => markers::scan_detailed_in_tree(content, &cfg.markers, tree),
        _ => markers::scan_detailed(content, &cfg.markers),
    }
}

fn complexity_facts(
    info: &lang::LangInfo,
    content: &str,
    tree: Option<&Tree>,
    line_stats: &lines::LineStats,
    cfg: &Config,
    health_eligible: bool,
) -> (Option<Complexity>, bool) {
    if !cfg.enabled.complexity || !health_eligible || !info.is_code() {
        return (None, false);
    }
    let (complexity, approximate) = complexity::analyze(info, content, tree, line_stats);
    (Some(complexity), approximate)
}

fn import_facts(
    info: &lang::LangInfo,
    content: &str,
    tree: Option<&Tree>,
    cfg: &Config,
) -> Vec<String> {
    if !cfg.enabled.imports {
        return Vec::new();
    }
    match (info.first_class, tree) {
        (Some(language), Some(tree)) => imports::extract(language, content, tree),
        _ => Vec::new(),
    }
}

fn symbol_facts(
    info: &lang::LangInfo,
    content: &str,
    tree: Option<&Tree>,
    cfg: &Config,
    requirements: ArtifactRequirements,
) -> (Option<SymbolCounts>, Option<Vec<SymbolOutline>>) {
    let needs_counts = cfg.enabled.complexity || cfg.enabled.imports;
    match (info.first_class, tree) {
        (Some(language), Some(tree)) if requirements.symbol_outlines => {
            let analysis = symbols::analyze(language, content, tree);
            (
                needs_counts.then_some(analysis.counts),
                Some(analysis.outlines),
            )
        }
        (Some(language), Some(tree)) if needs_counts => {
            (Some(symbols::count(language, content, tree)), None)
        }
        (_, _) if requirements.symbol_outlines => (None, Some(Vec::new())),
        _ => (None, None),
    }
}

fn graph_facts(
    info: &lang::LangInfo,
    path: &str,
    content: &str,
    tree: Option<&Tree>,
    requirements: ArtifactRequirements,
) -> Option<crate::graph::SourceFacts> {
    if !requirements.graph_facts {
        return None;
    }
    match (info.first_class, tree) {
        (Some(language), Some(tree)) => Some(crate::graph::extract_source_facts_from_tree(
            language,
            path,
            content,
            tree.root_node(),
        )),
        (Some(_), None) => Some(crate::graph::SourceFacts::parse_error()),
        (None, _) => None,
    }
}

struct RustTestFacts {
    has_inline_tests: bool,
    regions: Vec<LineRange>,
}

fn rust_test_facts(info: &lang::LangInfo, content: &str, tree: Option<&Tree>) -> RustTestFacts {
    if info.first_class != Some(lang::FirstClass::Rust) {
        return RustTestFacts {
            has_inline_tests: false,
            regions: Vec::new(),
        };
    }
    let fallback_tree = tree
        .is_none()
        .then(|| parse::parse(lang::FirstClass::Rust, content))
        .flatten();
    let tree = tree.or(fallback_tree.as_ref());
    RustTestFacts {
        has_inline_tests: tree.is_some_and(|tree| testcov::has_inline_rust_tests(content, tree)),
        regions: tree.map_or_else(Vec::new, |tree| {
            testcov::inline_rust_test_regions(content, tree)
        }),
    }
}

fn percentage_ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        usize_to_f64(numerator) / usize_to_f64(denominator)
    }
}

pub(super) fn build_file_duplication_coverage(
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

pub(super) fn percentage(part: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        usize_to_f64(part) / usize_to_f64(total) * 100.0
    }
}
