//! Output rendering. Dispatches to table (human), JSON (agent), or markdown.

mod change_summary;
pub mod config;
pub mod explain;
pub mod graph;
pub mod json;
pub mod markdown;
pub mod ndjson;
mod projection;
pub mod query;
pub mod sarif;
pub mod table;
mod work_scope;

use crate::model::ScanReport;
use anyhow::{Context, Result};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Table,
    Json,
    Markdown,
    Sarif,
    Ndjson,
    Dot,
    Mermaid,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RenderOptions {
    pub summary_only: bool,
    pub baseline_ready: bool,
    pub change_summary: bool,
    pub duplication_details: bool,
}

pub fn render(
    report: &ScanReport,
    format: Format,
    color: bool,
    summary_only: bool,
    duplication_details: bool,
) -> Result<String> {
    render_with_options(
        report,
        format,
        color,
        RenderOptions {
            summary_only,
            duplication_details,
            ..RenderOptions::default()
        },
    )
}

pub fn render_with_options(
    report: &ScanReport,
    format: Format,
    color: bool,
    options: RenderOptions,
) -> Result<String> {
    if matches!(
        format,
        Format::Json | Format::Ndjson | Format::Sarif | Format::Dot | Format::Mermaid
    ) {
        validate_machine_paths(report)?;
    }
    if options.change_summary {
        return match format {
            Format::Table => change_summary::table(report),
            Format::Json => change_summary::json(report),
            Format::Markdown => change_summary::markdown(report),
            Format::Ndjson => change_summary::ndjson(report),
            Format::Sarif | Format::Dot | Format::Mermaid => Err(anyhow::anyhow!(
                "change-summary output supports table, JSON, Markdown, or NDJSON"
            )),
        };
    }
    match format {
        Format::Table => Ok(table::render(report, color, options.duplication_details)),
        Format::Json => json::render(report, options.summary_only, options.baseline_ready),
        Format::Markdown => Ok(markdown::render(report, options.duplication_details)),
        Format::Ndjson => ndjson::render(report, options.summary_only),
        Format::Sarif => sarif::render(report),
        Format::Dot => report
            .graph
            .as_ref()
            .map(graph::dot)
            .context("DOT output requires graph analysis"),
        Format::Mermaid => report
            .graph
            .as_ref()
            .map(graph::mermaid)
            .context("Mermaid output requires graph analysis"),
    }
}

fn validate_machine_paths(report: &ScanReport) -> Result<()> {
    let paths = std::iter::once(report.root.as_path())
        .chain(std::iter::once(report.target.as_path()))
        .chain(report.files.iter().map(|file| file.path.as_path()))
        .chain(
            report
                .context
                .iter()
                .flat_map(|context| context.focus.iter().map(|path| path.as_path())),
        )
        .chain(
            report
                .context
                .iter()
                .flat_map(|context| context.changed_files.iter().map(|path| path.as_path())),
        )
        .chain(
            report
                .context
                .iter()
                .flat_map(|context| context.files.iter().map(|file| file.path.as_path())),
        )
        .chain(
            report
                .context
                .iter()
                .flat_map(|context| context.omitted.iter().map(|file| file.path.as_path())),
        )
        .chain(
            report
                .review
                .iter()
                .flat_map(|review| review.changed_files.iter())
                .flat_map(|file| [file.old_path.as_deref(), file.path.as_deref()])
                .flatten(),
        );
    for path in paths {
        path.to_str()
            .with_context(|| format!("report path is not valid UTF-8: {}", path.display()))?;
    }
    Ok(())
}

pub fn render_explain(
    report: &crate::model::ExplainReport,
    format: Format,
    color: bool,
) -> anyhow::Result<String> {
    explain::render(report, format, color)
}

pub fn render_symbol_query(
    report: &crate::model::SymbolQueryReport,
    format: Format,
    color: bool,
) -> anyhow::Result<String> {
    query::render(report, format, color)
}

pub fn render_capabilities(
    report: &crate::model::CapabilitiesReport,
    format: crate::cli::ConfigOutputFormat,
) -> anyhow::Result<String> {
    query::render_capabilities(report, format)
}

/// Format a byte count in a compact human-readable form.
pub(crate) fn human_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

/// Format a large integer with thousands separators.
pub(crate) fn thousands(n: usize) -> String {
    thousands_digits(n.to_string())
}

/// Format a large 64-bit integer with thousands separators.
pub(crate) fn thousands_u64(n: u64) -> String {
    thousands_digits(n.to_string())
}

fn thousands_digits(s: String) -> String {
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    let bytes = s.as_bytes();
    let len = bytes.len();
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
}

/// Render a clone-group similarity: exact clones (1.0) read "exact", near
/// duplicates as a floored percentage (never 100%, since only exact clones are).
pub(crate) fn similarity_label(similarity: f64) -> String {
    if similarity == 1.0 {
        "exact".to_string()
    } else {
        format!("{}%", (similarity * 100.0).floor() as i64)
    }
}

/// Make repository-derived text safe to print to a terminal without allowing
/// control characters to alter terminal state or the report layout.
pub(crate) fn terminal_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            ch if ch.is_control() => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{{{:x}}}", ch as u32);
            }
            ch => out.push(ch),
        }
    }
    out
}

/// Escape dynamic text used in Markdown table cells.
pub(crate) fn markdown_table_text(value: &str) -> String {
    markdown_text(value).replace('|', "\\|")
}

/// Escape repository-derived prose so it cannot introduce Markdown structure.
pub(crate) fn markdown_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in terminal_text(value).chars() {
        if matches!(
            ch,
            '\\' | '`' | '*' | '_' | '{' | '}' | '[' | ']' | '<' | '>' | '#'
        ) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Render an arbitrary string as a CommonMark code span, selecting a delimiter
/// longer than every backtick run in the value.
pub(crate) fn markdown_code_span(value: &str) -> String {
    let value = terminal_text(value);
    let longest = value.split(|ch| ch != '`').map(str::len).max().unwrap_or(0);
    let delimiter = "`".repeat(longest + 1);
    if value.starts_with(['`', ' ']) || value.ends_with(['`', ' ']) {
        format!("{delimiter} {value} {delimiter}")
    } else {
        format!("{delimiter}{value}{delimiter}")
    }
}

pub(crate) fn markdown_table_code_span(value: &str) -> String {
    markdown_code_span(&value.replace('|', "\\|"))
}

pub(crate) fn sarif_uri(path: &Path) -> Result<String> {
    let path = path
        .to_str()
        .with_context(|| format!("SARIF path is not valid UTF-8: {}", path.display()))?;
    sarif_uri_text(path)
}

pub(crate) fn sarif_uri_text(path: &str) -> Result<String> {
    let mut out = String::with_capacity(path.len());
    for byte in path.replace('\\', "/").bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
            out.push(byte as char);
        } else {
            use std::fmt::Write as _;
            write!(out, "%{byte:02X}")?;
        }
    }
    Ok(out)
}

/// Summarize a duplicate block's locations for a table cell: show the first
/// couple and note how many more copies exist.
pub(crate) fn dup_locations(locations: &[String], copies: usize) -> String {
    const SHOWN: usize = 2;
    if locations.is_empty() {
        return String::new();
    }
    let shown = locations.iter().take(SHOWN).cloned().collect::<Vec<_>>();
    let mut out = shown.join(", ");
    let remaining = copies.saturating_sub(shown.len());
    if remaining > 0 {
        out.push_str(&format!(" (+{remaining} more)"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_text_exposes_controls() {
        assert_eq!(terminal_text("a\n\u{1b}\tb"), "a\\n\\u{1b}\\tb");
    }

    #[test]
    fn code_span_handles_backticks() {
        assert_eq!(markdown_code_span("a`b"), "``a`b``");
        assert_eq!(markdown_code_span("`a`"), "`` `a` ``");
    }

    #[test]
    fn sarif_uri_percent_encodes_reserved_and_unicode_bytes() {
        assert_eq!(
            sarif_uri_text("src/a b#c?.rs").unwrap(),
            "src/a%20b%23c%3F.rs"
        );
        assert_eq!(sarif_uri_text("café.rs").unwrap(), "caf%C3%A9.rs");
    }

    #[test]
    fn only_one_is_labeled_exact() {
        assert_eq!(similarity_label(1.0), "exact");
        assert_eq!(similarity_label(0.9999), "99%");
    }
}
