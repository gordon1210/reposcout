use super::{Format, json_string, terminal_text};
use crate::model::{ReviewAnalysisStatus, ReviewContextReport};
use anyhow::{Result, bail};
use std::fmt::Write as _;

pub(crate) fn render(report: &ReviewContextReport, format: Format, pretty: bool) -> Result<String> {
    let mut output = match format {
        Format::Json => json_string(report, pretty)?,
        Format::Ndjson => json_string(report, false)?,
        Format::Table | Format::Markdown => human(report),
        _ => bail!("review-context supports table, JSON, Markdown, or NDJSON output"),
    };
    if !output.ends_with('\n') {
        output.push('\n');
    }
    Ok(output)
}

fn human(report: &ReviewContextReport) -> String {
    let mut output = header(report);
    coverage(report, &mut output);
    changes(report, &mut output);
    context(report, &mut output);
    relations(report, &mut output);
    output
}

fn relations(report: &ReviewContextReport, output: &mut String) {
    for relation in &report.relations {
        let _ = writeln!(
            output,
            "{} {}: {} -> {} [{}]",
            relation.side,
            relation.kind,
            terminal_text(&relation.edge.source),
            terminal_text(&relation.edge.target),
            terminal_text(&relation.edge.resolver)
        );
        if let Some(symbol) = &relation.symbol {
            let _ = writeln!(
                output,
                "  {:?} {}:{} -> {}:{}; site={}-{} syntax={:?}",
                symbol.kind,
                terminal_text(&symbol.source.name),
                symbol.source.declaration_span.start_line,
                terminal_text(&symbol.target.name),
                symbol.target.declaration_span.start_line,
                symbol.site.start_line,
                symbol.site.end_line,
                symbol.syntax
            );
        }
        if let Some(edge) = &relation.type_relation {
            let _ = writeln!(
                output,
                "  {}: {} -> {}",
                terminal_text(&edge.relation),
                terminal_text(&edge.source),
                terminal_text(&edge.target)
            );
        }
    }
}

fn header(report: &ReviewContextReport) -> String {
    let t = &report.totals;
    let c = &report.comparison;
    let mut output = format!(
        "Review context ({}): {} -> {}\nRequested base: {}\nTrees: {} -> {}\nEncoding: {}\n",
        c.mode,
        c.base_commit,
        c.head_commit,
        c.requested_base_commit,
        c.base_tree,
        c.head_tree,
        report.encoding
    );
    let _ = writeln!(
        output,
        "Changes: {} ({} not analyzed, {} without hunk analysis, {} output omitted); definitions: {} ({} omitted); relations: {} ({} omitted)",
        t.changes,
        t.changes_not_analyzed,
        t.changes_without_hunks,
        t.changes_omitted,
        t.definitions,
        t.definitions_omitted,
        t.relations,
        t.relations_omitted
    );
    let _ = writeln!(
        output,
        "Context: {} file sides / {} source tokens / {} bytes; {} unknown sizes; {} entries omitted",
        t.candidates,
        t.candidate_tokens,
        t.candidate_bytes,
        t.unknown_candidate_sizes,
        t.candidates_omitted
    );
    let _ = writeln!(
        output,
        "Selection: {} files / {} tokens; omitted by selection: {} files / {} tokens; source bodies omitted: {}",
        t.selected_files,
        t.selected_tokens,
        t.selection_omitted_files,
        t.selection_omitted_tokens,
        t.source_files_omitted
    );
    let _ = writeln!(
        output,
        "Diff: {} tokens; {} unavailable files; {} bodies omitted",
        t.diff_tokens, t.diff_unavailable_files, t.diff_files_omitted
    );
    let _ = writeln!(
        output,
        "Rename detection complete: {}",
        c.rename_detection_complete
    );
    output
}

fn coverage(report: &ReviewContextReport, output: &mut String) {
    for side in &report.coverage {
        let _ = writeln!(
            output,
            "Coverage {}: {}/{} captured; inventory truncated={}; unavailable={:?}; graph={}; unresolved imports={}; parse/config errors={}/{}; unresolved calls={}",
            side.tree,
            side.captured_files,
            side.observed_files,
            side.inventory_truncated,
            side.unavailable_files,
            side.graph_files,
            side.unresolved_imports,
            side.parse_errors,
            side.config_errors,
            side.call_resolution.unresolved
        );
        let _ = writeln!(
            output,
            "  Changed graph files: {}; changed without graph: {}; unsupported graph paths: {}; unsupported inventory: {}; unsupported/incomplete call files: {}/{}; unresolved type relations: {}",
            side.changed_graph_files,
            side.changed_files_without_graph,
            side.unsupported_graph_paths,
            side.unsupported_inventory_files,
            side.unsupported_call_files,
            side.incomplete_call_files,
            side.unresolved_type_relations
        );
    }
    for limitation in &report.limitations {
        let _ = writeln!(output, "Note: {limitation}");
    }
}

fn changes(report: &ReviewContextReport, output: &mut String) {
    for change in &report.changes {
        let _ = writeln!(
            output,
            "Change: {} ({} hunks, {} omitted; analysis={:?})",
            change.status,
            if change.hunk_status == ReviewAnalysisStatus::Unavailable {
                "unknown".into()
            } else {
                change.hunks.to_string()
            },
            change.hunks_omitted,
            change.hunk_status
        );
        for (label, side) in [("base", &change.base), ("head", &change.head)] {
            if let Some(side) = side {
                let _ = writeln!(
                    output,
                    "  {label}: {} mode={:o} {} blob={} hash={} extraction={:?} mapping={:?}; unmapped={} unprocessed={} ambiguous={} wrapper={}",
                    terminal_text(&side.path.to_string_lossy()),
                    side.mode,
                    side.status,
                    side.blob,
                    side.sha256.as_deref().unwrap_or("unknown"),
                    side.extraction,
                    side.mapping_status,
                    side.unmapped_ranges,
                    side.unprocessed_ranges,
                    side.ambiguous_definitions,
                    side.wrapper_ranges
                );
                let ranges = side
                    .ranges
                    .iter()
                    .map(|range| format!("{}-{}", range.start, range.end))
                    .collect::<Vec<_>>()
                    .join(",");
                let _ = writeln!(output, "    ranges={ranges}");
                for definition in &side.definitions {
                    let _ = writeln!(
                        output,
                        "    {}:{}-{} {}",
                        terminal_text(&definition.symbol.name),
                        definition.declaration_span.start_line,
                        definition.declaration_span.end_line,
                        terminal_text(&definition.symbol.kind)
                    );
                }
            }
        }
        if let Some(diff) = &change.diff {
            let _ = writeln!(output, "{}", terminal_text(diff));
        }
    }
}

fn context(report: &ReviewContextReport, output: &mut String) {
    for file in &report.context {
        let _ = writeln!(
            output,
            "Read {} {}: {:?} tokens / {:?} bytes; {} ({}) roles={} tests={}; distance={} via={}; snapshot={} hash={}",
            file.side,
            terminal_text(&file.path.to_string_lossy()),
            file.tokens,
            file.bytes,
            file.selection,
            file.status,
            file.roles.join(","),
            file.test_evidence.join(","),
            file.distance,
            file.via.as_ref().map_or_else(
                || "none".into(),
                |path| terminal_text(&path.to_string_lossy())
            ),
            file.snapshot,
            file.sha256.as_deref().unwrap_or("unknown")
        );
        if let Some(source) = &file.source {
            let _ = writeln!(output, "{}", terminal_text(source));
        }
    }
}
