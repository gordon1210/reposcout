//! Human-facing terminal report: aligned key/value sections plus tables for
//! language breakdown, token-heavy files, and churn hotspots.

use crate::model::{ScanDiagnostics, ScanReport};
use crate::numeric::usize_to_f64;
use crate::report::projection::{
    file_cyclomatic_average, finding_location, human_duplicate_projection,
    human_duplication_languages, human_risk_heading, human_test_signal, metric_delta_display,
    metric_label, source_language_rollup,
};
use crate::report::{
    ConfigGuidance, RenderOptions, config_guidance, dup_locations, human_bytes, similarity_label,
    terminal_text, thousands, thousands_u64,
};
use comfy_table::presets::UTF8_BORDERS_ONLY;
use comfy_table::{
    Cell, CellAlignment, Color, ColumnConstraint, ContentArrangement, Row, Table, Width,
};
use owo_colors::OwoColorize;
use std::fmt::Write as _;
use std::path::Path;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

#[derive(Clone, Copy)]
enum Tone {
    Neutral,
    Accent,
    Positive,
    Caution,
    Negative,
    Info,
}

#[must_use]
pub fn render(report: &ScanReport, color: bool, options: RenderOptions) -> String {
    let mut out = String::new();
    let target = terminal_text(&report.target.display().to_string());
    let encoding = terminal_text(&report.encoding);
    let title_suffix = format!("   (encoding: {encoding})");
    let title_path_width = output_width().map_or_else(
        || target.chars().count(),
        |width| width.saturating_sub("reposcout  ".len() + title_suffix.len()),
    );
    let title = format!("reposcout  {}", path_cell(&target, title_path_width));
    let _ = writeln!(out, "{}{}", header(&title, color), title_suffix);
    let _ = writeln!(out);

    render_languages(&mut out, report, color);
    render_top_source_files(&mut out, report, color);
    render_symbols(&mut out, report, color);
    if let Some(work_scope) = &report.work_scope {
        super::work_scope::table(&mut out, work_scope, color);
    }
    render_skip_candidates(&mut out, report, color);
    render_directories(&mut out, report, color);
    render_graph(&mut out, report, color);
    render_markers(&mut out, report, color);
    render_test_presence(&mut out, report, color);
    render_duplication(&mut out, report, color, options.duplication_details);
    render_complexity(&mut out, report, color);
    render_hotspots(&mut out, report, color);
    render_top_risks(&mut out, report, color);
    render_scan_diagnostics(&mut out, &report.diagnostics, color);
    render_assessment(&mut out, report, color);
    render_baseline(&mut out, report, color);
    render_review(&mut out, report, color);
    render_impact(&mut out, report, color);
    render_context(&mut out, report, color);
    render_overview(&mut out, report, color);
    render_config_guidance(&mut out, report, color, options.suppress_config_guidance);
    out
}

fn render_config_guidance(out: &mut String, report: &ScanReport, color: bool, suppressed: bool) {
    let Some(guidance) = config_guidance(report, suppressed) else {
        return;
    };

    let message = match guidance {
        ConfigGuidance::NoConfiguration => {
            "Tip: no config found — global/project settings can sharpen this report."
        }
        ConfigGuidance::GlobalOnly => {
            "Tip: global config active — a project config can sharpen this report further."
        }
    };
    let _ = writeln!(out, "  {}\n", toned_value(message, color, Tone::Info));
}

fn render_markers(out: &mut String, report: &ScanReport, color: bool) {
    if report.summary.markers.is_empty() {
        return;
    }
    let mut markers = report.summary.markers.iter().collect::<Vec<_>>();
    markers.sort_by_key(|(marker, _)| marker_priority(marker));
    let markers = markers
        .into_iter()
        .map(|(marker, count)| {
            toned_value(
                &format!("{} {count}", terminal_text(marker)),
                color,
                marker_tone(marker),
            )
        })
        .collect::<Vec<_>>();
    let _ = writeln!(out, "{}  {}", header("Markers", color), markers.join(" · "));
    let _ = writeln!(out);
}

fn render_symbols(out: &mut String, report: &ScanReport, color: bool) {
    let symbols = &report.summary.symbols;
    if symbols.functions == 0 && symbols.types == 0 && symbols.exports == 0 {
        return;
    }
    let _ = writeln!(
        out,
        "{}  functions {}, types {}, exports {}",
        header("Symbols", color),
        thousands(symbols.functions),
        thousands(symbols.types),
        thousands(symbols.exports),
    );
    let _ = writeln!(out);
}

fn render_skip_candidates(out: &mut String, report: &ScanReport, color: bool) {
    if report.summary.skip_candidates.is_empty() {
        return;
    }
    let _ = writeln!(
        out,
        "{}",
        toned_header(
            "Skip candidates (generated/minified/bundled/vendored)",
            color,
            Tone::Info,
        )
    );
    let mut table = new_table(vec!["Path", "Reason", "Tokens"]);
    let rows = report
        .summary
        .skip_candidates
        .iter()
        .map(|candidate| {
            vec![
                terminal_text(&candidate.path),
                terminal_text(&candidate.reason),
                thousands(candidate.tokens),
            ]
        })
        .collect();
    add_responsive_path_rows(&mut table, rows, 0, &[(0, 2), (1, 1)]);
    right_align(&mut table, &[2]);
    let _ = writeln!(out, "{table}\n");
}

fn render_languages(out: &mut String, report: &ScanReport, color: bool) {
    if report.summary.languages.is_empty() {
        return;
    }
    let _ = writeln!(out, "{}", header("Source languages", color));
    let mut table = new_table(vec!["Language", "Files", "SLOC", "Comment%", "Tokens"]);
    for language in source_language_rollup(&report.summary.languages) {
        let comment_percentage = if language.loc > 0 {
            usize_to_f64(language.comment_lines) / usize_to_f64(language.loc) * 100.0
        } else {
            0.0
        };
        table.add_row(vec![
            terminal_text(&language.name),
            thousands(language.files),
            thousands(language.sloc),
            format!("{comment_percentage:.0}%"),
            thousands(language.tokens),
        ]);
    }
    fit_responsive_table(&mut table, 0, &[(0, 1)]);
    right_align(&mut table, &[1, 2, 3, 4]);
    let _ = writeln!(out, "{table}\n");
}

fn render_top_source_files(out: &mut String, report: &ScanReport, color: bool) {
    if report.summary.top_source_token_files.is_empty() {
        return;
    }
    let _ = writeln!(out, "{}", header("Top source files by tokens", color));
    let mut table = new_table(vec!["File", "Tokens"]);
    let rows = report
        .summary
        .top_source_token_files
        .iter()
        .map(|file| {
            vec![
                terminal_text(&file.path.display().to_string()),
                thousands(file.tokens),
            ]
        })
        .collect();
    add_responsive_path_rows(&mut table, rows, 0, &[(0, 1)]);
    right_align(&mut table, &[1]);
    let _ = writeln!(out, "{table}\n");
}

fn render_hotspots(out: &mut String, report: &ScanReport, color: bool) {
    if report.summary.top_hotspots.is_empty() {
        return;
    }
    let _ = writeln!(out, "{}", header("Hotspots (churn × complexity)", color));
    let mut table = new_table(vec!["File", "Commits", "Cyclo", "Avg/fn", "Score"]);
    let rows = report
        .summary
        .top_hotspots
        .iter()
        .map(|hotspot| {
            vec![
                Cell::new(terminal_text(&hotspot.path.display().to_string())),
                Cell::new(thousands(hotspot.commits)),
                Cell::new(thousands(hotspot.cyclomatic as usize)),
                Cell::new(
                    file_cyclomatic_average(report, &hotspot.path)
                        .map_or_else(|| "-".to_string(), |average| format!("{average:.1}")),
                ),
                toned_cell(format!("{:.0}", hotspot.score), color, Tone::Caution),
            ]
        })
        .collect();
    add_responsive_path_cell_rows(&mut table, rows, 0, &[(0, 1)]);
    right_align(&mut table, &[1, 2, 3, 4]);
    let _ = writeln!(out, "{table}\n");
}

fn render_assessment(out: &mut String, report: &ScanReport, color: bool) {
    let assessment = &report.summary.assessment;
    let _ = writeln!(out, "{}", header("Assessment", color));
    let (context, context_tone) = if assessment.fits_context_known {
        (
            format!(
                "{} (budget {})",
                if assessment.fits_context {
                    "fits"
                } else {
                    "exceeds"
                },
                thousands(assessment.token_budget)
            ),
            if assessment.fits_context {
                Tone::Positive
            } else {
                Tone::Caution
            },
        )
    } else {
        ("unknown (tokens unavailable)".to_string(), Tone::Caution)
    };
    toned_kv(out, "Context", &context, color, context_tone);
    let cleanup = if assessment.cleanup_worth_complete {
        assessment.cleanup_worth.clone()
    } else {
        format!("{} (partial)", assessment.cleanup_worth)
    };
    let cleanup_tone = if assessment.cleanup_worth_complete {
        match assessment.cleanup_worth.as_str() {
            "low" => Tone::Positive,
            "medium" => Tone::Caution,
            "high" => Tone::Negative,
            _ => Tone::Neutral,
        }
    } else {
        Tone::Caution
    };
    toned_kv(out, "Cleanup worth", &cleanup, color, cleanup_tone);
    if !assessment.unavailable_signals.is_empty() {
        toned_kv(
            out,
            "Unavailable",
            &assessment.unavailable_signals.join(", "),
            color,
            Tone::Caution,
        );
    }
    if !assessment.reasons.is_empty() {
        kv(out, "Reasons", &assessment.reasons.join("; "));
    }
    let _ = writeln!(out);
}

fn render_test_presence(out: &mut String, report: &ScanReport, color: bool) {
    let tests = &report.summary.test_presence;
    let _ = writeln!(out, "{}", header("Test filename matching", color));
    kv(out, "Test files", &thousands(tests.test_files));
    kv(out, "Source files", &thousands(tests.source_files));
    toned_kv(
        out,
        "No filename match",
        &thousands(tests.untested_source_files),
        color,
        if tests.untested_source_files == 0 {
            Tone::Positive
        } else {
            Tone::Caution
        },
    );
    if !tests.untested_samples.is_empty() {
        kv(out, "Samples", &tests.untested_samples.join(", "));
    }
    let _ = writeln!(out);
}

fn render_top_risks(out: &mut String, report: &ScanReport, color: bool) {
    if report.summary.top_risks.is_empty() {
        return;
    }
    let _ = writeln!(out, "{}", header(&human_risk_heading(report), color));
    let mut table = new_table(vec![
        "Path", "Score", "SLOC", "Cyclo", "Avg/fn", "Churn", "Reasons",
    ]);
    let rows = report
        .summary
        .top_risks
        .iter()
        .map(|risk| {
            vec![
                Cell::new(terminal_text(&risk.path)),
                toned_cell(format!("{:.2}", risk.score), color, risk_tone(risk.score)),
                Cell::new(thousands(risk.sloc)),
                Cell::new(thousands(risk.cyclomatic as usize)),
                Cell::new(
                    file_cyclomatic_average(report, Path::new(&risk.path))
                        .map_or_else(|| "-".to_string(), |average| format!("{average:.1}")),
                ),
                Cell::new(thousands(risk.churn_commits)),
                Cell::new(terminal_text(
                    &risk
                        .reasons
                        .iter()
                        .map(|reason| human_test_signal(reason))
                        .collect::<Vec<_>>()
                        .join(", "),
                )),
            ]
        })
        .collect();
    add_responsive_path_cell_rows(&mut table, rows, 0, &[(0, 1), (6, 1)]);
    right_align(&mut table, &[1, 2, 3, 4, 5]);
    let _ = writeln!(out, "{table}\n");
}

fn render_directories(out: &mut String, report: &ScanReport, color: bool) {
    if report.directories.is_empty() {
        return;
    }
    let _ = writeln!(out, "{}", header("By directory", color));
    let mut table = new_table(vec![
        "Path",
        "Files",
        "Tokens",
        "SLOC",
        "Cyclo avg",
        "MI avg",
        "Dup lines",
        "No filename match",
    ]);
    let rows = report
        .directories
        .iter()
        .map(|directory| {
            vec![
                terminal_text(&directory.path),
                thousands(directory.files),
                thousands(directory.tokens),
                thousands(directory.sloc),
                format!("{:.1}", directory.cyclomatic_avg),
                format!("{:.0}", directory.mi_avg),
                thousands(directory.duplicated_lines),
                thousands(directory.untested_source_files),
            ]
        })
        .collect();
    add_responsive_path_rows(&mut table, rows, 0, &[(0, 1)]);
    right_align(&mut table, &[1, 2, 3, 4, 5, 6, 7]);
    let _ = writeln!(out, "{table}\n");
}

fn header(text: &str, color: bool) -> String {
    toned_header(text, color, Tone::Accent)
}

fn toned_header(text: &str, color: bool, tone: Tone) -> String {
    let text = terminal_text(text);
    if !color {
        return text;
    }
    match tone {
        Tone::Neutral => format!("{}", text.bold()),
        Tone::Accent => format!("{}", text.cyan().bold()),
        Tone::Positive => format!("{}", text.green().bold()),
        Tone::Caution => format!("{}", text.yellow().bold()),
        Tone::Negative => format!("{}", text.red().bold()),
        Tone::Info => format!("{}", text.blue().bold()),
    }
}

fn render_baseline(out: &mut String, report: &ScanReport, color: bool) {
    let Some(baseline) = &report.baseline else {
        return;
    };

    let _ = writeln!(out, "{}", header("Baseline comparison", color));
    let mut table = new_table(vec!["Metric", "Baseline", "Current", "Delta"]);
    for metric in &baseline.metrics {
        let display = metric_delta_display(metric);
        table.add_row(vec![
            terminal_text(metric_label(&metric.metric)),
            display.baseline,
            display.current,
            display.delta,
        ]);
    }
    fit_responsive_table(&mut table, 0, &[(0, 1)]);
    right_align(&mut table, &[1, 2, 3]);
    let _ = writeln!(out, "{table}");
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
    toned_kv(
        out,
        "Regressions",
        &regressions,
        color,
        if baseline.regressions.is_empty() {
            Tone::Positive
        } else {
            Tone::Negative
        },
    );
    if baseline.finding_changes.comparison == "complete" {
        let counts = &baseline.finding_changes.counts;
        kv(
            out,
            "Finding changes",
            &format!(
                "{} new · {} worsened · {} improved · {} resolved",
                thousands(counts.new),
                thousands(counts.worsened),
                thousands(counts.improved),
                thousands(counts.resolved)
            ),
        );
    } else if let Some(reason) = &baseline.finding_changes.reason {
        kv(out, "Finding changes", &format!("unavailable: {reason}"));
    }
    let _ = writeln!(out);
}

#[expect(
    clippy::too_many_lines,
    reason = "context table rendering follows the serialized plan fields in a fixed human-readable order"
)]
fn render_context(out: &mut String, report: &ScanReport, color: bool) {
    let Some(context) = &report.context else {
        return;
    };
    let _ = writeln!(out, "{}", header("Agent context plan", color));
    kv(out, "Planning time", &format!("{} ms", context.planning_ms));
    if !context.graph_languages.is_empty() {
        kv(
            out,
            "Graph signals",
            &format!(
                "{} · {} unresolved · {} parse errors · {} config errors",
                context.graph_languages.join(", "),
                context.graph_unresolved_imports,
                context.graph_parse_errors,
                context.graph_config_errors
            ),
        );
    }
    if !context.files.is_empty() {
        let mut table = new_table(vec!["File", "Tokens", "Score", "Why"]);
        let rows = context
            .files
            .iter()
            .map(|file| {
                vec![
                    terminal_text(&file.path.display().to_string()),
                    thousands(file.tokens),
                    format!("{:.2}", file.score),
                    terminal_text(&file.reasons.join(", ")),
                ]
            })
            .collect();
        add_responsive_path_rows(&mut table, rows, 0, &[(0, 2), (3, 3)]);
        right_align(&mut table, &[1, 2]);
        let _ = writeln!(out, "{table}");
    }
    if !context.outline_only.is_empty() {
        let _ = writeln!(out, "{}", header("Outline-only focus", color));
        let mut table = new_table(vec!["File", "Source tokens", "Reason"]);
        let rows = context
            .outline_only
            .iter()
            .map(|file| {
                vec![
                    terminal_text(&file.path.display().to_string()),
                    thousands(file.source_tokens),
                    terminal_text(&file.reason),
                ]
            })
            .collect();
        add_responsive_path_rows(&mut table, rows, 0, &[(0, 2), (2, 1)]);
        right_align(&mut table, &[1]);
        let _ = writeln!(out, "{table}");
    }
    if context.files.iter().any(|file| !file.symbols.is_empty()) {
        let _ = writeln!(out, "{}", header("Selected symbol outlines", color));
        let mut table = new_table(vec!["File", "Line", "Kind", "Signature", "Why"]);
        let mut rows = Vec::new();
        for file in &context.files {
            for symbol in &file.symbols {
                rows.push(vec![
                    terminal_text(&file.path.display().to_string()),
                    thousands(symbol.line),
                    terminal_text(&symbol.kind),
                    terminal_text(&symbol.signature),
                    terminal_text(&symbol.reasons.join(", ")),
                ]);
            }
        }
        add_responsive_path_rows(&mut table, rows, 0, &[(0, 1), (3, 2), (4, 1)]);
        right_align(&mut table, &[1]);
        let _ = writeln!(out, "{table}");
    }
    if context
        .outline_only
        .iter()
        .any(|file| !file.symbols.is_empty())
    {
        let _ = writeln!(out, "{}", header("Outline-only declarations", color));
        let mut table = new_table(vec!["File", "Line", "Kind", "Signature", "Why"]);
        let mut rows = Vec::new();
        for file in &context.outline_only {
            for symbol in &file.symbols {
                rows.push(vec![
                    terminal_text(&file.path.display().to_string()),
                    thousands(symbol.line),
                    terminal_text(&symbol.kind),
                    terminal_text(&symbol.signature),
                    terminal_text(&symbol.reasons.join(", ")),
                ]);
            }
        }
        add_responsive_path_rows(&mut table, rows, 0, &[(0, 1), (3, 2), (4, 1)]);
        right_align(&mut table, &[1]);
        let _ = writeln!(out, "{table}");
    }
    if !context.omitted.is_empty() {
        let notes = context
            .omitted
            .iter()
            .map(|file| {
                format!(
                    "{} ({} tokens: {})",
                    file.path.display(),
                    thousands(file.tokens),
                    file.reason
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        kv(out, "Omission notes", &notes);
    }
    let _ = writeln!(out);
}

fn render_overview(out: &mut String, report: &ScanReport, color: bool) {
    let summary = &report.summary;
    let _ = writeln!(out, "{}", header("Overview", color));
    kv(
        out,
        "Files",
        &format!(
            "{} total · {} source",
            thousands(summary.files),
            thousands(summary.source.files)
        ),
    );
    kv(out, "Size", &human_bytes(summary.bytes));
    kv(
        out,
        "Tokens",
        &format!(
            "{} total · {} source",
            thousands(summary.tokens),
            thousands(summary.source.tokens)
        ),
    );
    kv(out, "Lines (LOC)", &thousands(summary.loc));
    kv(out, "Source (SLOC)", &thousands(summary.source.sloc));
    let source_comment_ratio = if summary.source.loc > 0 {
        usize_to_f64(summary.source.comment_lines) / usize_to_f64(summary.source.loc)
    } else {
        0.0
    };
    kv(
        out,
        "Source comments",
        &format!(
            "{} ({:.1}%)",
            thousands(summary.source.comment_lines),
            source_comment_ratio * 100.0
        ),
    );
    if summary.line_metrics_approximate_files > 0 {
        kv(
            out,
            "Approx. line metrics",
            &format!(
                "{} files",
                thousands(summary.line_metrics_approximate_files)
            ),
        );
    }
    let _ = writeln!(out);
}

fn render_scan_diagnostics(out: &mut String, diagnostics: &ScanDiagnostics, color: bool) {
    if diagnostics.unsupported_files == 0
        && diagnostics.unreadable_files == 0
        && diagnostics.walker_errors == 0
        && !diagnostics.scan_truncated
        && !diagnostics.type2_analysis_partial
    {
        return;
    }

    let _ = writeln!(
        out,
        "{}",
        toned_header("Scan diagnostics", color, Tone::Caution)
    );
    kv(out, "Discovered", &thousands(diagnostics.discovered_files));
    kv(out, "Analyzed", &thousands(diagnostics.analyzed_files));
    if diagnostics.unsupported_files > 0 {
        toned_kv(
            out,
            "Unsupported",
            &thousands(diagnostics.unsupported_files),
            color,
            Tone::Caution,
        );
        if !diagnostics.unsupported_samples.is_empty() {
            kv(out, "Examples", &diagnostics.unsupported_samples.join(", "));
        }
    }
    if diagnostics.unreadable_files > 0 {
        toned_kv(
            out,
            "Unreadable",
            &thousands(diagnostics.unreadable_files),
            color,
            Tone::Negative,
        );
    }
    if diagnostics.walker_errors > 0 {
        toned_kv(
            out,
            "Walker errors",
            &thousands(diagnostics.walker_errors),
            color,
            Tone::Negative,
        );
    }
    if diagnostics.oversized_files > 0 {
        toned_kv(
            out,
            "Oversized files",
            &format!(
                "{} ({})",
                thousands(diagnostics.oversized_files),
                human_bytes(diagnostics.oversized_bytes)
            ),
            color,
            Tone::Caution,
        );
    }
    if diagnostics.files_omitted_by_limit > 0 {
        let count = if diagnostics.files_omitted_count_incomplete {
            format!("at least {}", thousands(diagnostics.files_omitted_by_limit))
        } else {
            thousands(diagnostics.files_omitted_by_limit)
        };
        toned_kv(
            out,
            "Known files omitted",
            &format!(
                "{} (known size {})",
                count,
                human_bytes(diagnostics.bytes_omitted_by_limit)
            ),
            color,
            Tone::Caution,
        );
    }
    if diagnostics.duration_limit_reached {
        toned_kv(out, "Scan duration", "limit reached", color, Tone::Caution);
    }
    if diagnostics.scan_truncated {
        toned_kv(
            out,
            "Scan completeness",
            "partial (resource limit reached)",
            color,
            Tone::Caution,
        );
    }
    render_type2_diagnostics(out, diagnostics, color);
    let _ = writeln!(out);
}

fn render_type2_diagnostics(out: &mut String, diagnostics: &ScanDiagnostics, color: bool) {
    if !diagnostics.type2_analysis_partial {
        return;
    }
    toned_kv(
        out,
        "Type-2 analysis",
        "partial (safety limit reached)",
        color,
        Tone::Caution,
    );
    kv(
        out,
        "Pools truncated",
        &thousands(diagnostics.type2_pools_truncated),
    );
    if diagnostics.type2_candidate_buckets_skipped > 0
        || diagnostics.type2_candidate_buckets_partially_selected > 0
    {
        kv(
            out,
            "Candidate buckets",
            &format!(
                "{} skipped, {} partially searched",
                thousands(diagnostics.type2_candidate_buckets_skipped),
                thousands(diagnostics.type2_candidate_buckets_partially_selected)
            ),
        );
    }
    kv(
        out,
        "Seed pairs skipped",
        &thousands_u64(diagnostics.type2_seed_pairs_skipped),
    );
    if diagnostics.type2_match_limit_reached {
        kv(out, "Match buffer limit", "reached");
    }
    if diagnostics.type2_suppression_limit_reached {
        kv(
            out,
            "Overlap work limit",
            &format!(
                "reached ({} matches omitted)",
                thousands(diagnostics.type2_matches_skipped_during_suppression)
            ),
        );
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "complexity table rendering keeps its summary and ranked callable projections together"
)]
fn render_complexity(out: &mut String, report: &ScanReport, color: bool) {
    let summary = &report.summary;
    let complexity = &summary.complexity;
    if complexity.cyclomatic_total > 0 || complexity.mi_avg > 0.0 {
        let _ = writeln!(out, "{}", header("Function complexity", color));
        kv(
            out,
            "Rule",
            &format!(
                "cyclomatic ≤ {} per function",
                complexity.cyclomatic_threshold
            ),
        );
        toned_kv(
            out,
            "Functions",
            &format!(
                "{} analyzed · {} over limit",
                thousands(complexity.functions),
                thousands(complexity.functions_over_threshold)
            ),
            color,
            if complexity.functions == 0 {
                Tone::Neutral
            } else if complexity.functions_over_threshold == 0 {
                Tone::Positive
            } else {
                Tone::Negative
            },
        );
        kv(
            out,
            "Cyclomatic/fn",
            &format!(
                "avg {:.1} · max {}",
                complexity.cyclomatic_avg, complexity.cyclomatic_max
            ),
        );
        kv(
            out,
            "Cognitive/fn",
            &format!(
                "avg {:.1} · max {}",
                complexity.cognitive_avg, complexity.cognitive_max
            ),
        );
        toned_kv(
            out,
            "File MI",
            &format!(
                "avg {:.1} · min {:.1}",
                complexity.mi_avg, complexity.mi_min
            ),
            color,
            if complexity.mi_min < 10.0 {
                Tone::Negative
            } else if complexity.mi_min < 20.0 {
                Tone::Caution
            } else {
                Tone::Positive
            },
        );
        if complexity.approximate_files > 0 {
            kv(
                out,
                "",
                &format!("({} files approximate)", complexity.approximate_files),
            );
        }
        let _ = writeln!(out);
    }

    if !summary.complexity_violations.is_empty() {
        let _ = writeln!(
            out,
            "{}",
            toned_header("Complexity violations", color, Tone::Negative)
        );
        let mut table = new_table(vec![
            "Function", "Location", "Cyclo", "Max", "Over", "Cog.", "Nest",
        ]);
        let rows = summary
            .complexity_violations
            .iter()
            .map(|function| {
                vec![
                    Cell::new(terminal_text(&function.name)),
                    Cell::new(terminal_text(&format!(
                        "{}:{}",
                        function.path.display(),
                        function.line
                    ))),
                    toned_cell(
                        thousands(function.cyclomatic as usize),
                        color,
                        Tone::Negative,
                    ),
                    Cell::new(thousands(complexity.cyclomatic_threshold as usize)),
                    toned_cell(
                        format!(
                            "+{}",
                            function
                                .cyclomatic
                                .saturating_sub(complexity.cyclomatic_threshold)
                        ),
                        color,
                        Tone::Negative,
                    ),
                    Cell::new(thousands(function.cognitive as usize)),
                    Cell::new(thousands(function.max_nesting as usize)),
                ]
            })
            .collect();
        add_responsive_path_cell_rows(&mut table, rows, 1, &[(0, 1), (1, 2)]);
        right_align(&mut table, &[2, 3, 4, 5, 6]);
        let _ = writeln!(out, "{table}");
        let _ = writeln!(out);
    } else if !summary.top_functions.is_empty() {
        let _ = writeln!(
            out,
            "{}",
            header("Most complex functions (all within limit)", color)
        );
        let mut table = new_table(vec!["Function", "Location", "Cyclo", "Cog.", "Nest"]);
        let rows = summary
            .top_functions
            .iter()
            .map(|function| {
                vec![
                    Cell::new(terminal_text(&function.name)),
                    Cell::new(terminal_text(&format!(
                        "{}:{}",
                        function.path.display(),
                        function.line
                    ))),
                    toned_cell(
                        thousands(function.cyclomatic as usize),
                        color,
                        complexity_tone(function.cyclomatic, complexity.cyclomatic_threshold),
                    ),
                    Cell::new(thousands(function.cognitive as usize)),
                    Cell::new(thousands(function.max_nesting as usize)),
                ]
            })
            .collect();
        add_responsive_path_cell_rows(&mut table, rows, 1, &[(0, 1), (1, 2)]);
        right_align(&mut table, &[2, 3, 4]);
        let _ = writeln!(out, "{table}");
        let _ = writeln!(out);
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "duplication table rendering keeps summary, coverage, truncation, and optional details in one output contract"
)]
fn render_duplication(
    out: &mut String,
    report: &ScanReport,
    color: bool,
    duplication_details: bool,
) {
    let summary = &report.summary;
    let duplication = &summary.duplication;
    let _ = writeln!(out, "{}", header("Duplication", color));
    toned_kv(
        out,
        "Exact groups",
        &thousands(duplication.exact_groups),
        color,
        count_tone(duplication.exact_groups),
    );
    toned_kv(
        out,
        "Near groups",
        &thousands(duplication.near_groups),
        color,
        count_tone(duplication.near_groups),
    );
    toned_kv(
        out,
        "Line coverage",
        &format!(
            "{:.1}% ({} / {} analyzed lines)",
            duplication.duplicated_pct,
            thousands(duplication.duplicated_lines),
            thousands(duplication.analyzed_lines)
        ),
        color,
        duplication_tone(duplication.duplicated_pct),
    );
    toned_kv(
        out,
        "Token coverage",
        &format!(
            "{:.1}% ({} / {} lexical tokens)",
            duplication.duplicated_tokens_pct,
            thousands(duplication.duplicated_tokens),
            thousands(duplication.analyzed_tokens)
        ),
        color,
        duplication_tone(duplication.duplicated_tokens_pct),
    );
    let _ = writeln!(out);

    let (title, duplicates) = human_duplicate_projection(summary);
    if !duplicates.is_empty() {
        let _ = writeln!(out, "{}", header(title, color));
        let mut table = new_table(vec![
            "Lines",
            "Copies",
            "Similarity",
            "Removable",
            "Locations",
        ]);
        let rows = duplicates
            .iter()
            .map(|duplicate| {
                vec![
                    thousands(duplicate.lines),
                    thousands(duplicate.copies),
                    similarity_label(duplicate.similarity),
                    thousands(duplicate.duplicated_lines),
                    terminal_text(&dup_locations(&duplicate.locations, duplicate.copies)),
                ]
            })
            .collect();
        add_responsive_path_rows(&mut table, rows, 4, &[(4, 1)]);
        right_align(&mut table, &[0, 1, 3]);
        let _ = writeln!(out, "{table}");
        let _ = writeln!(out);
    }

    if duplication_details && !report.duplicates.findings.is_empty() {
        let _ = writeln!(out, "{}", header("Duplicate findings", color));
        let mut table = new_table(vec![
            "ID",
            "Kind",
            "Format",
            "Lines",
            "Tokens",
            "Similarity",
            "Locations",
        ]);
        let rows = report
            .duplicates
            .findings
            .iter()
            .map(|finding| {
                vec![
                    terminal_text(&finding.id.chars().take(10).collect::<String>()),
                    terminal_text(&finding.kind),
                    terminal_text(&finding.format),
                    format!("{}/{}", finding.lines_a, finding.lines_b),
                    thousands(finding.tokens),
                    similarity_label(finding.similarity),
                    terminal_text(&format!(
                        "{}:{}:{} ↔ {}:{}:{}",
                        finding.fragment_a.path.display(),
                        finding.fragment_a.start_line,
                        finding.fragment_a.start_column,
                        finding.fragment_b.path.display(),
                        finding.fragment_b.start_line,
                        finding.fragment_b.start_column,
                    )),
                ]
            })
            .collect();
        add_responsive_path_rows(&mut table, rows, 6, &[(6, 1)]);
        right_align(&mut table, &[3, 4]);
        let _ = writeln!(out, "{table}");
        let _ = writeln!(out);
    }

    let languages = human_duplication_languages(&duplication.by_language);
    if !languages.is_empty() {
        let _ = writeln!(out, "{}", header("Duplication by language", color));
        let mut table = new_table(vec![
            "Language",
            "Groups E/N",
            "Line coverage",
            "Token coverage",
        ]);
        for language in languages {
            table.add_row(vec![
                Cell::new(terminal_text(&language.name)),
                Cell::new(format!(
                    "{}/{}",
                    language.exact_groups, language.near_groups
                )),
                toned_cell(
                    format!(
                        "{:.1}% ({})",
                        language.duplicated_lines_pct,
                        thousands(language.duplicated_lines)
                    ),
                    color,
                    duplication_tone(language.duplicated_lines_pct),
                ),
                toned_cell(
                    format!(
                        "{:.1}% ({})",
                        language.duplicated_tokens_pct,
                        thousands(language.duplicated_tokens)
                    ),
                    color,
                    duplication_tone(language.duplicated_tokens_pct),
                ),
            ]);
        }
        fit_responsive_table(&mut table, 0, &[(0, 1)]);
        right_align(&mut table, &[1, 2, 3]);
        let _ = writeln!(out, "{table}");
        let _ = writeln!(out);
    }
}

fn render_review(out: &mut String, report: &ScanReport, color: bool) {
    let Some(review) = &report.review else {
        return;
    };

    let _ = writeln!(out, "{}", header("Changed-line review", color));
    kv(out, "Mode", &review.mode);
    kv(out, "Scope", &review.scope);
    kv(out, "Changed files", &thousands(review.changed_files.len()));
    if review.mode == "deep" {
        kv(
            out,
            "Findings",
            &format!(
                "{} new · {} worsened · {} improved · {} resolved",
                thousands(review.counts.new),
                thousands(review.counts.worsened),
                thousands(review.counts.improved),
                thousands(review.counts.resolved)
            ),
        );
    } else {
        kv(out, "Findings", &thousands(review.counts.current));
    }
    if review.diagnostics.binary_files > 0 || review.diagnostics.unreadable_files > 0 {
        kv(
            out,
            "Skipped",
            &format!(
                "{} binary · {} unreadable",
                thousands(review.diagnostics.binary_files),
                thousands(review.diagnostics.unreadable_files)
            ),
        );
    }
    if review.diagnostics.oversized_files > 0 {
        kv(
            out,
            "Oversized snapshots",
            &format!(
                "{} ({})",
                thousands(review.diagnostics.oversized_files),
                human_bytes(review.diagnostics.oversized_bytes)
            ),
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
        kv(out, "Known snapshot files omitted", &count);
    }
    if review.diagnostics.duration_limit_reached {
        kv(out, "Review duration", "limit reached");
    }
    let _ = writeln!(out);

    if !review.findings.is_empty() {
        let mut table = new_table(vec!["State", "Kind", "Severity", "Location", "Finding"]);
        let mut rows = Vec::new();
        for item in &review.findings {
            let finding = item.after.as_ref().unwrap_or(&item.finding);
            rows.push(vec![
                toned_cell(
                    terminal_text(&item.state),
                    color,
                    review_state_tone(&item.state),
                ),
                Cell::new(terminal_text(&finding.kind)),
                Cell::new(terminal_text(&finding.severity)),
                Cell::new(terminal_text(&finding_location(finding))),
                Cell::new(terminal_text(&finding.message)),
            ]);
        }
        add_responsive_path_cell_rows(&mut table, rows, 3, &[(3, 1), (4, 2)]);
        let _ = writeln!(out, "{table}");
        let _ = writeln!(out);
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "graph table rendering is a linear projection of bounded graph diagnostics, focus, cycles, and symbol reach"
)]
fn render_graph(out: &mut String, report: &ScanReport, color: bool) {
    let Some(graph) = &report.graph else {
        return;
    };

    let _ = writeln!(
        out,
        "{}",
        header("Dependency graph (heuristic first-class languages)", color)
    );
    kv(
        out,
        "Languages",
        &if graph.languages.is_empty() {
            "none".to_string()
        } else {
            graph.languages.join(", ")
        },
    );
    kv(out, "Nodes", &thousands(graph.nodes));
    kv(out, "Edges", &thousands(graph.edges));
    toned_kv(
        out,
        "Cycles",
        &thousands(graph.cycles.len()),
        color,
        count_tone(graph.cycles.len()),
    );
    kv(out, "Orphans", &thousands(graph.orphans.len()));
    toned_kv(
        out,
        "Unresolved",
        &thousands(graph.unresolved_imports),
        color,
        count_tone(graph.unresolved_imports),
    );
    toned_kv(
        out,
        "Parse errors",
        &thousands(graph.parse_errors),
        color,
        error_count_tone(graph.parse_errors),
    );
    toned_kv(
        out,
        "Config errors",
        &thousands(graph.config_errors),
        color,
        error_count_tone(graph.config_errors),
    );
    if !graph.config_files.is_empty() {
        kv(out, "Resolver configs", &graph.config_files.join(", "));
    }
    if !graph.focus.is_empty() {
        kv(out, "Focus", &graph.focus.join(", "));
        kv(
            out,
            "Traversal",
            &format!(
                "{} · depth {}",
                graph.direction,
                graph.depth.unwrap_or_default()
            ),
        );
    }
    if !graph.unmatched_focus.is_empty() {
        kv(out, "Unmatched focus", &graph.unmatched_focus.join(", "));
    }
    let _ = writeln!(out);

    if !graph.focus.is_empty() && !graph.files.is_empty() {
        let _ = writeln!(out, "{}", header("  Focused graph files", color));
        let mut table = new_table(vec!["Path", "Distance", "Fan-in", "Fan-out"]);
        let rows = graph
            .files
            .iter()
            .map(|file| {
                vec![
                    terminal_text(&file.path),
                    file.focus_distance.unwrap_or_default().to_string(),
                    thousands(file.fan_in),
                    thousands(file.fan_out),
                ]
            })
            .collect();
        add_responsive_path_rows(&mut table, rows, 0, &[(0, 1)]);
        right_align(&mut table, &[1, 2, 3]);
        let _ = writeln!(out, "{table}");
        let _ = writeln!(out);
    }

    if !graph.cycles.is_empty() {
        let _ = writeln!(out, "{}", header("  Import cycles", color));
        for cycle in graph.cycles.iter().take(5) {
            kv(out, "", &cycle.join(" -> "));
        }
        if graph.cycles.len() > 5 {
            kv(out, "", &format!("… +{} more", graph.cycles.len() - 5));
        }
        let _ = writeln!(out);
    }

    if !graph.top_depended.is_empty() {
        let _ = writeln!(out, "{}", header("  Most depended-upon", color));
        let mut table = new_table(vec!["Path", "Fan-in"]);
        let rows = graph
            .top_depended
            .iter()
            .map(|node| vec![terminal_text(&node.path), thousands(node.fan_in)])
            .collect();
        add_responsive_path_rows(&mut table, rows, 0, &[(0, 1)]);
        right_align(&mut table, &[1]);
        let _ = writeln!(out, "{table}");
        let _ = writeln!(out);
    }

    if !graph.most_dependent.is_empty() {
        let _ = writeln!(out, "{}", header("  Most dependent", color));
        let mut table = new_table(vec!["Path", "Fan-out"]);
        let rows = graph
            .most_dependent
            .iter()
            .map(|node| vec![terminal_text(&node.path), thousands(node.fan_out)])
            .collect();
        add_responsive_path_rows(&mut table, rows, 0, &[(0, 1)]);
        right_align(&mut table, &[1]);
        let _ = writeln!(out, "{table}");
        let _ = writeln!(out);
    }

    if !graph.orphans.is_empty() {
        let _ = writeln!(out, "{}", header("  Orphans (dead-code candidates)", color));
        for orphan in graph.orphans.iter().take(15) {
            kv(out, "", orphan);
        }
        if graph.orphans.len() > 15 {
            kv(out, "", &format!("… +{} more", graph.orphans.len() - 15));
        }
        let _ = writeln!(out);
    }
}

fn render_impact(out: &mut String, report: &ScanReport, color: bool) {
    let Some(impact) = &report.impact else {
        return;
    };

    let _ = writeln!(
        out,
        "{}",
        header("Change impact (first-class languages)", color)
    );
    toned_kv(
        out,
        "Confidence",
        &impact.confidence,
        color,
        match impact.confidence.as_str() {
            "high" => Tone::Positive,
            "partial" => Tone::Caution,
            "low" => Tone::Negative,
            _ => Tone::Neutral,
        },
    );
    kv(out, "Changed files", &thousands(impact.changed_files.len()));
    kv(
        out,
        "Graph-covered",
        &thousands(impact.graph_changed_files.len()),
    );
    kv(
        out,
        "Direct dependents",
        &thousands(impact.direct_dependents.len()),
    );
    kv(
        out,
        "Transitive",
        &thousands(impact.transitive_dependents.len()),
    );
    kv(out, "Unresolved", &thousands(impact.unresolved_imports));
    kv(out, "Parse errors", &thousands(impact.parse_errors));
    kv(out, "Config errors", &thousands(impact.config_errors));
    if !impact.direct_dependents.is_empty() {
        kv(
            out,
            "Direct paths",
            &impact
                .direct_dependents
                .iter()
                .take(10)
                .cloned()
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    if !impact.transitive_dependents.is_empty() {
        kv(
            out,
            "Transitive paths",
            &impact
                .transitive_dependents
                .iter()
                .take(10)
                .cloned()
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    let _ = writeln!(out);
}

fn kv(out: &mut String, key: &str, value: &str) {
    toned_kv(out, key, value, false, Tone::Neutral);
}

fn toned_kv(out: &mut String, key: &str, value: &str, color: bool, tone: Tone) {
    let key = terminal_text(key);
    let value = terminal_text(value);
    let value = toned_value(&value, color, tone);
    let _ = writeln!(out, "  {key:<16} {value}");
}

fn toned_value(text: &str, color: bool, tone: Tone) -> String {
    if !color {
        return text.to_string();
    }
    match tone {
        Tone::Neutral => text.to_string(),
        Tone::Accent => format!("{}", text.cyan()),
        Tone::Positive => format!("{}", text.green()),
        Tone::Caution => format!("{}", text.yellow()),
        Tone::Negative => format!("{}", text.red()),
        Tone::Info => format!("{}", text.blue()),
    }
}

fn toned_cell(text: impl Into<String>, color: bool, tone: Tone) -> Cell {
    let cell = Cell::new_owned(text.into());
    if !color {
        return cell;
    }
    match tone {
        Tone::Neutral => cell,
        Tone::Accent => cell.fg(Color::Cyan),
        Tone::Positive => cell.fg(Color::Green),
        Tone::Caution => cell.fg(Color::Yellow),
        Tone::Negative => cell.fg(Color::Red),
        Tone::Info => cell.fg(Color::Blue),
    }
}

fn risk_tone(score: f64) -> Tone {
    if score >= 0.7 {
        Tone::Negative
    } else if score >= 0.4 {
        Tone::Caution
    } else {
        Tone::Neutral
    }
}

fn complexity_tone(value: u32, threshold: u32) -> Tone {
    if value > threshold {
        Tone::Negative
    } else if threshold > 0 && value.saturating_mul(4) >= threshold.saturating_mul(3) {
        Tone::Caution
    } else {
        Tone::Neutral
    }
}

fn duplication_tone(percentage: f64) -> Tone {
    if percentage > 0.0 {
        Tone::Caution
    } else {
        Tone::Positive
    }
}

fn count_tone(count: usize) -> Tone {
    if count == 0 {
        Tone::Positive
    } else {
        Tone::Caution
    }
}

fn error_count_tone(count: usize) -> Tone {
    if count == 0 {
        Tone::Positive
    } else {
        Tone::Negative
    }
}

fn marker_priority(marker: &str) -> u8 {
    match marker {
        "BUG" => 0,
        "FIXME" => 1,
        "HACK" => 2,
        "XXX" => 3,
        "TODO" => 4,
        _ => 5,
    }
}

fn marker_tone(marker: &str) -> Tone {
    match marker {
        "BUG" | "FIXME" => Tone::Negative,
        "HACK" | "XXX" => Tone::Caution,
        "TODO" => Tone::Info,
        _ => Tone::Neutral,
    }
}

fn review_state_tone(state: &str) -> Tone {
    match state {
        "new" | "worsened" => Tone::Negative,
        "improved" | "resolved" => Tone::Positive,
        _ => Tone::Caution,
    }
}

const DEFAULT_TABLE_WIDTH: usize = 100;
const CELL_HORIZONTAL_PADDING: usize = 2;

fn path_cell(path: &str, max_chars: usize) -> String {
    let escaped = terminal_text(path);
    if UnicodeWidthStr::width(escaped.as_str()) <= max_chars {
        return escaped;
    }
    if max_chars == 0 {
        return String::new();
    }
    let suffix_width = max_chars.saturating_sub(UnicodeWidthChar::width('…').unwrap_or(1));
    let mut used_width = 0usize;
    let mut suffix_start = escaped.len();
    for (index, character) in escaped.char_indices().rev() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if used_width.saturating_add(character_width) > suffix_width {
            break;
        }
        used_width = used_width.saturating_add(character_width);
        suffix_start = index;
    }
    format!("…{}", &escaped[suffix_start..])
}

fn output_width() -> Option<usize> {
    Table::new().width().map(usize::from)
}

fn add_responsive_path_rows(
    table: &mut Table,
    rows: Vec<Vec<String>>,
    path_column: usize,
    flexible_columns: &[(usize, usize)],
) {
    let rows = rows
        .into_iter()
        .map(|row| row.into_iter().map(Cell::new_owned).collect())
        .collect();
    add_responsive_path_cell_rows(table, rows, path_column, flexible_columns);
}

fn add_responsive_path_cell_rows(
    table: &mut Table,
    rows: Vec<Vec<Cell>>,
    path_column: usize,
    flexible_columns: &[(usize, usize)],
) {
    let mut sizing_table = table.clone();
    sizing_table.add_rows(rows.clone());
    let max_widths = sizing_table
        .column_max_content_widths()
        .into_iter()
        .map(usize::from)
        .collect::<Vec<_>>();
    if max_widths.is_empty() || path_column >= max_widths.len() {
        table.add_rows(rows);
        return;
    }

    let table_width = sizing_table
        .width()
        .map_or(DEFAULT_TABLE_WIDTH, usize::from);
    let widths = responsive_column_widths(table_width, &max_widths, path_column, flexible_columns);
    apply_responsive_widths(table, table_width, &widths);

    for mut cells in rows {
        if let Some(path) = cells.get_mut(path_column) {
            *path = Cell::new(path_cell(&path.content(), widths[path_column]));
        }
        let mut row = Row::from(cells);
        row.max_height(1);
        table.add_row(row);
    }
}

fn fit_responsive_table(
    table: &mut Table,
    expanding_column: usize,
    flexible_columns: &[(usize, usize)],
) {
    let max_widths = table
        .column_max_content_widths()
        .into_iter()
        .map(usize::from)
        .collect::<Vec<_>>();
    if max_widths.is_empty() || expanding_column >= max_widths.len() {
        return;
    }
    let table_width = table.width().map_or(DEFAULT_TABLE_WIDTH, usize::from);
    let widths =
        responsive_column_widths(table_width, &max_widths, expanding_column, flexible_columns);
    apply_responsive_widths(table, table_width, &widths);
    for row in table.row_iter_mut() {
        row.max_height(1);
    }
}

fn apply_responsive_widths(table: &mut Table, table_width: usize, widths: &[usize]) {
    table
        .set_width(u16::try_from(table_width).unwrap_or(u16::MAX))
        .set_content_arrangement(ContentArrangement::Dynamic);
    for (index, width) in widths.iter().copied().enumerate() {
        if let Some(column) = table.column_mut(index) {
            column.set_constraint(ColumnConstraint::Absolute(Width::Fixed(
                u16::try_from(width.saturating_add(CELL_HORIZONTAL_PADDING)).unwrap_or(u16::MAX),
            )));
        }
    }
}

fn responsive_column_widths(
    table_width: usize,
    max_widths: &[usize],
    path_column: usize,
    flexible_columns: &[(usize, usize)],
) -> Vec<usize> {
    let column_count = max_widths.len();
    let structural_width = column_count
        .saturating_add(1)
        .saturating_add(column_count.saturating_mul(CELL_HORIZONTAL_PADDING));
    let content_budget = table_width
        .saturating_sub(structural_width)
        .max(column_count);
    let mut widths = max_widths.to_vec();
    let is_flexible = |index| flexible_columns.iter().any(|(column, _)| *column == index);
    for (index, width) in widths.iter_mut().enumerate() {
        if is_flexible(index) {
            *width = 0;
        }
    }

    let minimum_flexible_width = flexible_columns.len().min(content_budget);
    let fixed_budget = content_budget.saturating_sub(minimum_flexible_width);
    shrink_fixed_columns(&mut widths, flexible_columns, fixed_budget);
    let fixed_width = widths.iter().sum::<usize>();
    let flexible_budget = content_budget.saturating_sub(fixed_width);
    distribute_flexible_width(
        &mut widths,
        max_widths,
        path_column,
        flexible_columns,
        flexible_budget,
    );
    widths
}

fn shrink_fixed_columns(widths: &mut [usize], flexible_columns: &[(usize, usize)], budget: usize) {
    let mut current = widths.iter().sum::<usize>();
    while current > budget {
        let Some(index) = widths
            .iter()
            .enumerate()
            .filter(|(index, width)| {
                **width > 1 && !flexible_columns.iter().any(|(column, _)| column == index)
            })
            .max_by_key(|(_, width)| **width)
            .map(|(index, _)| index)
        else {
            break;
        };
        widths[index] -= 1;
        current -= 1;
    }
}

fn distribute_flexible_width(
    widths: &mut [usize],
    max_widths: &[usize],
    path_column: usize,
    flexible_columns: &[(usize, usize)],
    budget: usize,
) {
    if flexible_columns.is_empty() {
        return;
    }
    let base = usize::from(budget >= flexible_columns.len());
    for (column, _) in flexible_columns {
        widths[*column] = base;
    }
    let mut remaining = budget.saturating_sub(base.saturating_mul(flexible_columns.len()));
    let weight_total = flexible_columns
        .iter()
        .map(|(_, weight)| (*weight).max(1))
        .sum::<usize>();
    let distributable = remaining;
    for (column, weight) in flexible_columns {
        let share = distributable.saturating_mul((*weight).max(1)) / weight_total;
        widths[*column] = widths[*column].saturating_add(share);
        remaining = remaining.saturating_sub(share);
    }
    widths[path_column] = widths[path_column].saturating_add(remaining);

    let mut reclaimed = 0;
    for (column, _) in flexible_columns {
        if *column == path_column {
            continue;
        }
        let maximum = max_widths[*column].max(1);
        if widths[*column] > maximum {
            reclaimed += widths[*column] - maximum;
            widths[*column] = maximum;
        }
    }
    widths[path_column] = widths[path_column].saturating_add(reclaimed);
}

fn new_table(headers: Vec<&str>) -> Table {
    let mut t = Table::new();
    t.load_style(UTF8_BORDERS_ONLY)
        .set_content_arrangement(ContentArrangement::DynamicFullWidth)
        .set_header(headers);
    t
}

fn right_align(table: &mut Table, columns: &[usize]) {
    for &i in columns {
        if let Some(col) = table.column_mut(i) {
            col.set_cell_alignment(CellAlignment::Right);
        }
    }
}

#[cfg(test)]
mod tests;
