//! Markdown report — good for pasting into PRs, issues, or agent context.

use crate::model::{ScanDiagnostics, ScanReport};
use crate::numeric::usize_to_f64;
use crate::report::projection::{
    file_cyclomatic_average, finding_location, human_duplicate_projection, human_risk_heading,
    human_test_signal, metric_delta_display, metric_label, source_language_rollup,
};
use crate::report::{
    ConfigGuidance, RenderOptions, config_guidance, dup_locations, human_bytes, markdown_code_span,
    markdown_table_code_span, markdown_table_text, markdown_text, similarity_label, terminal_text,
    thousands, thousands_u64,
};
use std::fmt::Write as _;
use std::path::Path;

#[must_use]
pub fn render(report: &ScanReport, options: RenderOptions) -> String {
    let mut out = String::new();

    let _ = writeln!(
        out,
        "# reposcout — {}",
        markdown_code_span(&report.target.display().to_string())
    );
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "_encoding: {} · generated: {}_",
        markdown_text(&report.encoding),
        markdown_text(&report.generated_at)
    );
    let _ = writeln!(out);

    render_overview(&mut out, report);
    if let Some(work_scope) = &report.work_scope {
        super::work_scope::markdown(&mut out, work_scope);
    }

    render_complexity(&mut out, report);

    render_duplication(&mut out, report, options.duplication_details);

    render_markers_symbols_and_skips(&mut out, report);
    render_languages_and_token_files(&mut out, report);
    render_hotspots(&mut out, report);
    render_assessment_and_tests(&mut out, report);
    render_top_risks(&mut out, report);
    render_context(&mut out, report);
    render_directories(&mut out, report);
    render_baseline(&mut out, report);
    render_review(&mut out, report);
    render_graph(&mut out, report);
    render_impact(&mut out, report);
    render_config_guidance(&mut out, report, options.suppress_config_guidance);

    out
}

fn render_config_guidance(out: &mut String, report: &ScanReport, suppressed: bool) {
    let Some(guidance) = config_guidance(report, suppressed) else {
        return;
    };

    let introduction = match guidance {
        ConfigGuidance::NoConfiguration => concat!(
            "No RepoScout configuration was found. A global config can establish reusable ",
            "defaults across repositories."
        ),
        ConfigGuidance::GlobalOnly => {
            "The global RepoScout configuration is active, but no project config was found."
        }
    };
    let _ = writeln!(
        out,
        "> **Configuration tip:** {introduction} A project `reposcout.toml` can further improve \
         signal quality by tailoring exclusions, health scope, and analysis settings to this \
         repository. Inspect effective settings with `reposcout config .`."
    );
    let _ = writeln!(out);
}

fn render_markers_symbols_and_skips(out: &mut String, report: &ScanReport) {
    let s = &report.summary;

    if !s.markers.is_empty() {
        let markers: Vec<String> = s
            .markers
            .iter()
            .map(|(k, v)| format!("{}: {v}", markdown_text(k)))
            .collect();
        let _ = writeln!(out, "## Markers");
        let _ = writeln!(out);
        let _ = writeln!(out, "{}", markers.join(", "));
        let _ = writeln!(out);
    }

    let sym = &s.symbols;
    if sym.functions > 0 || sym.types > 0 || sym.exports > 0 {
        let _ = writeln!(out, "## Symbols");
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "functions {}, types {}, exports {}",
            thousands(sym.functions),
            thousands(sym.types),
            thousands(sym.exports),
        );
        let _ = writeln!(out);
    }

    if !s.skip_candidates.is_empty() {
        let _ = writeln!(
            out,
            "## Skip candidates (generated/minified/bundled/vendored)"
        );
        let _ = writeln!(out);
        let _ = writeln!(out, "| Path | Reason | Tokens |");
        let _ = writeln!(out, "|---|---|--:|");
        for c in &s.skip_candidates {
            let _ = writeln!(
                out,
                "| {} | {} | {} |",
                markdown_table_code_span(&c.path),
                markdown_table_text(&c.reason),
                thousands(c.tokens)
            );
        }
        let _ = writeln!(out);
    }
}

fn render_languages_and_token_files(out: &mut String, report: &ScanReport) {
    let s = &report.summary;

    if !s.languages.is_empty() {
        let _ = writeln!(out, "## Source languages");
        let _ = writeln!(out);
        let _ = writeln!(out, "| Language | Files | SLOC | Comment% | Tokens |");
        let _ = writeln!(out, "|---|--:|--:|--:|--:|");
        for l in source_language_rollup(&s.languages) {
            let cpct = if l.loc > 0 {
                usize_to_f64(l.comment_lines) / usize_to_f64(l.loc) * 100.0
            } else {
                0.0
            };
            let _ = writeln!(
                out,
                "| {} | {} | {} | {:.0}% | {} |",
                markdown_table_text(&l.name),
                thousands(l.files),
                thousands(l.sloc),
                cpct,
                thousands(l.tokens)
            );
        }
        let _ = writeln!(out);
    }

    if !s.top_source_token_files.is_empty() {
        let _ = writeln!(out, "## Top source files by tokens");
        let _ = writeln!(out);
        let _ = writeln!(out, "| File | Tokens |");
        let _ = writeln!(out, "|---|--:|");
        for f in &s.top_source_token_files {
            let _ = writeln!(
                out,
                "| {} | {} |",
                markdown_table_code_span(&f.path.display().to_string()),
                thousands(f.tokens)
            );
        }
        let _ = writeln!(out);
    }
}

fn render_hotspots(out: &mut String, report: &ScanReport) {
    let s = &report.summary;

    if !s.top_hotspots.is_empty() {
        let _ = writeln!(out, "## Hotspots (churn × complexity)");
        let _ = writeln!(out);
        let _ = writeln!(out, "| File | Commits | Cyclomatic | Avg/fn | Score |");
        let _ = writeln!(out, "|---|--:|--:|--:|--:|");
        for h in &s.top_hotspots {
            let average = file_cyclomatic_average(report, &h.path)
                .map_or_else(|| "-".to_string(), |average| format!("{average:.1}"));
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} | {:.0} |",
                markdown_table_code_span(&h.path.display().to_string()),
                h.commits,
                h.cyclomatic,
                average,
                h.score
            );
        }
        let _ = writeln!(out);
    }
}

fn render_assessment_and_tests(out: &mut String, report: &ScanReport) {
    let s = &report.summary;

    let a = &s.assessment;
    let _ = writeln!(out, "## Assessment");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "- Context: **{}** (budget {})",
        if !a.fits_context_known {
            "unknown"
        } else if a.fits_context {
            "fits"
        } else {
            "exceeds"
        },
        thousands(a.token_budget)
    );
    let _ = writeln!(
        out,
        "- Cleanup worth: **{}**{}",
        markdown_text(&a.cleanup_worth),
        if a.cleanup_worth_complete {
            ""
        } else {
            " (partial evidence)"
        }
    );
    if !a.unavailable_signals.is_empty() {
        let _ = writeln!(
            out,
            "- Unavailable signals: {}",
            markdown_text(&a.unavailable_signals.join(", "))
        );
    }
    for reason in &a.reasons {
        let _ = writeln!(out, "  - {}", markdown_text(reason));
    }
    let _ = writeln!(out);

    let Some(tp) = &s.test_presence else {
        return;
    };
    let _ = writeln!(out, "## Configured test discovery");
    let _ = writeln!(out);
    let frameworks = tp
        .frameworks
        .iter()
        .map(|framework| {
            format!(
                "{} ({})",
                framework.name,
                markdown_code_span(&framework.evidence)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let _ = writeln!(out, "- Frameworks: {frameworks}");
    let _ = writeln!(out, "- Test files: **{}**", thousands(tp.test_files));
    let _ = writeln!(out);
}

fn render_top_risks(out: &mut String, report: &ScanReport) {
    let s = &report.summary;
    if !s.top_risks.is_empty() {
        let _ = writeln!(out, "## {}", human_risk_heading(report));
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "| Path | Score | SLOC | Cyclomatic | Avg/fn | Churn | Reasons |"
        );
        let _ = writeln!(out, "|---|--:|--:|--:|--:|--:|---|");
        for r in &s.top_risks {
            let average = file_cyclomatic_average(report, Path::new(&r.path))
                .map_or_else(|| "-".to_string(), |average| format!("{average:.1}"));
            let _ = writeln!(
                out,
                "| {} | {:.2} | {} | {} | {} | {} | {} |",
                markdown_table_code_span(&r.path),
                r.score,
                thousands(r.sloc),
                r.cyclomatic,
                average,
                r.churn_commits,
                markdown_table_text(
                    &r.reasons
                        .iter()
                        .map(|reason| human_test_signal(reason))
                        .collect::<Vec<_>>()
                        .join(", "),
                ),
            );
        }
        let _ = writeln!(out);
    }
}

fn render_directories(out: &mut String, report: &ScanReport) {
    if !report.directories.is_empty() {
        let _ = writeln!(out, "## By directory");
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "| Path | Files | Tokens | SLOC | Cyclo avg | MI avg | Dup lines | No filename match |"
        );
        let _ = writeln!(out, "|---|--:|--:|--:|--:|--:|--:|--:|");
        for d in &report.directories {
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} | {:.1} | {:.0} | {} | {} |",
                markdown_table_code_span(&d.path),
                thousands(d.files),
                thousands(d.tokens),
                thousands(d.sloc),
                d.cyclomatic_avg,
                d.mi_avg,
                thousands(d.duplicated_lines),
                thousands(d.untested_source_files),
            );
        }
        let _ = writeln!(out);
    }
}

fn render_baseline(out: &mut String, report: &ScanReport) {
    let Some(baseline) = &report.baseline else {
        return;
    };

    let _ = writeln!(out, "## Baseline comparison");
    let _ = writeln!(out);
    let _ = writeln!(out, "| Metric | Baseline | Current | Delta |");
    let _ = writeln!(out, "|---|--:|--:|--:|");
    for metric in &baseline.metrics {
        let display = metric_delta_display(metric);
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} |",
            markdown_table_text(metric_label(&metric.metric)),
            display.baseline,
            display.current,
            display.delta
        );
    }
    let _ = writeln!(out);
    let regressions = if baseline.regressions.is_empty() {
        "none".to_string()
    } else {
        baseline
            .regressions
            .iter()
            .map(|regression| human_test_signal(regression))
            .collect::<Vec<_>>()
            .join("; ")
    };
    let _ = writeln!(out, "Regressions: {}", markdown_text(&regressions));
    if baseline.finding_changes.comparison == "complete" {
        let counts = &baseline.finding_changes.counts;
        let _ = writeln!(
            out,
            "Finding changes: **{} new**, **{} worsened**, **{} improved**, **{} resolved**.",
            thousands(counts.new),
            thousands(counts.worsened),
            thousands(counts.improved),
            thousands(counts.resolved)
        );
    } else if let Some(reason) = &baseline.finding_changes.reason {
        let _ = writeln!(
            out,
            "Finding changes unavailable: {}.",
            markdown_text(reason)
        );
    }
    let _ = writeln!(out);
}

#[expect(
    clippy::too_many_lines,
    reason = "context rendering follows the serialized plan fields in a fixed human-readable order"
)]
fn render_context(out: &mut String, report: &ScanReport) {
    let Some(context) = &report.context else {
        return;
    };
    let _ = writeln!(out, "## Agent context plan");
    let _ = writeln!(out);
    let _ = writeln!(out, "- Planning time: **{} ms**", context.planning_ms);
    if !context.graph_languages.is_empty() {
        let _ = writeln!(
            out,
            "- Graph signals: {} ({} unresolved imports, {} parse errors, {} config errors)",
            context
                .graph_languages
                .iter()
                .map(|language| markdown_text(language))
                .collect::<Vec<_>>()
                .join(", "),
            context.graph_unresolved_imports,
            context.graph_parse_errors,
            context.graph_config_errors
        );
    }
    let _ = writeln!(out);
    if !context.files.is_empty() {
        let _ = writeln!(out, "| File | Tokens | Score | Why | ");
        let _ = writeln!(out, "|---|--:|--:|---|");
        for file in &context.files {
            let _ = writeln!(
                out,
                "| {} | {} | {:.2} | {} |",
                markdown_table_code_span(&file.path.display().to_string()),
                thousands(file.tokens),
                file.score,
                markdown_table_text(&file.reasons.join(", "))
            );
        }
        let _ = writeln!(out);
    }
    if !context.outline_only.is_empty() {
        let _ = writeln!(out, "### Outline-only focus");
        let _ = writeln!(out);
        for file in &context.outline_only {
            let _ = writeln!(
                out,
                "- {} — {} source tokens omitted: {}",
                markdown_code_span(&file.path.display().to_string()),
                thousands(file.source_tokens),
                markdown_text(&file.reason)
            );
        }
        let _ = writeln!(out);
    }
    if context.files.iter().any(|file| !file.symbols.is_empty()) {
        let _ = writeln!(out, "### Selected symbol outlines");
        let _ = writeln!(out);
        let _ = writeln!(out, "| File | Line | Kind | Signature | Why |");
        let _ = writeln!(out, "|---|--:|---|---|---|");
        for file in &context.files {
            for symbol in &file.symbols {
                let _ = writeln!(
                    out,
                    "| {} | {} | {} | {} | {} |",
                    markdown_table_code_span(&file.path.display().to_string()),
                    symbol.line,
                    markdown_table_text(&symbol.kind),
                    markdown_table_code_span(&symbol.signature),
                    markdown_table_text(&symbol.reasons.join(", "))
                );
            }
        }
        let _ = writeln!(out);
    }
    if context
        .outline_only
        .iter()
        .any(|file| !file.symbols.is_empty())
    {
        let _ = writeln!(out, "### Outline-only declarations");
        let _ = writeln!(out);
        let _ = writeln!(out, "| File | Line | Kind | Signature | Why |");
        let _ = writeln!(out, "|---|--:|---|---|---|");
        for file in &context.outline_only {
            for symbol in &file.symbols {
                let _ = writeln!(
                    out,
                    "| {} | {} | {} | {} | {} |",
                    markdown_table_code_span(&file.path.display().to_string()),
                    symbol.line,
                    markdown_table_text(&symbol.kind),
                    markdown_table_code_span(&symbol.signature),
                    markdown_table_text(&symbol.reasons.join(", "))
                );
            }
        }
        let _ = writeln!(out);
    }
    if !context.omitted.is_empty() {
        let _ = writeln!(out, "### Context omissions");
        let _ = writeln!(out);
        for file in &context.omitted {
            let _ = writeln!(
                out,
                "- {} — {} tokens: {}",
                markdown_code_span(&file.path.display().to_string()),
                thousands(file.tokens),
                markdown_text(&file.reason)
            );
        }
        let _ = writeln!(out);
    }
}

fn render_overview(out: &mut String, report: &ScanReport) {
    let summary = &report.summary;
    let _ = writeln!(out, "## Overview");
    let _ = writeln!(out);
    let _ = writeln!(out, "| Metric | Value |");
    let _ = writeln!(out, "|---|--:|");
    let _ = writeln!(
        out,
        "| Files | {} total · {} source |",
        thousands(summary.files),
        thousands(summary.source.files)
    );
    let _ = writeln!(out, "| Size | {} |", human_bytes(summary.bytes));
    let _ = writeln!(
        out,
        "| Tokens | {} total · {} source |",
        thousands(summary.tokens),
        thousands(summary.source.tokens)
    );
    let _ = writeln!(out, "| Lines (LOC) | {} |", thousands(summary.loc));
    let _ = writeln!(
        out,
        "| Source (SLOC) | {} |",
        thousands(summary.source.sloc)
    );
    let source_comment_ratio = if summary.source.loc > 0 {
        usize_to_f64(summary.source.comment_lines) / usize_to_f64(summary.source.loc)
    } else {
        0.0
    };
    let _ = writeln!(
        out,
        "| Source comments | {} ({:.1}%) |",
        thousands(summary.source.comment_lines),
        source_comment_ratio * 100.0
    );
    if summary.line_metrics_approximate_files > 0 {
        let _ = writeln!(
            out,
            "| Approx. line metrics | {} files |",
            thousands(summary.line_metrics_approximate_files)
        );
    }
    let _ = writeln!(out);

    let diagnostics = &report.diagnostics;
    render_scan_diagnostics(out, diagnostics);
}

#[expect(
    clippy::too_many_lines,
    reason = "diagnostic rendering exhaustively presents every bounded-scan signal in one ordered section"
)]
fn render_scan_diagnostics(out: &mut String, diagnostics: &ScanDiagnostics) {
    if diagnostics.unsupported_files == 0
        && diagnostics.unreadable_files == 0
        && diagnostics.walker_errors == 0
        && !diagnostics.scan_truncated
        && !diagnostics.type2_analysis_partial
    {
        return;
    }

    let _ = writeln!(out, "## Scan diagnostics");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "- Discovered: **{}**, analyzed: **{}**",
        thousands(diagnostics.discovered_files),
        thousands(diagnostics.analyzed_files)
    );
    if diagnostics.unsupported_files > 0 {
        let _ = writeln!(
            out,
            "- Unsupported files: **{}**",
            thousands(diagnostics.unsupported_files)
        );
        if !diagnostics.unsupported_samples.is_empty() {
            let _ = writeln!(
                out,
                "- Unsupported examples: {}.",
                diagnostics
                    .unsupported_samples
                    .iter()
                    .map(|path| format!("`{}`", markdown_text(path)))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }
    if diagnostics.unreadable_files > 0 {
        let _ = writeln!(
            out,
            "- Unreadable files: **{}**",
            thousands(diagnostics.unreadable_files)
        );
    }
    if diagnostics.walker_errors > 0 {
        let _ = writeln!(
            out,
            "- Walker errors: **{}**",
            thousands(diagnostics.walker_errors)
        );
    }
    if diagnostics.oversized_files > 0 {
        let _ = writeln!(
            out,
            "- Oversized files skipped: **{}** (**{}**).",
            thousands(diagnostics.oversized_files),
            human_bytes(diagnostics.oversized_bytes)
        );
    }
    if diagnostics.files_omitted_by_limit > 0 {
        let count = if diagnostics.files_omitted_count_incomplete {
            format!(
                "at least {} (traversal stopped before an exact count)",
                thousands(diagnostics.files_omitted_by_limit)
            )
        } else {
            thousands(diagnostics.files_omitted_by_limit)
        };
        let _ = writeln!(
            out,
            "- Known files omitted by resource limits: **{count}** (known size **{}**).",
            human_bytes(diagnostics.bytes_omitted_by_limit)
        );
    }
    if diagnostics.duration_limit_reached {
        let _ = writeln!(out, "- The cooperative scan duration limit was reached.");
    }
    if diagnostics.scan_truncated {
        let _ = writeln!(
            out,
            "- Scan results are **partial** because an input or runtime limit was reached."
        );
    }
    if diagnostics.type2_analysis_partial {
        let _ = writeln!(
            out,
            "- Type-2 analysis is **partial** because a safety limit was reached."
        );
        let _ = writeln!(
            out,
            "- Type-2 work omitted: **{} candidate seed pairs** across **{} format pools**.",
            thousands_u64(diagnostics.type2_seed_pairs_skipped),
            thousands(diagnostics.type2_pools_truncated)
        );
        if diagnostics.type2_candidate_buckets_skipped > 0
            || diagnostics.type2_candidate_buckets_partially_selected > 0
        {
            let _ = writeln!(
                out,
                "- Type-2 fingerprint buckets: **{} skipped**, **{} partially searched**.",
                thousands(diagnostics.type2_candidate_buckets_skipped),
                thousands(diagnostics.type2_candidate_buckets_partially_selected)
            );
        }
        if diagnostics.type2_match_limit_reached {
            let _ = writeln!(out, "- The Type-2 match buffer limit was reached.");
        }
        if diagnostics.type2_suppression_limit_reached {
            let _ = writeln!(
                out,
                "- The Type-2 overlap work limit was reached; **{} buffered matches** were omitted.",
                thousands(diagnostics.type2_matches_skipped_during_suppression)
            );
        }
    }
    let _ = writeln!(out);
}

fn render_complexity(out: &mut String, report: &ScanReport) {
    let summary = &report.summary;
    let complexity = &summary.complexity;
    if complexity.cyclomatic_total > 0 || complexity.mi_avg > 0.0 {
        let _ = writeln!(out, "## Function complexity");
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "- Rule: cyclomatic complexity must be **≤ {} per function**.",
            complexity.cyclomatic_threshold
        );
        let _ = writeln!(
            out,
            "- Functions: **{} analyzed**, **{} over the limit**.",
            thousands(complexity.functions),
            thousands(complexity.functions_over_threshold)
        );
        let _ = writeln!(out);
        let _ = writeln!(out, "| Scope / metric | Avg | Max/Min |");
        let _ = writeln!(out, "|---|--:|--:|");
        let _ = writeln!(
            out,
            "| Function cyclomatic | {:.1} | {} |",
            complexity.cyclomatic_avg, complexity.cyclomatic_max
        );
        let _ = writeln!(
            out,
            "| Function cognitive | {:.1} | {} |",
            complexity.cognitive_avg, complexity.cognitive_max
        );
        let _ = writeln!(
            out,
            "| File maintainability | {:.1} | {:.1} |",
            complexity.mi_avg, complexity.mi_min
        );
        let _ = writeln!(out);
    }

    if !summary.complexity_violations.is_empty() {
        let _ = writeln!(out, "## Complexity violations");
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "| Function | Location | Cyclomatic | Max | Over | Cognitive | Nesting |"
        );
        let _ = writeln!(out, "|---|---|--:|--:|--:|--:|--:|");
        for function in &summary.complexity_violations {
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} | +{} | {} | {} |",
                markdown_table_code_span(&function.name),
                markdown_table_code_span(&format!("{}:{}", function.path.display(), function.line)),
                function.cyclomatic,
                complexity.cyclomatic_threshold,
                function
                    .cyclomatic
                    .saturating_sub(complexity.cyclomatic_threshold),
                function.cognitive,
                function.max_nesting
            );
        }
        let _ = writeln!(out);
    } else if !summary.top_functions.is_empty() {
        let _ = writeln!(out, "## Most complex functions (all within limit)");
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "| Function | Location | Cyclomatic | Cognitive | Nesting |"
        );
        let _ = writeln!(out, "|---|---|--:|--:|--:|");
        for function in &summary.top_functions {
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} | {} |",
                markdown_table_code_span(&function.name),
                markdown_table_code_span(&format!("{}:{}", function.path.display(), function.line)),
                function.cyclomatic,
                function.cognitive,
                function.max_nesting
            );
        }
        let _ = writeln!(out);
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "duplication rendering keeps summary, coverage, truncation, and optional detail ordering together as one output contract"
)]
fn render_duplication(out: &mut String, report: &ScanReport, duplication_details: bool) {
    let summary = &report.summary;
    let duplication = &summary.duplication;
    let _ = writeln!(out, "## Duplication");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "- Exact groups: **{}**, near groups: **{}**",
        duplication.exact_groups, duplication.near_groups
    );
    let _ = writeln!(
        out,
        "- Line coverage: **{:.1}%** ({} / {} analyzed lines)",
        duplication.duplicated_pct,
        thousands(duplication.duplicated_lines),
        thousands(duplication.analyzed_lines)
    );
    let _ = writeln!(
        out,
        "- Token coverage: **{:.1}%** ({} / {} duplication-lexer tokens)",
        duplication.duplicated_tokens_pct,
        thousands(duplication.duplicated_tokens),
        thousands(duplication.analyzed_tokens)
    );
    let _ = writeln!(out);

    let (title, duplicates) = human_duplicate_projection(summary);
    if !duplicates.is_empty() {
        let _ = writeln!(out, "## {title}");
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "| Lines | Copies | Similarity | Removable | Locations |"
        );
        let _ = writeln!(out, "|--:|--:|---|--:|---|");
        for duplicate in duplicates {
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} | {} |",
                thousands(duplicate.lines),
                thousands(duplicate.copies),
                similarity_label(duplicate.similarity),
                thousands(duplicate.duplicated_lines),
                markdown_table_text(&dup_locations(&duplicate.locations, duplicate.copies)),
            );
        }
        let _ = writeln!(out);
    }

    if duplication_details && !report.duplicates.findings.is_empty() {
        let _ = writeln!(out, "## Duplicate findings");
        let _ = writeln!(out);
        for finding in &report.duplicates.findings {
            let _ = writeln!(
                out,
                "### {} · {} · {} · {} tokens · {}",
                markdown_code_span(&finding.id),
                markdown_text(&finding.kind),
                markdown_text(&finding.format),
                finding.tokens,
                similarity_label(finding.similarity)
            );
            let _ = writeln!(out);
            let _ = writeln!(
                out,
                "- A: {}",
                markdown_code_span(&format!(
                    "{}:{}:{}-{}:{}",
                    finding.fragment_a.path.display(),
                    finding.fragment_a.start_line,
                    finding.fragment_a.start_column,
                    finding.fragment_a.end_line,
                    finding.fragment_a.end_column
                ))
            );
            let _ = writeln!(
                out,
                "- B: {}",
                markdown_code_span(&format!(
                    "{}:{}:{}-{}:{}",
                    finding.fragment_b.path.display(),
                    finding.fragment_b.start_line,
                    finding.fragment_b.start_column,
                    finding.fragment_b.end_line,
                    finding.fragment_b.end_column
                ))
            );
            if let Some(snippet) = &finding.fragment_a.snippet {
                let _ = writeln!(out);
                let _ = writeln!(out, "Fragment A:");
                let _ = writeln!(out);
                for line in snippet.lines() {
                    let _ = writeln!(out, "    {}", terminal_text(line));
                }
            }
            if let Some(snippet) = &finding.fragment_b.snippet {
                let _ = writeln!(out);
                let _ = writeln!(out, "Fragment B:");
                let _ = writeln!(out);
                for line in snippet.lines() {
                    let _ = writeln!(out, "    {}", terminal_text(line));
                }
            }
            let _ = writeln!(out);
        }
    }

    let languages = super::projection::human_duplication_languages(&duplication.by_language);
    if !languages.is_empty() {
        let _ = writeln!(out, "## Duplication by language");
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "| Language | Groups E/N | Line coverage | Token coverage |"
        );
        let _ = writeln!(out, "|---|--:|--:|--:|");
        for language in languages {
            let _ = writeln!(
                out,
                "| {} | {}/{} | {:.1}% ({}) | {:.1}% ({}) |",
                markdown_table_text(&language.name),
                language.exact_groups,
                language.near_groups,
                language.duplicated_lines_pct,
                thousands(language.duplicated_lines),
                language.duplicated_tokens_pct,
                thousands(language.duplicated_tokens)
            );
        }
        let _ = writeln!(out);
    }
}

fn render_review(out: &mut String, report: &ScanReport) {
    let Some(review) = &report.review else {
        return;
    };

    let _ = writeln!(out, "## Changed-line review");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "Mode: **{}**; scope: **{}**; changed files: **{}**.",
        markdown_text(&review.mode),
        markdown_text(&review.scope),
        thousands(review.changed_files.len())
    );
    if review.mode == "deep" {
        let _ = writeln!(
            out,
            "Findings: **{} new**, **{} worsened**, **{} improved**, **{} resolved**.",
            thousands(review.counts.new),
            thousands(review.counts.worsened),
            thousands(review.counts.improved),
            thousands(review.counts.resolved)
        );
    } else {
        let _ = writeln!(
            out,
            "Findings on changed lines: **{}**.",
            thousands(review.counts.current)
        );
    }
    if review.diagnostics.binary_files > 0 || review.diagnostics.unreadable_files > 0 {
        let _ = writeln!(
            out,
            "Skipped: **{} binary**, **{} unreadable**.",
            thousands(review.diagnostics.binary_files),
            thousands(review.diagnostics.unreadable_files)
        );
    }
    if review.diagnostics.oversized_files > 0 {
        let _ = writeln!(
            out,
            "Oversized snapshot files skipped: **{}** (**{}**).",
            thousands(review.diagnostics.oversized_files),
            human_bytes(review.diagnostics.oversized_bytes)
        );
    }
    if review.diagnostics.files_omitted_by_limit > 0 {
        let count = if review.diagnostics.files_omitted_count_incomplete {
            format!(
                "at least {} (traversal stopped before an exact count)",
                thousands(review.diagnostics.files_omitted_by_limit)
            )
        } else {
            thousands(review.diagnostics.files_omitted_by_limit)
        };
        let _ = writeln!(
            out,
            "Known snapshot files omitted by resource limits: **{count}**."
        );
    }
    if review.diagnostics.duration_limit_reached {
        let _ = writeln!(out, "The cooperative review duration limit was reached.");
    }
    let _ = writeln!(out);

    if !review.findings.is_empty() {
        let _ = writeln!(out, "| State | Kind | Severity | Location | Finding |");
        let _ = writeln!(out, "|---|---|---|---|---|");
        for item in &review.findings {
            let finding = item.after.as_ref().unwrap_or(&item.finding);
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} | {} |",
                markdown_table_text(&item.state),
                markdown_table_text(&finding.kind),
                markdown_table_text(&finding.severity),
                markdown_table_code_span(&finding_location(finding)),
                markdown_table_text(&finding.message)
            );
        }
        let _ = writeln!(out);
    }
}

fn render_graph(out: &mut String, report: &ScanReport) {
    let Some(graph) = &report.graph else {
        return;
    };
    render_graph_summary(out, graph);
    render_graph_metadata(out, graph);
    render_focused_graph_files(out, graph);
    render_graph_cycles(out, graph);
    render_top_depended(out, graph);
    render_most_dependent(out, graph);
    render_graph_orphans(out, graph);
}

fn render_graph_summary(out: &mut String, graph: &crate::model::DepGraph) {
    let _ = writeln!(out, "## Dependency graph (heuristic first-class languages)");
    let _ = writeln!(out);
    let languages = if graph.languages.is_empty() {
        "none".to_string()
    } else {
        graph
            .languages
            .iter()
            .map(|language| markdown_text(language))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let _ = writeln!(out, "_Languages: {languages}_");
    let _ = writeln!(out);
    let _ = writeln!(out, "| Metric | Value |");
    let _ = writeln!(out, "|---|--:|");
    let _ = writeln!(out, "| Nodes | {} |", thousands(graph.nodes));
    let _ = writeln!(out, "| Edges | {} |", thousands(graph.edges));
    let _ = writeln!(out, "| Cycles | {} |", thousands(graph.cycles.len()));
    let _ = writeln!(out, "| Orphans | {} |", thousands(graph.orphans.len()));
    let _ = writeln!(
        out,
        "| Unresolved imports | {} |",
        thousands(graph.unresolved_imports)
    );
    let _ = writeln!(out, "| Parse errors | {} |", thousands(graph.parse_errors));
    let _ = writeln!(
        out,
        "| Config errors | {} |",
        thousands(graph.config_errors)
    );
    let _ = writeln!(out);
}

fn render_graph_metadata(out: &mut String, graph: &crate::model::DepGraph) {
    if !graph.config_files.is_empty() {
        let _ = writeln!(
            out,
            "- Resolver configs: {}",
            graph
                .config_files
                .iter()
                .map(|path| markdown_code_span(path))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if !graph.focus.is_empty() {
        let _ = writeln!(
            out,
            "- Focus: {}; traversal: **{}**, depth **{}**",
            graph
                .focus
                .iter()
                .map(|path| markdown_code_span(path))
                .collect::<Vec<_>>()
                .join(", "),
            markdown_text(&graph.direction),
            graph.depth.unwrap_or_default()
        );
    }
    if !graph.unmatched_focus.is_empty() {
        let _ = writeln!(
            out,
            "- Unmatched focus: {}",
            graph
                .unmatched_focus
                .iter()
                .map(|path| markdown_code_span(path))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if !graph.config_files.is_empty()
        || !graph.focus.is_empty()
        || !graph.unmatched_focus.is_empty()
    {
        let _ = writeln!(out);
    }
}

fn render_focused_graph_files(out: &mut String, graph: &crate::model::DepGraph) {
    if !graph.focus.is_empty() && !graph.files.is_empty() {
        let _ = writeln!(out, "### Focused graph files");
        let _ = writeln!(out);
        let _ = writeln!(out, "| Path | Distance | Fan-in | Fan-out |");
        let _ = writeln!(out, "|---|--:|--:|--:|");
        for file in &graph.files {
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} |",
                markdown_table_code_span(&file.path),
                file.focus_distance.unwrap_or_default(),
                file.fan_in,
                file.fan_out
            );
        }
        let _ = writeln!(out);
    }
}

fn render_graph_cycles(out: &mut String, graph: &crate::model::DepGraph) {
    if !graph.cycles.is_empty() {
        let _ = writeln!(out, "### Import cycles");
        let _ = writeln!(out);
        for cycle in graph.cycles.iter().take(5) {
            let _ = writeln!(out, "- {}", markdown_code_span(&cycle.join(" -> ")));
        }
        if graph.cycles.len() > 5 {
            let _ = writeln!(out, "- … +{} more", graph.cycles.len() - 5);
        }
        let _ = writeln!(out);
    }
}

fn render_top_depended(out: &mut String, graph: &crate::model::DepGraph) {
    if !graph.top_depended.is_empty() {
        let _ = writeln!(out, "### Most depended-upon");
        let _ = writeln!(out);
        let _ = writeln!(out, "| Path | Fan-in |");
        let _ = writeln!(out, "|---|--:|");
        for node in &graph.top_depended {
            let _ = writeln!(
                out,
                "| {} | {} |",
                markdown_table_code_span(&node.path),
                thousands(node.fan_in)
            );
        }
        let _ = writeln!(out);
    }
}

fn render_most_dependent(out: &mut String, graph: &crate::model::DepGraph) {
    if !graph.most_dependent.is_empty() {
        let _ = writeln!(out, "### Most dependent");
        let _ = writeln!(out);
        let _ = writeln!(out, "| Path | Fan-out |");
        let _ = writeln!(out, "|---|--:|");
        for node in &graph.most_dependent {
            let _ = writeln!(
                out,
                "| {} | {} |",
                markdown_table_code_span(&node.path),
                thousands(node.fan_out)
            );
        }
        let _ = writeln!(out);
    }
}

fn render_graph_orphans(out: &mut String, graph: &crate::model::DepGraph) {
    if !graph.orphans.is_empty() {
        let _ = writeln!(out, "### Orphans (dead-code candidates)");
        let _ = writeln!(out);
        for orphan in graph.orphans.iter().take(15) {
            let _ = writeln!(out, "- {}", markdown_code_span(orphan));
        }
        if graph.orphans.len() > 15 {
            let _ = writeln!(out, "- … +{} more", graph.orphans.len() - 15);
        }
        let _ = writeln!(out);
    }
}

fn render_impact(out: &mut String, report: &ScanReport) {
    let Some(impact) = &report.impact else {
        return;
    };

    let _ = writeln!(out, "## Change impact (first-class languages)");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "- Confidence: **{}**; changed files: **{}**; graph-covered: **{}**",
        markdown_text(&impact.confidence),
        thousands(impact.changed_files.len()),
        thousands(impact.graph_changed_files.len())
    );
    let _ = writeln!(
        out,
        "- Direct dependents: **{}**; transitive dependents: **{}**; unresolved imports: **{}**; parse errors: **{}**; config errors: **{}**",
        thousands(impact.direct_dependents.len()),
        thousands(impact.transitive_dependents.len()),
        thousands(impact.unresolved_imports),
        thousands(impact.parse_errors),
        thousands(impact.config_errors)
    );
    if !impact.direct_dependents.is_empty() {
        let _ = writeln!(out, "### Direct dependents");
        let _ = writeln!(out);
        for path in impact.direct_dependents.iter().take(10) {
            let _ = writeln!(out, "- {}", markdown_code_span(path));
        }
        let _ = writeln!(out);
    }
    if !impact.transitive_dependents.is_empty() {
        let _ = writeln!(out, "### Transitive dependents");
        let _ = writeln!(out);
        for path in impact.transitive_dependents.iter().take(10) {
            let _ = writeln!(out, "- {}", markdown_code_span(path));
        }
        let _ = writeln!(out);
    }
}

#[cfg(test)]
mod tests;
