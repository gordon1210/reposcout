use super::{Format, json_string, terminal_text};
use crate::model::ConsumersQueryReport;
use anyhow::{Result, bail};
use std::fmt::Write as _;

pub(crate) fn render(
    report: &ConsumersQueryReport,
    format: Format,
    pretty: bool,
) -> Result<String> {
    let mut output = match format {
        Format::Json => json_string(report, pretty)?,
        Format::Ndjson => json_string(report, false)?,
        Format::Table | Format::Markdown => {
            let mut output = format!(
                "Consumers: {} of {} matches; depth omitted: {}; path omitted: {}; limit omitted: {}; budget omitted: {}\n",
                report.returned_matches,
                report.total_matches,
                report.depth_omitted,
                report.path_omitted,
                report.limit_omitted,
                report.budget_omitted
            );
            let _ = writeln!(
                output,
                "Coverage: {} available / {} files; {} unresolved; {} unsupported; {} extraction omissions",
                report.coverage.files_available,
                report.coverage.files_total,
                report.coverage.resolution.unresolved,
                report.coverage.files_unsupported,
                report.coverage.relations_omitted
            );
            for hit in &report.hits {
                let _ = writeln!(
                    output,
                    "{}:{} {} depth={} sha256={}",
                    terminal_text(&hit.symbol.path),
                    terminal_text(&hit.symbol.name),
                    terminal_text(&hit.symbol.name),
                    hit.depth,
                    hit.symbol.source_hash
                );
                for edge in &hit.evidence {
                    let _ = writeln!(
                        output,
                        "  {:?}: {}:{} -> {}:{} [{}]",
                        edge.kind,
                        terminal_text(&edge.source.path),
                        edge.site.start_line,
                        terminal_text(&edge.target.path),
                        edge.target.declaration_span.start_line,
                        terminal_text(&edge.resolver)
                    );
                }
                let _ = writeln!(
                    output,
                    "  read: --symbol {} {} --expect-hash {} {}",
                    terminal_text(&hit.read.path.to_string_lossy()),
                    terminal_text(&hit.symbol.name),
                    terminal_text(&hit.read.path.to_string_lossy()),
                    hit.read.expected_hash
                );
            }
            output
        }
        Format::Sarif | Format::Dot | Format::Mermaid => {
            bail!("consumers supports table, JSON, Markdown, or NDJSON output")
        }
    };
    if !output.ends_with('\n') {
        output.push('\n');
    }
    Ok(output)
}
