use super::{Format, json_string, markdown_code_span, markdown_text, terminal_text};
use crate::model::{
    FindMatchMode, FindQueryHit, FindQueryReport, FindReadSelector, SourceRevision,
};
use anyhow::{Result, bail};
use std::fmt::Write as _;

/// Render shared lexical-search facts without discovery, analysis or source I/O.
pub(crate) fn render(report: &FindQueryReport, format: Format, pretty: bool) -> Result<String> {
    let mut output = match format {
        Format::Json => json_string(report, pretty)?,
        Format::Ndjson => json_string(report, false)?,
        Format::Table => table(report),
        Format::Markdown => markdown(report),
        Format::Sarif | Format::Dot | Format::Mermaid => {
            bail!("find supports table, JSON, Markdown, or NDJSON output")
        }
    };
    if !output.ends_with('\n') {
        output.push('\n');
    }
    Ok(output)
}

fn table(report: &FindQueryReport) -> String {
    let mut output = String::new();
    let _ = writeln!(
        output,
        "Lexical find `{}` · {} of {} matches",
        terminal_text(&report.query),
        report.returned_matches,
        report.total_matches
    );
    render_summary(&mut output, report, false);
    for hit in &report.hits {
        render_hit(&mut output, hit, false);
    }
    output
}

fn markdown(report: &FindQueryReport) -> String {
    let mut output = String::new();
    let _ = writeln!(
        output,
        "# Lexical find {}\n\n{} of {} matches returned.",
        markdown_code_span(&report.query),
        report.returned_matches,
        report.total_matches
    );
    render_summary(&mut output, report, true);
    for hit in &report.hits {
        render_hit(&mut output, hit, true);
    }
    output
}

fn render_summary(output: &mut String, report: &FindQueryReport, markdown: bool) {
    let mode = match report.match_mode {
        FindMatchMode::All => "all",
        FindMatchMode::Any => "any",
    };
    let prefix = if markdown { "- " } else { "" };
    let _ = writeln!(
        output,
        "{prefix}Terms: {} ({mode}); limit omitted: {}; budget omitted: {}",
        report
            .query_terms
            .iter()
            .map(|term| safe(term, markdown))
            .collect::<Vec<_>>()
            .join(", "),
        report.limit_omitted,
        report.budget_omitted
    );
    let _ = writeln!(
        output,
        "{prefix}Coverage: {} files, {} inspected, {} unsupported, {} unavailable, {} parse-error, {} field-truncated; {} of {} definitions inspected, {} omitted",
        report.coverage.files_total,
        report.coverage.files_inspected,
        report.coverage.unsupported_files,
        report.coverage.unavailable_files,
        report.coverage.parse_error_files,
        report.coverage.field_truncated_files,
        report.coverage.definitions_inspected,
        report.coverage.definitions_total,
        report.coverage.definitions_omitted
    );
    if !report.coverage.truncated_fields.is_empty() {
        let fields = report
            .coverage
            .truncated_fields
            .iter()
            .map(|field| format!("{}={}", field_label(field.field), field.files))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(output, "{prefix}Truncated fields: {fields}");
    }
    if let Some(root) = &report.root {
        let _ = writeln!(
            output,
            "{prefix}Root: {}",
            safe(&root.to_string_lossy(), markdown)
        );
    } else if report.root_omitted {
        let _ = writeln!(output, "{prefix}Root: omitted");
    }
    if report.kind_filter.is_some() || report.language_filter.is_some() {
        let _ = writeln!(
            output,
            "{prefix}Filters: kind={}, language={}",
            report
                .kind_filter
                .as_deref()
                .map_or("*".to_string(), |value| safe(value, markdown)),
            report
                .language_filter
                .as_deref()
                .map_or("*".to_string(), |value| safe(value, markdown))
        );
    }
}

fn render_hit(output: &mut String, hit: &FindQueryHit, markdown: bool) {
    let prefix = if markdown { "## " } else { "" };
    let _ = writeln!(
        output,
        "\n{prefix}{}. {}:{} · score {} · {} {} · {}",
        hit.rank,
        safe(&hit.path.to_string_lossy(), markdown),
        hit.declaration_span.start_line,
        hit.score,
        safe(&hit.language, markdown),
        safe(&hit.kind, markdown),
        safe(&hit.name, markdown)
    );
    let _ = writeln!(output, "Reason: {}", safe(&hit.reason, markdown));
    if let Some(signature) = &hit.signature {
        let _ = writeln!(output, "Signature: {}", safe(signature, markdown));
    }
    let selector = match &hit.read.selector {
        FindReadSelector::Symbol(symbol) => format!("symbol {}", safe(symbol, markdown)),
    };
    let _ = writeln!(
        output,
        "Read: {}; file {}; snapshot {}; expected SHA-256 {}",
        selector,
        safe(&hit.read.path.to_string_lossy(), markdown),
        snapshot(&hit.read.snapshot),
        hit.read.expected_hash
    );
}

fn safe(value: &str, markdown: bool) -> String {
    if markdown {
        markdown_text(value)
    } else {
        terminal_text(value)
    }
}

fn snapshot(revision: &SourceRevision) -> &str {
    match revision {
        SourceRevision::Worktree => "worktree",
        SourceRevision::Index => "index",
        SourceRevision::Tree(_) => "tree",
        SourceRevision::Empty => "empty",
    }
}

fn field_label(field: crate::model::LexicalField) -> &'static str {
    match field {
        crate::model::LexicalField::Name => "name",
        crate::model::LexicalField::Path => "path",
        crate::model::LexicalField::Signature => "signature",
        crate::model::LexicalField::Comment => "comment",
        crate::model::LexicalField::Code => "code",
    }
}
