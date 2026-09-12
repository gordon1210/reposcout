use super::{Format, json_string, markdown_text, terminal_text};
use crate::model::DefinitionPlanReport;
use anyhow::{Result, bail};
use std::fmt::Write as _;

/// Render shared definition-plan facts and optional source without discovery, analysis or source I/O.
pub(crate) fn render(
    report: &DefinitionPlanReport,
    format: Format,
    pretty: bool,
) -> Result<String> {
    let mut output = match format {
        Format::Json => json_string(report, pretty)?,
        Format::Ndjson => json_string(report, false)?,
        Format::Table | Format::Markdown => human(report, format)?,
        Format::Sarif | Format::Dot | Format::Mermaid => {
            bail!("plan supports table, JSON, Markdown, or NDJSON output")
        }
    };
    if !output.ends_with('\n') {
        output.push('\n');
    }
    Ok(output)
}

fn human(report: &DefinitionPlanReport, format: Format) -> Result<String> {
    let mut output = String::new();
    let markdown = format == Format::Markdown;
    let safe = |text: &str| {
        if markdown {
            markdown_text(text)
        } else {
            terminal_text(text)
        }
    };
    let _ = writeln!(
        output,
        "{}Definition plan: {} selected; {} source tokens / {} context budget",
        if markdown { "## " } else { "" },
        report.selected.len() + report.output_omitted,
        report.selected_tokens,
        report.context_budget
    );
    let _ = writeln!(
        output,
        "Candidates: {}; planning omitted: {}; output omitted: {}; files: {} selected / {} captured; discovery incomplete: {}",
        report.candidate_definitions,
        report.omitted_definitions,
        report.output_omitted,
        report.selected_files,
        report.input_files,
        report.discovery_incomplete
    );
    let _ = writeln!(
        output,
        "Seeds: {} unresolved, {} ambiguous, {} unavailable; unavailable files: {}; cost facts omitted: {}; file details omitted: {}",
        report.unresolved_seeds,
        report.ambiguous_seeds,
        report.unavailable_seeds,
        report.unavailable_files,
        report.planning_omitted_definitions,
        report.output_omitted_files
    );
    for item in &report.selected {
        let path = report
            .files
            .iter()
            .find(|file| file.id == item.file)
            .map_or_else(
                || format!("file#{}", item.file),
                |file| file.path.to_string_lossy().into_owned(),
            );
        let _ = writeln!(
            output,
            "- {} {}:{} {} ({} tokens; {})",
            safe(&item.role),
            safe(&path),
            item.declaration_span.start_line,
            safe(&item.name),
            item.tokens,
            safe(&item.reasons.join(", "))
        );
        for gap in &item.environment_gaps {
            let _ = writeln!(output, "  Environment gap: {}", safe(gap));
        }
    }
    for omission in &report.omissions {
        let _ = writeln!(
            output,
            "- Omitted {} {}: {}{}",
            safe(&omission.path.to_string_lossy()),
            safe(omission.name.as_deref().unwrap_or("")),
            safe(&omission.reason),
            if omission.explicit { " (explicit)" } else { "" }
        );
    }
    if report.omitted_details > 0 {
        let _ = writeln!(
            output,
            "Omission details omitted: {}",
            report.omitted_details
        );
    }
    if let Some(source) = &report.source {
        output.push_str(&super::source::render(source, format, false)?);
    }
    Ok(output)
}
