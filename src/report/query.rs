use super::{
    Format, markdown_code_span, markdown_table_code_span, markdown_table_text, terminal_text,
};
use crate::cli::ConfigOutputFormat;
use crate::model::{CapabilitiesReport, SymbolQueryReport};
use anyhow::{Result, anyhow};
use owo_colors::OwoColorize;

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

pub fn render_capabilities(
    report: &CapabilitiesReport,
    format: ConfigOutputFormat,
    pretty_json: bool,
) -> Result<String> {
    match format {
        ConfigOutputFormat::Json => Ok(super::json_string(report, pretty_json)?),
        ConfigOutputFormat::Table => {
            let mut out = format!("RepoScout {} capabilities\n", report.version);
            out.push_str(&format!(
                "Default: {} via {}\n",
                report.default_operation, report.default_invocation
            ));
            out.push_str(&format!("Commands: {}\n", report.commands.join(", ")));
            out.push_str(&format!("Formats: {}\n", report.output_formats.join(", ")));
            out.push_str(&format!(
                "Symbol query formats: {}\n",
                report.symbol_query_formats.join(", ")
            ));
            out.push_str(&format!(
                "Symbol kinds: {}\n",
                report.symbol_kinds.join(", ")
            ));
            out.push_str(&format!(
                "Profiles: {}\n",
                report.execution_profiles.join(", ")
            ));
            out.push_str(&format!(
                "Daemon profiles: {}\n",
                report.daemon_profiles.join(", ")
            ));
            out.push_str(&format!(
                "First-class languages: {}\n",
                report.first_class_languages.join(", ")
            ));
            out.push_str(&format!(
                "Default health corpus: {}\n",
                report.default_health_languages.join(", ")
            ));
            out.push_str(&format!(
                "Opt-in health formats: {}\n",
                report.optional_health_formats.join(", ")
            ));
            out.push_str(&format!(
                "Health scopes: {}\n",
                report.health_scopes.join(", ")
            ));
            if !report.health_exclude_flag.is_empty() {
                out.push_str(&format!(
                    "Health path exclusions: {}\n",
                    report.health_exclude_flag
                ));
            }
            out.push_str(&format!(
                "Machine interfaces: {}\n",
                report.machine_interfaces.join(", ")
            ));
            out.push_str(&format!(
                "Error formats: {}\n",
                report.error_formats.join(", ")
            ));
            out.push_str(&format!(
                "Change summary: {} (formats: {}; paths <= {}, gaps <= {}, validations <= {})\n",
                report.change_summary.flag,
                report.change_summary.formats.join(", "),
                report.change_summary.max_path_entries,
                report.change_summary.max_gap_entries,
                report.change_summary.max_validations
            ));
            out.push_str(&format!(
                "Work scope: strategy {} (paths <= {}, components <= {})\n",
                report.work_scope.strategy_version,
                report.work_scope.max_path_entries,
                report.work_scope.max_components
            ));
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
        out.push_str(&format!("{}\n", title.bold().cyan()));
    } else {
        out.push_str(&format!("{title}\n"));
    }
    for item in &report.matches {
        let location = format!("{}:{}", item.path.display(), item.line);
        out.push_str(&format!(
            "{}  {:<10} {:<10} {}{}\n",
            terminal_text(&location),
            terminal_text(&item.language),
            terminal_text(&item.kind),
            terminal_text(&item.name),
            if item.exported { "  [exported]" } else { "" }
        ));
        if !item.signature.is_empty() {
            out.push_str(&format!("  {}\n", terminal_text(&item.signature)));
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
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            markdown_table_code_span(&location),
            markdown_table_text(&item.language),
            markdown_table_text(&item.kind),
            markdown_table_code_span(&item.name),
            item.exported
        ));
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
