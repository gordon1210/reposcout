use super::{
    Format, markdown_code_span, markdown_table_code_span, markdown_table_text, terminal_text,
};
use crate::cli::ConfigOutputFormat;
use crate::model::{CapabilitiesReport, SymbolQueryReport};
use anyhow::{Result, anyhow};
use owo_colors::OwoColorize;
use std::fmt::Write as _;

/// Render a symbol-query report.
///
/// # Errors
///
/// Returns an error when the requested format is unsupported or serialization
/// fails.
pub fn render(
    report: &SymbolQueryReport,
    format: Format,
    color: bool,
    pretty_json: bool,
) -> Result<String> {
    match format {
        Format::Table => Ok(table(report, color)),
        Format::Json => Ok(super::json_string(report, pretty_json)?),
        Format::Markdown => Ok(markdown(report)),
        Format::Ndjson => ndjson(report),
        Format::Sarif | Format::Dot | Format::Mermaid => Err(anyhow!(
            "symbol queries do not support the requested output format"
        )),
    }
}

/// Render capability discovery as a human table or JSON.
///
/// # Errors
///
/// Returns an error when JSON serialization fails.
pub fn render_capabilities(
    report: &CapabilitiesReport,
    format: ConfigOutputFormat,
    pretty_json: bool,
) -> Result<String> {
    match format {
        ConfigOutputFormat::Json => Ok(super::json_string(report, pretty_json)?),
        ConfigOutputFormat::Table => {
            let mut out = format!("RepoScout {} capabilities\n", report.version);
            let _ = writeln!(
                out,
                "Default: {} via {}",
                report.default_operation, report.default_invocation
            );
            let _ = writeln!(out, "Commands: {}", report.commands.join(", "));
            let _ = writeln!(out, "Formats: {}", report.output_formats.join(", "));
            let _ = writeln!(
                out,
                "Symbol query formats: {}",
                report.symbol_query_formats.join(", ")
            );
            let _ = writeln!(out, "Symbol kinds: {}", report.symbol_kinds.join(", "));
            let _ = writeln!(out, "Profiles: {}", report.execution_profiles.join(", "));
            let _ = writeln!(
                out,
                "Daemon profiles: {}",
                report.daemon_profiles.join(", ")
            );
            let _ = writeln!(
                out,
                "First-class languages: {}",
                report.first_class_languages.join(", ")
            );
            let _ = writeln!(
                out,
                "Default health corpus: {}",
                report.default_health_languages.join(", ")
            );
            let _ = writeln!(
                out,
                "Opt-in health formats: {}",
                report.optional_health_formats.join(", ")
            );
            let _ = writeln!(out, "Health scopes: {}", report.health_scopes.join(", "));
            if !report.health_exclude_flag.is_empty() {
                let _ = writeln!(
                    out,
                    "Health path exclusions: {}",
                    report.health_exclude_flag
                );
            }
            let _ = writeln!(
                out,
                "Machine interfaces: {}",
                report.machine_interfaces.join(", ")
            );
            let _ = writeln!(out, "Error formats: {}", report.error_formats.join(", "));
            let _ = writeln!(
                out,
                "Change summary: {} (formats: {}; paths <= {}, gaps <= {}, validations <= {})",
                report.change_summary.flag,
                report.change_summary.formats.join(", "),
                report.change_summary.max_path_entries,
                report.change_summary.max_gap_entries,
                report.change_summary.max_validations
            );
            let _ = writeln!(
                out,
                "Work scope: strategy {} (paths <= {}, components <= {})",
                report.work_scope.strategy_version,
                report.work_scope.max_path_entries,
                report.work_scope.max_components
            );
            Ok(out)
        }
    }
}

fn table(report: &SymbolQueryReport, color: bool) -> String {
    let mut out = String::new();
    let title = format!(
        "Symbol query `{}` — {} match{}{}",
        terminal_text(&report.query),
        report.total_matches,
        if report.total_matches == 1 { "" } else { "es" },
        if report.truncated { " (truncated)" } else { "" }
    );
    if color {
        let _ = writeln!(out, "{}", title.bold().cyan());
    } else {
        let _ = writeln!(out, "{title}");
    }
    for item in &report.matches {
        let location = format!("{}:{}", item.path.display(), item.line);
        let _ = writeln!(
            out,
            "{}  {:<10} {:<10} {}{}",
            terminal_text(&location),
            terminal_text(&item.language),
            terminal_text(&item.kind),
            terminal_text(&item.name),
            if item.exported { "  [exported]" } else { "" }
        );
        if !item.signature.is_empty() {
            let _ = writeln!(out, "  {}", terminal_text(&item.signature));
        }
    }
    out
}

fn markdown(report: &SymbolQueryReport) -> String {
    let mut out = format!(
        "# Symbol query {}\n\n{} match{} found{}\n\n",
        markdown_code_span(&report.query),
        report.total_matches,
        if report.total_matches == 1 { "" } else { "es" },
        if report.truncated {
            "; results truncated."
        } else {
            "."
        }
    );
    out.push_str("| Location | Language | Kind | Symbol | Exported |\n");
    out.push_str("|---|---|---|---|---|\n");
    for item in &report.matches {
        let location = format!("{}:{}", item.path.display(), item.line);
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} |",
            markdown_table_code_span(&location),
            markdown_table_text(&item.language),
            markdown_table_text(&item.kind),
            markdown_table_code_span(&item.name),
            item.exported
        );
    }
    out
}

fn ndjson(report: &SymbolQueryReport) -> Result<String> {
    let mut lines = Vec::with_capacity(report.matches.len() + 1);
    lines.push(serde_json::to_string(&serde_json::json!({
        "kind": "symbol_query",
        "schema_version": report.schema_version,
        "root": report.root,
        "target": report.target,
        "generated_at": report.generated_at,
        "query": report.query,
        "match_mode": report.match_mode,
        "filters": {
            "kind": report.kind,
            "language": report.language,
        },
        "total_matches": report.total_matches,
        "returned_matches": report.returned_matches,
        "truncated": report.truncated,
        "first_class_files": report.first_class_files,
        "execution": report.execution,
    }))?);
    for item in &report.matches {
        lines.push(serde_json::to_string(&serde_json::json!({
            "kind": "symbol_match",
            "match": item,
        }))?);
    }
    Ok(lines.join("\n"))
}
