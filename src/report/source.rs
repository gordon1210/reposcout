use super::{Format, json_string, markdown_text, terminal_text};
use crate::model::{
    SourceQueryCapability, SourceQueryDefinition, SourceQueryReport, SourceQueryResult, SourceSpan,
};
use anyhow::{Result, bail};
use serde::Serialize;
use std::fmt::Write as _;

/// Render shared source-query facts without reading source or running analysis.
pub(crate) fn render(report: &SourceQueryReport, format: Format, pretty: bool) -> Result<String> {
    let mut output = match format {
        Format::Json => json_string(report, pretty)?,
        Format::Ndjson => json_string(report, false)?,
        Format::Table | Format::Markdown => human(report, format == Format::Markdown)?,
        Format::Sarif | Format::Dot | Format::Mermaid => {
            bail!("read supports table, JSON, Markdown, or NDJSON output")
        }
    };
    if !output.ends_with('\n') {
        output.push('\n');
    }
    Ok(output)
}

fn human(report: &SourceQueryReport, markdown: bool) -> Result<String> {
    let mut output = String::new();
    writeln!(
        output,
        "{}Source query · {} · schema {}",
        if markdown { "# " } else { "" },
        report.mode,
        report.schema_version
    )?;
    writeln!(
        output,
        "Budget: {} tokens ({}), {} bytes; targets: {}, omitted: {}",
        report.token_budget,
        report.encoding,
        report.byte_budget,
        report.requested_targets,
        report.omitted_targets
    )?;
    if let Some(root) = &report.root {
        writeln!(
            output,
            "Root: {}",
            safe_text(&root.to_string_lossy(), markdown)
        )?;
    } else if report.root_omitted {
        writeln!(output, "Root: omitted")?;
    }
    for file in &report.files {
        writeln!(
            output,
            "\nFile {}: {}",
            file.id,
            safe_text(&file.path.to_string_lossy(), markdown)
        )?;
        if let Some(language) = &file.language {
            writeln!(output, "Language: {}", safe_text(language, markdown))?;
        }
        if let Some(hash) = &file.sha256 {
            writeln!(output, "SHA-256: {hash}")?;
        }
        if let Some(extraction) = &file.extraction {
            writeln!(output, "Extraction: {}", label(extraction)?)?;
        }
        if let (Some(declarations), Some(sources)) = (file.declarations, file.source_definitions) {
            writeln!(
                output,
                "Declarations: {declarations}; available source definitions: {sources}"
            )?;
        }
    }
    for result in &report.results {
        render_result(&mut output, result, markdown)?;
    }
    for source in &report.sources {
        writeln!(
            output,
            "\nSource {} · file {} · {}",
            source.id,
            source.file,
            span_text(source.span)
        )?;
        let content = source_text(&source.content);
        if markdown {
            let longest = content
                .split(|ch| ch != '`')
                .map(str::len)
                .max()
                .unwrap_or(0);
            let fence = "`".repeat((longest + 1).max(3));
            writeln!(output, "\n{fence}\n{content}")?;
            writeln!(output, "{fence}")?;
        } else {
            writeln!(output, "{content}")?;
        }
    }
    Ok(output)
}

fn render_result(output: &mut String, result: &SourceQueryResult, markdown: bool) -> Result<()> {
    write!(
        output,
        "\nTarget {}: {}",
        result.target,
        label(&result.status)?
    )?;
    if let Some(file) = result.file {
        write!(output, " · file {file}")?;
    }
    if let Some(reason) = &result.selection {
        write!(output, " · {}", safe_text(reason, markdown))?;
    }
    if let Some(source) = result.source {
        write!(output, " · source {source}")?;
    }
    output.push('\n');
    if let Some(definition) = &result.definition {
        render_definition(output, definition, markdown)?;
    }
    if result.total_candidates > 0 || result.omitted_candidates > 0 {
        writeln!(
            output,
            "Candidates: {}; omitted: {}",
            result.total_candidates, result.omitted_candidates
        )?;
    }
    for candidate in &result.candidates {
        render_definition(output, candidate, markdown)?;
    }
    Ok(())
}

fn render_definition(
    output: &mut String,
    definition: &SourceQueryDefinition,
    markdown: bool,
) -> Result<()> {
    writeln!(
        output,
        "  {} {} · declaration {}",
        safe_text(&definition.kind, markdown),
        safe_text(&definition.name, markdown),
        span_text(definition.declaration_span)
    )?;
    if let Some(span) = definition.source_span {
        writeln!(output, "  Retrieval: {}", span_text(span))?;
    } else {
        writeln!(output, "  Retrieval: unavailable")?;
    }
    if let Some(signature) = &definition.signature {
        writeln!(output, "  Signature: {}", safe_text(signature, markdown))?;
    }
    Ok(())
}

fn span_text(span: SourceSpan) -> String {
    format!(
        "lines {}–{}, bytes {}..{}",
        span.start_line, span.end_line, span.start_byte, span.end_byte
    )
}

fn safe_text(value: &str, markdown: bool) -> String {
    if markdown {
        markdown_text(value)
    } else {
        terminal_text(value)
    }
}

fn label(value: &impl Serialize) -> Result<String> {
    Ok(json_string(value, false)?.trim_matches('"').to_string())
}

fn source_text(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_control() && !matches!(ch, '\n' | '\t') {
            let _ = write!(output, "\\u{{{:x}}}", u32::from(ch));
        } else {
            output.push(ch);
        }
    }
    output
}

pub(super) fn capability_table(source: &SourceQueryCapability) -> String {
    let mut output = format!(
        "Source query: {} ({}, {})\n",
        source.command, source.snapshot, source.hash_algorithm
    );
    let _ = writeln!(output, "  Selectors: {}", source.selectors.join("; "));
    let _ = writeln!(output, "  Formats: {}", source.formats.join(", "));
    let _ = writeln!(
        output,
        "  Output: {}–{} tokens (default {}), {}–{} bytes (default {})",
        source.min_tokens,
        source.max_tokens,
        source.default_tokens,
        source.min_bytes,
        source.max_bytes,
        source.default_bytes
    );
    let _ = writeln!(
        output,
        "  Input: <= {} targets; <= {} bytes/file, <= {} bytes total",
        source.max_targets, source.max_input_file_bytes, source.max_input_total_bytes
    );
    let _ = writeln!(
        output,
        "  Candidates: <= {}; outline declarations: <= {}",
        source.max_candidates, source.max_outline_declarations
    );
    for language in &source.languages {
        let _ = writeln!(
            output,
            "  {}: {}",
            language.language,
            language.kinds.join(", ")
        );
    }
    output
}
