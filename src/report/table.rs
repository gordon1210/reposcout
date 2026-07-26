//! Human-facing terminal report: aligned key/value sections plus tables for
//! language breakdown, token-heavy files, and churn hotspots.

use crate::model::{ScanDiagnostics, ScanReport};
use crate::report::projection::{
    file_cyclomatic_average, finding_location, human_test_signal, metric_delta_display,
    metric_label, source_language_rollup,
};
use crate::report::{
    dup_locations, human_bytes, similarity_label, terminal_text, thousands, thousands_u64,
};
use comfy_table::presets::UTF8_BORDERS_ONLY;
use comfy_table::{CellAlignment, ContentArrangement, Table};
use owo_colors::OwoColorize;
use std::fmt::Write as _;
use std::path::Path;

pub fn render(report: &ScanReport, color: bool, duplication_details: bool) -> String {
    let mut out = String::new();
    let s = &report.summary;

    let title = format!(
        "reposcout  {}",
        terminal_text(&report.target.display().to_string())
    );
    let _ = writeln!(
        out,
        "{}   (encoding: {})",
        header(&title, color),
        terminal_text(&report.encoding)
    );
    let _ = writeln!(out);

    render_overview(&mut out, report, color);

    render_complexity(&mut out, report, color);

    render_duplication(&mut out, report, color, duplication_details);

    // Markers
    if !s.markers.is_empty() {
        let markers: Vec<String> = s
            .markers
            .iter()
            .map(|(k, v)| format!("{} {v}", terminal_text(k)))
            .collect();
        let _ = writeln!(out, "{}  {}", header("Markers", color), markers.join(" · "));
        let _ = writeln!(out);
    }

    // Symbols
    let sym = &s.symbols;
    if sym.functions > 0 || sym.types > 0 || sym.exports > 0 {
        let _ = writeln!(
            out,
            "{}  functions {}, types {}, exports {}",
            header("Symbols", color),
            thousands(sym.functions),
            thousands(sym.types),
            thousands(sym.exports),
        );
        let _ = writeln!(out);
    }

    // Skip candidates
    if !s.skip_candidates.is_empty() {
        let _ = writeln!(
            out,
            "{}",
            header("Skip candidates (generated/minified/vendored)", color)
        );
        let mut t = new_table(vec!["Path", "Reason", "Tokens"]);
        for c in &s.skip_candidates {
            t.add_row(vec![
                terminal_text(&c.path),
                terminal_text(&c.reason),
                thousands(c.tokens),
            ]);
        }
        right_align(&mut t, &[2]);
        let _ = writeln!(out, "{t}");
        let _ = writeln!(out);
    }

    // Languages
    if !s.languages.is_empty() {
        let _ = writeln!(out, "{}", header("Source languages", color));
        let mut t = new_table(vec!["Language", "Files", "SLOC", "Comment%", "Tokens"]);
        for l in source_language_rollup(&s.languages) {
            let cpct = if l.loc > 0 {
                l.comment_lines as f64 / l.loc as f64 * 100.0
            } else {
                0.0
            };
            t.add_row(vec![
                terminal_text(&l.name),
                thousands(l.files),
                thousands(l.sloc),
                format!("{cpct:.0}%"),
                thousands(l.tokens),
            ]);
        }
        right_align(&mut t, &[1, 2, 3, 4]);
        let _ = writeln!(out, "{t}");
        let _ = writeln!(out);
    }

    // Top source files by tokens
    if !s.top_source_token_files.is_empty() {
        let _ = writeln!(out, "{}", header("Top source files by tokens", color));
        let mut t = new_table(vec!["File", "Tokens"]);
        for f in &s.top_source_token_files {
            t.add_row(vec![
                terminal_text(&f.path.display().to_string()),
                thousands(f.tokens),
            ]);
        }
        right_align(&mut t, &[1]);
        let _ = writeln!(out, "{t}");
        let _ = writeln!(out);
    }

    // Hotspots
    if !s.top_hotspots.is_empty() {
        let _ = writeln!(out, "{}", header("Hotspots (churn × complexity)", color));
        let mut t = new_table(vec!["File", "Commits", "Cyclomatic", "Avg/fn", "Score"]);
        for h in &s.top_hotspots {
            t.add_row(vec![
                terminal_text(&h.path.display().to_string()),
                thousands(h.commits),
                thousands(h.cyclomatic as usize),
                file_cyclomatic_average(report, &h.path)
                    .map(|average| format!("{average:.1}"))
                    .unwrap_or_else(|| "-".to_string()),
                format!("{:.0}", h.score),
            ]);
        }
        right_align(&mut t, &[1, 2, 3, 4]);
        let _ = writeln!(out, "{t}");
        let _ = writeln!(out);
    }

    // Assessment
    {
        let a = &s.assessment;
        let _ = writeln!(out, "{}", header("Assessment", color));
        kv(
            &mut out,
            "Context",
            &if a.fits_context_known {
                format!(
                    "{} (budget {})",
                    if a.fits_context { "fits" } else { "exceeds" },
                    thousands(a.token_budget)
                )
            } else {
                "unknown (tokens unavailable)".to_string()
            },
        );
        kv(
            &mut out,
            "Cleanup worth",
            &if a.cleanup_worth_complete {
                a.cleanup_worth.clone()
            } else {
                format!("{} (partial)", a.cleanup_worth)
            },
        );
        if !a.unavailable_signals.is_empty() {
            kv(&mut out, "Unavailable", &a.unavailable_signals.join(", "));
        }
        if !a.reasons.is_empty() {
            kv(&mut out, "Reasons", &a.reasons.join("; "));
        }
        let _ = writeln!(out);
    }

    // Test presence
    {
        let tp = &s.test_presence;
        let _ = writeln!(out, "{}", header("Test presence", color));
        kv(&mut out, "Test files", &thousands(tp.test_files));
        kv(&mut out, "Source files", &thousands(tp.source_files));
        kv(
            &mut out,
            "No matching test",
            &thousands(tp.untested_source_files),
        );
        if !tp.untested_samples.is_empty() {
            kv(&mut out, "Samples", &tp.untested_samples.join(", "));
        }
        let _ = writeln!(out);
    }

    // Top risks
    if !s.top_risks.is_empty() {
        let _ = writeln!(out, "{}", header("Top risks", color));
        let mut t = new_table(vec![
            "Path",
            "Score",
            "SLOC",
            "Cyclomatic",
            "Avg/fn",
            "Churn",
            "Reasons",
        ]);
        for r in &s.top_risks {
            t.add_row(vec![
                terminal_text(&r.path),
                format!("{:.2}", r.score),
                thousands(r.sloc),
                thousands(r.cyclomatic as usize),
                file_cyclomatic_average(report, Path::new(&r.path))
                    .map(|average| format!("{average:.1}"))
                    .unwrap_or_else(|| "-".to_string()),
                thousands(r.churn_commits),
                terminal_text(
                    &r.reasons
                        .iter()
                        .map(|reason| human_test_signal(reason))
                        .collect::<Vec<_>>()
                        .join(", "),
                ),
            ]);
        }
        right_align(&mut t, &[1, 2, 3, 4, 5]);
        let _ = writeln!(out, "{t}");
        let _ = writeln!(out);
    }

    render_context(&mut out, report, color);

    // By directory
    if !report.directories.is_empty() {
        let _ = writeln!(out, "{}", header("By directory", color));
        let mut t = new_table(vec![
            "Path",
            "Files",
            "Tokens",
            "SLOC",
            "Cyclo avg",
            "MI avg",
            "Dup lines",
            "No matching test",
        ]);
        for d in &report.directories {
            t.add_row(vec![
                terminal_text(&d.path),
                thousands(d.files),
                thousands(d.tokens),
                thousands(d.sloc),
                format!("{:.1}", d.cyclomatic_avg),
                format!("{:.0}", d.mi_avg),
                thousands(d.duplicated_lines),
                thousands(d.untested_source_files),
            ]);
        }
        right_align(&mut t, &[1, 2, 3, 4, 5, 6, 7]);
        let _ = writeln!(out, "{t}");
        let _ = writeln!(out);
    }

    render_baseline(&mut out, report, color);

    render_review(&mut out, report, color);

    render_graph(&mut out, report, color);
    render_impact(&mut out, report, color);

    out
}

fn header(text: &str, color: bool) -> String {
    if color {
        format!("{}", text.cyan().bold())
    } else {
        text.to_string()
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
    kv(out, "Regressions", &regressions);
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

fn render_context(out: &mut String, report: &ScanReport, color: bool) {
    let Some(context) = &report.context else {
        return;
    };
    let _ = writeln!(out, "{}", header("Agent context plan", color));
    kv(
        out,
        "Token budget",
        &format!(
            "{} / {} selected",
            thousands(context.selected_tokens),
            thousands(context.budget_tokens)
        ),
    );
    kv(
        out,
        "Files",
        &format!(
            "{} selected · {} candidates · {} omitted · {} skipped",
            context.files.len(),
            context.candidate_files,
            context.omitted_files,
            context.skipped_files
        ),
    );
    kv(
        out,
        "Planning",
        &format!(
            "{} ms · {} outline symbols · {} bytes · {} outline omissions",
            context.planning_ms,
            context.outline_symbols,
            context.outline_bytes,
            context.outline_omitted_symbols
        ),
    );
    if !context.focus.is_empty() {
        kv(
            out,
            "Focus",
            &context
                .focus
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    if !context.unmatched_focus.is_empty() {
        kv(
            out,
            "Unmatched focus",
            &context
                .unmatched_focus
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
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
    if let Some(diagnostics) = &context.planning_diagnostics {
        kv(
            out,
            "Planning coverage",
            &format!(
                "{} / {} analyzed · {} unsupported · {} unreadable · {} walker errors",
                diagnostics.analyzed_files,
                diagnostics.discovered_files,
                diagnostics.unsupported_files,
                diagnostics.unreadable_files,
                diagnostics.walker_errors
            ),
        );
    }
    if let Some(scope) = &context.change_scope {
        let mut paths = context
            .changed_files
            .iter()
            .take(10)
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>();
        if context.changed_files.len() > paths.len() {
            paths.push(format!(
                "+{} more",
                context.changed_files.len() - paths.len()
            ));
        }
        let paths = if paths.is_empty() {
            "no changed paths".to_string()
        } else {
            paths.join(", ")
        };
        kv(out, "Change seed", &format!("{scope} · {paths}"));
    }
    if !context.files.is_empty() {
        let mut table = new_table(vec!["File", "Tokens", "Score", "Why"]);
        for file in &context.files {
            table.add_row(vec![
                terminal_text(&file.path.display().to_string()),
                thousands(file.tokens),
                format!("{:.2}", file.score),
                terminal_text(&file.reasons.join(", ")),
            ]);
        }
        right_align(&mut table, &[1, 2]);
        let _ = writeln!(out, "{table}");
    }
    if !context.outline_only.is_empty() {
        let _ = writeln!(out, "{}", header("Outline-only focus", color));
        let mut table = new_table(vec!["File", "Source tokens", "Reason"]);
        for file in &context.outline_only {
            table.add_row(vec![
                terminal_text(&file.path.display().to_string()),
                thousands(file.source_tokens),
                terminal_text(&file.reason),
            ]);
        }
        right_align(&mut table, &[1]);
        let _ = writeln!(out, "{table}");
    }
    if context.files.iter().any(|file| !file.symbols.is_empty()) {
        let _ = writeln!(out, "{}", header("Selected symbol outlines", color));
        let mut table = new_table(vec!["File", "Line", "Kind", "Signature", "Why"]);
        for file in &context.files {
            for symbol in &file.symbols {
                table.add_row(vec![
                    terminal_text(&file.path.display().to_string()),
                    thousands(symbol.line),
                    terminal_text(&symbol.kind),
                    terminal_text(&symbol.signature),
                    terminal_text(&symbol.reasons.join(", ")),
                ]);
            }
        }
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
        for file in &context.outline_only {
            for symbol in &file.symbols {
                table.add_row(vec![
                    terminal_text(&file.path.display().to_string()),
                    thousands(symbol.line),
                    terminal_text(&symbol.kind),
                    terminal_text(&symbol.signature),
                    terminal_text(&symbol.reasons.join(", ")),
                ]);
            }
        }
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
    let diagnostics = &report.diagnostics;
    render_scan_diagnostics(out, diagnostics, color);

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
        summary.source.comment_lines as f64 / summary.source.loc as f64
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

    let _ = writeln!(out, "{}", header("Scan diagnostics", color));
    kv(out, "Discovered", &thousands(diagnostics.discovered_files));
    kv(out, "Analyzed", &thousands(diagnostics.analyzed_files));
    if diagnostics.unsupported_files > 0 {
        kv(
            out,
            "Unsupported",
            &thousands(diagnostics.unsupported_files),
        );
    }
    if diagnostics.unreadable_files > 0 {
        kv(out, "Unreadable", &thousands(diagnostics.unreadable_files));
    }
    if diagnostics.walker_errors > 0 {
        kv(out, "Walker errors", &thousands(diagnostics.walker_errors));
    }
    if diagnostics.oversized_files > 0 {
        kv(
            out,
            "Oversized files",
            &format!(
                "{} ({})",
                thousands(diagnostics.oversized_files),
                human_bytes(diagnostics.oversized_bytes)
            ),
        );
    }
    if diagnostics.files_omitted_by_limit > 0 {
        let count = if diagnostics.files_omitted_count_incomplete {
            format!("at least {}", thousands(diagnostics.files_omitted_by_limit))
        } else {
            thousands(diagnostics.files_omitted_by_limit)
        };
        kv(
            out,
            "Known files omitted",
            &format!(
                "{} (known size {})",
                count,
                human_bytes(diagnostics.bytes_omitted_by_limit)
            ),
        );
    }
    if diagnostics.duration_limit_reached {
        kv(out, "Scan duration", "limit reached");
    }
    if diagnostics.scan_truncated {
        kv(out, "Scan completeness", "partial (resource limit reached)");
    }
    if diagnostics.type2_analysis_partial {
        kv(out, "Type-2 analysis", "partial (safety limit reached)");
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
    let _ = writeln!(out);
}

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
        kv(
            out,
            "Functions",
            &format!(
                "{} analyzed · {} over limit",
                thousands(complexity.functions),
                thousands(complexity.functions_over_threshold)
            ),
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
        kv(
            out,
            "File MI",
            &format!(
                "avg {:.1} · min {:.1}",
                complexity.mi_avg, complexity.mi_min
            ),
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
        let _ = writeln!(out, "{}", header("Complexity violations", color));
        let mut table = new_table(vec![
            "Function",
            "Location",
            "Cyclomatic",
            "Max",
            "Over",
            "Cognitive",
            "Nesting",
        ]);
        for function in &summary.complexity_violations {
            table.add_row(vec![
                terminal_text(&function.name),
                terminal_text(&format!("{}:{}", function.path.display(), function.line)),
                thousands(function.cyclomatic as usize),
                thousands(complexity.cyclomatic_threshold as usize),
                format!(
                    "+{}",
                    function
                        .cyclomatic
                        .saturating_sub(complexity.cyclomatic_threshold)
                ),
                thousands(function.cognitive as usize),
                thousands(function.max_nesting as usize),
            ]);
        }
        right_align(&mut table, &[2, 3, 4, 5, 6]);
        let _ = writeln!(out, "{table}");
        let _ = writeln!(out);
    } else if !summary.top_functions.is_empty() {
        let _ = writeln!(
            out,
            "{}",
            header("Most complex functions (all within limit)", color)
        );
        let mut table = new_table(vec![
            "Function",
            "Location",
            "Cyclomatic",
            "Cognitive",
            "Nesting",
        ]);
        for function in &summary.top_functions {
            table.add_row(vec![
                terminal_text(&function.name),
                terminal_text(&format!("{}:{}", function.path.display(), function.line)),
                thousands(function.cyclomatic as usize),
                thousands(function.cognitive as usize),
                thousands(function.max_nesting as usize),
            ]);
        }
        right_align(&mut table, &[2, 3, 4]);
        let _ = writeln!(out, "{table}");
        let _ = writeln!(out);
    }
}

fn render_duplication(
    out: &mut String,
    report: &ScanReport,
    color: bool,
    duplication_details: bool,
) {
    let summary = &report.summary;
    let duplication = &summary.duplication;
    let _ = writeln!(out, "{}", header("Duplication", color));
    kv(out, "Exact groups", &thousands(duplication.exact_groups));
    kv(out, "Near groups", &thousands(duplication.near_groups));
    kv(
        out,
        "Line coverage",
        &format!(
            "{:.1}% ({} / {} analyzed lines)",
            duplication.duplicated_pct,
            thousands(duplication.duplicated_lines),
            thousands(duplication.analyzed_lines)
        ),
    );
    kv(
        out,
        "Token coverage",
        &format!(
            "{:.1}% ({} / {} lexical tokens)",
            duplication.duplicated_tokens_pct,
            thousands(duplication.duplicated_tokens),
            thousands(duplication.analyzed_tokens)
        ),
    );
    let _ = writeln!(out);

    if !summary.top_duplicates.is_empty() {
        let _ = writeln!(out, "{}", header("Top duplicates", color));
        let mut table = new_table(vec![
            "Lines",
            "Copies",
            "Similarity",
            "Removable",
            "Locations",
        ]);
        for duplicate in &summary.top_duplicates {
            table.add_row(vec![
                thousands(duplicate.lines),
                thousands(duplicate.copies),
                similarity_label(duplicate.similarity),
                thousands(duplicate.duplicated_lines),
                terminal_text(&dup_locations(&duplicate.locations, duplicate.copies)),
            ]);
        }
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
        for finding in &report.duplicates.findings {
            table.add_row(vec![
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
            ]);
        }
        right_align(&mut table, &[3, 4]);
        let _ = writeln!(out, "{table}");
        let _ = writeln!(out);
    }

    if !duplication.by_language.is_empty() {
        let _ = writeln!(out, "{}", header("Duplication by language", color));
        let mut table = new_table(vec![
            "Language",
            "Groups E/N",
            "Line coverage",
            "Token coverage",
        ]);
        for language in &duplication.by_language {
            table.add_row(vec![
                terminal_text(&language.name),
                format!("{}/{}", language.exact_groups, language.near_groups),
                format!(
                    "{:.1}% ({})",
                    language.duplicated_lines_pct,
                    thousands(language.duplicated_lines)
                ),
                format!(
                    "{:.1}% ({})",
                    language.duplicated_tokens_pct,
                    thousands(language.duplicated_tokens)
                ),
            ]);
        }
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
        for item in &review.findings {
            let finding = item.after.as_ref().unwrap_or(&item.finding);
            table.add_row(vec![
                terminal_text(&item.state),
                terminal_text(&finding.kind),
                terminal_text(&finding.severity),
                terminal_text(&finding_location(finding)),
                terminal_text(&finding.message),
            ]);
        }
        let _ = writeln!(out, "{table}");
        let _ = writeln!(out);
    }
}

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
    kv(out, "Cycles", &thousands(graph.cycles.len()));
    kv(out, "Orphans", &thousands(graph.orphans.len()));
    kv(out, "Unresolved", &thousands(graph.unresolved_imports));
    kv(out, "Parse errors", &thousands(graph.parse_errors));
    kv(out, "Config errors", &thousands(graph.config_errors));
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
        for file in &graph.files {
            table.add_row(vec![
                terminal_text(&file.path),
                file.focus_distance.unwrap_or_default().to_string(),
                thousands(file.fan_in),
                thousands(file.fan_out),
            ]);
        }
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
        for node in &graph.top_depended {
            table.add_row(vec![terminal_text(&node.path), thousands(node.fan_in)]);
        }
        right_align(&mut table, &[1]);
        let _ = writeln!(out, "{table}");
        let _ = writeln!(out);
    }

    if !graph.most_dependent.is_empty() {
        let _ = writeln!(out, "{}", header("  Most dependent", color));
        let mut table = new_table(vec!["Path", "Fan-out"]);
        for node in &graph.most_dependent {
            table.add_row(vec![terminal_text(&node.path), thousands(node.fan_out)]);
        }
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
    kv(out, "Confidence", &impact.confidence);
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
    let key = terminal_text(key);
    let value = terminal_text(value);
    let _ = writeln!(out, "  {key:<16} {value}");
}

fn new_table(headers: Vec<&str>) -> Table {
    let mut t = Table::new();
    t.load_preset(UTF8_BORDERS_ONLY)
        .set_content_arrangement(ContentArrangement::Dynamic)
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
mod tests {
    use super::render_scan_diagnostics;
    use crate::model::ScanDiagnostics;

    #[test]
    fn partial_type2_analysis_is_visible_in_human_diagnostics() {
        let diagnostics = ScanDiagnostics {
            type2_analysis_partial: true,
            type2_pools_truncated: 1,
            type2_candidate_buckets_skipped: 12,
            type2_candidate_buckets_partially_selected: 1,
            type2_seed_pairs_skipped: 42,
            type2_match_limit_reached: true,
            ..ScanDiagnostics::default()
        };
        let mut out = String::new();

        render_scan_diagnostics(&mut out, &diagnostics, false);

        assert!(out.contains("Type-2 analysis"));
        assert!(out.contains("partial (safety limit reached)"));
        assert!(out.contains("Seed pairs skipped"));
        assert!(out.contains("42"));
        assert!(out.contains("Match buffer limit"));
    }

    #[test]
    fn incomplete_omission_counts_are_labeled_as_lower_bounds() {
        let diagnostics = ScanDiagnostics {
            files_omitted_by_limit: 1,
            files_omitted_count_incomplete: true,
            scan_truncated: true,
            ..ScanDiagnostics::default()
        };
        let mut out = String::new();

        render_scan_diagnostics(&mut out, &diagnostics, false);

        assert!(out.contains("Known files omitted"));
        assert!(out.contains("at least 1"));
    }
}
