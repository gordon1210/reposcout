use super::{
    Assessment, BTreeMap, Config, DEFAULT_CONTEXT_BUDGET, DuplicateCoverage, Duplication,
    DuplicationSummary, FileRef, FileReport, FunctionHotspot, HashSet, HealthPolicy, Hotspot,
    LanguageDuplication, LanguageStat, LineRange, PathBuf, ProductionDuplication, RiskEntry,
    ScanDiagnostics, SkipCandidate, Summary, TestPresence, instance_has_production_lines, lang,
    percentage, ranked_duplicate_candidates, risk, testcov, top_duplicate_blocks_where,
    top_duplicate_findings, u64_to_f64, usize_to_f64,
};

pub(super) struct AggregateAccum {
    languages: BTreeMap<String, LanguageStat>,
    cyclomatic_total: u64,
    cognitive_total: u64,
    function_count: usize,
    mi_sum: f64,
    mi_count: usize,
    mi_min: f64,
    function_hotspots: Vec<FunctionHotspot>,
}

#[expect(
    clippy::too_many_arguments,
    reason = "aggregation keeps each shared scan input explicit"
)]
pub(super) fn aggregate(
    files: &[FileReport],
    dup: &Duplication,
    duplicate_coverage: &DuplicateCoverage,
    test_regions: &BTreeMap<PathBuf, Vec<LineRange>>,
    cfg: &Config,
    health_policy: &HealthPolicy,
    duplication_complete: bool,
    frameworks: Vec<crate::model::TestFramework>,
) -> (Summary, Vec<RiskEntry>) {
    let mut s = Summary::default();
    let accumulated = accumulate_file_metrics(files, &mut s);
    finish_complexity_and_languages(&mut s, accumulated, cfg);
    summarize_duplication(
        &mut s,
        dup,
        duplicate_coverage,
        test_regions,
        cfg,
        health_policy,
    );
    populate_file_rankings(&mut s, files, cfg);
    let (test_presence, top_risks, all_risk_entries) =
        test_and_risk_summary(files, cfg, health_policy, frameworks);
    s.test_presence = test_presence;
    s.top_risks = top_risks;
    let production_duplication = cfg.enabled.duplication.then(|| {
        production_duplication(
            files,
            duplicate_coverage,
            test_regions,
            health_policy,
            duplication_complete,
        )
    });
    s.assessment = build_assessment(&s, production_duplication, cfg.enabled);
    (s, all_risk_entries)
}

pub(super) fn accumulate_file_metrics(
    files: &[FileReport],
    summary: &mut Summary,
) -> AggregateAccum {
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
        e.name.clone_from(&f.language);
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
                accumulated.cyclomatic_total += u64::from(func.cyclomatic);
                accumulated.cognitive_total += u64::from(func.cognitive);
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

pub(super) fn finish_complexity_and_languages(
    summary: &mut Summary,
    mut accumulated: AggregateAccum,
    cfg: &Config,
) {
    summary.comment_ratio = if summary.loc > 0 {
        usize_to_f64(summary.comment_lines) / usize_to_f64(summary.loc)
    } else {
        0.0
    };

    summary.complexity.cyclomatic_total = accumulated.cyclomatic_total;
    summary.complexity.cognitive_total = accumulated.cognitive_total;
    summary.complexity.functions = accumulated.function_count;
    summary.complexity.cyclomatic_threshold = cfg.max_complexity;
    if accumulated.function_count > 0 {
        summary.complexity.cyclomatic_avg =
            u64_to_f64(accumulated.cyclomatic_total) / usize_to_f64(accumulated.function_count);
        summary.complexity.cognitive_avg =
            u64_to_f64(accumulated.cognitive_total) / usize_to_f64(accumulated.function_count);
    }
    if accumulated.mi_count > 0 {
        summary.complexity.mi_avg = accumulated.mi_sum / usize_to_f64(accumulated.mi_count);
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

pub(super) fn summarize_duplication(
    summary: &mut Summary,
    dup: &Duplication,
    duplicate_coverage: &DuplicateCoverage,
    test_regions: &BTreeMap<PathBuf, Vec<LineRange>>,
    cfg: &Config,
    health_policy: &HealthPolicy,
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
    let duplicate_candidates = ranked_duplicate_candidates(dup);
    summary.top_duplicates =
        top_duplicate_blocks_where(&duplicate_candidates, cfg.top, cfg.min_dup_lines, |_| true);
    summary.top_production_duplicates =
        top_duplicate_blocks_where(&duplicate_candidates, cfg.top, cfg.min_dup_lines, |group| {
            group.instances.iter().any(|instance| {
                instance_has_production_lines(
                    instance,
                    test_regions,
                    health_policy,
                    cfg.min_dup_lines,
                )
            })
        });
    summary.top_duplicate_findings = top_duplicate_findings(dup, cfg.top);
}

pub(super) fn populate_file_rankings(summary: &mut Summary, files: &[FileReport], cfg: &Config) {
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
            let commits = f.churn.as_ref().map_or(0, |c| c.commits);
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
                score: usize_to_f64(commits) * (f64::from(cyclomatic) + 1.0),
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

pub(super) fn test_and_risk_summary(
    files: &[FileReport],
    cfg: &Config,
    health_policy: &HealthPolicy,
    frameworks: Vec<crate::model::TestFramework>,
) -> (Option<TestPresence>, Vec<RiskEntry>, Vec<RiskEntry>) {
    let mut test_file_count = 0usize;
    let mut risk_entries = Vec::new();

    for file in files {
        if !lang::detect(&file.path)
            .is_some_and(|info| info.is_code() && health_policy.includes(&file.path, info))
        {
            continue;
        }
        let is_configured_test = !frameworks.is_empty()
            && testcov::is_framework_test_file(&frameworks, file.path.to_string_lossy().as_ref());
        if is_configured_test {
            test_file_count += 1;
        } else if let Some(entry) = risk::entry(file) {
            risk_entries.push(entry);
        }
    }

    risk_entries.sort_by(compare_risk_entries);
    let all_risk_entries = risk_entries.clone();
    risk_entries.truncate(cfg.top);
    let test_presence = (!frameworks.is_empty()).then_some(TestPresence {
        frameworks,
        test_files: test_file_count,
    });
    (test_presence, risk_entries, all_risk_entries)
}

pub(super) fn compare_risk_entries(left: &RiskEntry, right: &RiskEntry) -> std::cmp::Ordering {
    right
        .score
        .total_cmp(&left.score)
        .then_with(|| right.sloc.cmp(&left.sloc))
        .then_with(|| right.cyclomatic.cmp(&left.cyclomatic))
        .then_with(|| right.churn_commits.cmp(&left.churn_commits))
        .then_with(|| left.path.cmp(&right.path))
}

pub(super) fn production_duplication(
    files: &[FileReport],
    coverage: &DuplicateCoverage,
    test_regions: &BTreeMap<PathBuf, Vec<LineRange>>,
    health_policy: &HealthPolicy,
    complete: bool,
) -> ProductionDuplication {
    let source_files = files.iter().filter(|file| {
        lang::detect(&file.path)
            .is_some_and(|info| info.is_code() && health_policy.includes(&file.path, info))
            && !testcov::is_test_file(file.path.to_string_lossy().as_ref())
    });
    let (duplicated_lines, lines) = source_files.fold((0usize, 0usize), |totals, file| {
        let regions = test_regions
            .get(&file.path)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let test_lines = regions
            .iter()
            .map(|range| {
                range
                    .end
                    .min(file.loc)
                    .saturating_sub(range.start.saturating_sub(1))
            })
            .sum::<usize>()
            .min(file.loc);
        (
            totals.0 + coverage.covered_lines_excluding(&file.path, regions),
            totals.1 + file.loc.saturating_sub(test_lines),
        )
    });
    ProductionDuplication {
        corpus: "production-source".to_string(),
        duplicated_lines,
        analyzed_lines: lines,
        duplicated_pct: percentage(duplicated_lines, lines),
        complete,
    }
}

pub(super) fn production_duplication_is_complete(
    duplication_enabled: bool,
    diagnostics: &ScanDiagnostics,
) -> bool {
    duplication_enabled
        && !diagnostics.type2_analysis_partial
        && diagnostics.unreadable_files == 0
        && diagnostics.walker_errors == 0
        && diagnostics.oversized_files == 0
        && diagnostics.files_omitted_by_limit == 0
        && !diagnostics.files_omitted_count_incomplete
        && diagnostics.bytes_omitted_by_limit == 0
        && !diagnostics.duration_limit_reached
}

pub(super) fn build_assessment(
    summary: &Summary,
    production_duplication: Option<ProductionDuplication>,
    enabled: crate::config::Enabled,
) -> Assessment {
    let token_budget = DEFAULT_CONTEXT_BUDGET;
    let fits_context_known = enabled.tokens;
    let readable_source_tokens = summary.source.tokens;
    let fits_context = fits_context_known && readable_source_tokens <= token_budget;
    let cleanup_worth_complete =
        cleanup_evidence_is_complete(enabled, production_duplication.as_ref());
    let unavailable_signals = unavailable_signals(enabled);
    let cleanup_reasons = cleanup_reasons(summary, production_duplication.as_ref());
    let signal_count = cleanup_reasons.len();
    let cleanup_worth = if signal_count >= 3 {
        "high"
    } else if signal_count >= 1 {
        "medium"
    } else {
        "low"
    }
    .to_string();

    let mut assessment_reasons = vec![context_fit_reason(
        fits_context_known,
        fits_context,
        token_budget,
        readable_source_tokens,
    )];
    assessment_reasons.extend(cleanup_reasons);

    Assessment {
        fits_context_known,
        fits_context,
        token_budget,
        cleanup_worth,
        cleanup_worth_complete,
        unavailable_signals,
        production_duplication,
        reasons: assessment_reasons,
    }
}

fn cleanup_evidence_is_complete(
    enabled: crate::config::Enabled,
    production_duplication: Option<&ProductionDuplication>,
) -> bool {
    enabled.complexity
        && enabled.duplication
        && enabled.churn
        && production_duplication.is_some_and(|duplication| duplication.complete)
}

fn unavailable_signals(enabled: crate::config::Enabled) -> Vec<String> {
    [
        (enabled.tokens, "tokens"),
        (enabled.complexity, "complexity"),
        (enabled.duplication, "duplication"),
        (enabled.churn, "churn"),
    ]
    .into_iter()
    .filter(|(available, _)| !*available)
    .map(|(_, name)| name.to_string())
    .collect()
}

fn cleanup_reasons(
    summary: &Summary,
    production_duplication: Option<&ProductionDuplication>,
) -> Vec<String> {
    let mut reasons = Vec::new();
    if let Some(reason) = duplication_cleanup_reason(production_duplication) {
        reasons.push(reason);
    }
    if let Some(reason) = maintainability_cleanup_reason(summary) {
        reasons.push(reason);
    }
    if summary
        .top_risks
        .iter()
        .filter(|risk| risk.score >= 0.7)
        .count()
        >= 3
    {
        reasons.push("several high-risk files".to_string());
    }
    reasons
}

fn duplication_cleanup_reason(duplication: Option<&ProductionDuplication>) -> Option<String> {
    let duplication = duplication.filter(|value| value.duplicated_pct > 15.0)?;
    Some(if duplication.complete {
        format!(
            "high source duplication ({:.1}%)",
            duplication.duplicated_pct
        )
    } else {
        format!(
            "high observed source duplication ({:.1}%; partial evidence)",
            duplication.duplicated_pct
        )
    })
}

fn maintainability_cleanup_reason(summary: &Summary) -> Option<String> {
    let complexity = &summary.complexity;
    let available =
        complexity.functions > 0 || complexity.approximate_files > 0 || complexity.mi_avg > 0.0;
    (available && complexity.mi_avg < 20.0).then(|| {
        let band = if complexity.mi_avg < 10.0 {
            "low"
        } else {
            "moderate"
        };
        format!("{band} maintainability (MI avg {:.0})", complexity.mi_avg)
    })
}

fn context_fit_reason(
    known: bool,
    fits: bool,
    token_budget: usize,
    source_tokens: usize,
) -> String {
    if !known {
        "context fit unavailable (tokens analyzer disabled)".to_string()
    } else if fits {
        format!("fits in {token_budget}-token context ({source_tokens} source tokens)")
    } else {
        format!("exceeds {token_budget}-token context budget ({source_tokens} source tokens)")
    }
}
