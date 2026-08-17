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
use serde::Serialize;
use std::fmt::Write as _;
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Projection {
    #[default]
    Full,
    Summary,
    BaselineReady,
    ChangeSummary,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RenderOptions {
    pub projection: Projection,
    pub duplication_details: bool,
    pub pretty_json: bool,
    /// Hide configuration guidance when project configuration was deliberately
    /// disabled by the caller or execution profile.
    pub suppress_config_guidance: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ConfigGuidance {
    NoConfiguration,
    GlobalOnly,
}

pub(super) fn config_guidance(report: &ScanReport, suppressed: bool) -> Option<ConfigGuidance> {
    if suppressed || report.execution.project_config.is_some() {
        return None;
    }
    if report.execution.global_config.is_some() {
        return Some(ConfigGuidance::GlobalOnly);
    }
    (report.execution.config_mode == "defaults").then_some(ConfigGuidance::NoConfiguration)
}

/// Render a scan report in the selected human or machine format.
///
/// # Errors
///
/// Returns an error when the selected projection and format are incompatible,
/// report paths cannot be represented safely, or serialization fails.
pub fn render(
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
    if options.projection == Projection::ChangeSummary {
        return match format {
            Format::Table => change_summary::table(report),
            Format::Json => change_summary::json(report, options.pretty_json),
            Format::Markdown => change_summary::markdown(report),
            Format::Ndjson => change_summary::ndjson(report),
            Format::Sarif | Format::Dot | Format::Mermaid => Err(anyhow::anyhow!(
                "change-summary output supports table, JSON, Markdown, or NDJSON"
            )),
        };
    }
    match format {
        Format::Table => Ok(table::render(report, color, options)),
        Format::Json => json::render(report, options.projection, options.pretty_json),
        Format::Markdown => Ok(markdown::render(report, options)),
        Format::Ndjson => ndjson::render(report, options.projection == Projection::Summary),
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
                .flat_map(|context| context.focus.iter().map(std::path::PathBuf::as_path)),
        )
        .chain(report.context.iter().flat_map(|context| {
            context
                .changed_files
                .iter()
                .map(std::path::PathBuf::as_path)
        }))
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

/// Render a focused file explanation.
///
/// # Errors
///
/// Returns an error when the format is unsupported or serialization fails.
pub fn render_explain(
    report: &crate::model::ExplainReport,
    format: Format,
    color: bool,
    pretty_json: bool,
) -> Result<String> {
    explain::render(report, format, color, pretty_json)
}

/// Render a symbol-query report.
///
/// # Errors
///
/// Returns an error when the format is unsupported or serialization fails.
pub fn render_symbol_query(
    report: &crate::model::SymbolQueryReport,
    format: Format,
    color: bool,
    pretty_json: bool,
) -> Result<String> {
    query::render(report, format, color, pretty_json)
}

/// Render machine-discoverable `RepoScout` capabilities.
///
/// # Errors
///
/// Returns an error when serialization fails.
pub fn render_capabilities(
    report: &crate::model::CapabilitiesReport,
    format: crate::cli::ConfigOutputFormat,
    pretty_json: bool,
) -> Result<String> {
    query::render_capabilities(report, format, pretty_json)
}

pub(crate) fn json_string<T: Serialize>(value: &T, pretty: bool) -> serde_json::Result<String> {
    if pretty {
        serde_json::to_string_pretty(value)
    } else {
        serde_json::to_string(value)
    }
}

/// Format a byte count in a compact human-readable form.
pub(crate) fn human_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut v = crate::numeric::u64_to_f64(bytes);
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
    thousands_digits(&n.to_string())
}

/// Format a large 64-bit integer with thousands separators.
pub(crate) fn thousands_u64(n: u64) -> String {
    thousands_digits(&n.to_string())
}

fn thousands_digits(s: &str) -> String {
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
    if (similarity - 1.0).abs() < f64::EPSILON {
        "exact".to_string()
    } else {
        format!("{:.0}%", (similarity * 100.0).floor())
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

/// Render an arbitrary string as a `CommonMark` code span, selecting a delimiter
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
        let _ = write!(out, " (+{remaining} more)");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        Assessment, ComplexitySummary, DuplicateBlock, Duplication, DuplicationSummary,
        FindingCatalog, FindingProfile, LanguageDuplication, LanguageStat, ProductionDuplication,
        RiskEntry, ScanDiagnostics, ScanProfile, ScanReport, Summary, TestFramework, TestPresence,
        WorkScope,
    };
    use std::path::PathBuf;

    fn report_with_summary(summary: Summary) -> ScanReport {
        ScanReport {
            schema_version: crate::model::SCHEMA_VERSION.to_string(),
            root: PathBuf::from("/repo"),
            target: PathBuf::from("/repo"),
            generated_at: "2026-01-01T00:00:00Z".to_string(),
            encoding: "o200k_base".to_string(),
            analysis_profile: None,
            execution: crate::model::ExecutionMetadata::default(),
            finding_catalog: FindingCatalog::default(),
            summary,
            work_scope: None,
            files: Vec::new(),
            duplicates: Duplication::default(),
            directories: Vec::new(),
            baseline: None,
            graph: None,
            context: None,
            diagnostics: ScanDiagnostics::default(),
            impact: None,
            change_summary: None,
            review: None,
        }
    }

    fn human_renderings(report: &ScanReport) -> [String; 2] {
        [
            render(report, Format::Table, false, RenderOptions::default()).unwrap(),
            render(report, Format::Markdown, false, RenderOptions::default()).unwrap(),
        ]
    }

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

    #[test]
    fn human_reports_fall_back_only_for_legacy_duplicate_projections() {
        let duplicate = DuplicateBlock {
            lines: 4,
            tokens: 12,
            similarity: 1.0,
            copies: 2,
            duplicated_lines: 4,
            locations: vec!["legacy/a.rs:1-4".to_string(), "legacy/b.rs:1-4".to_string()],
        };
        let mut legacy = report_with_summary(Summary {
            top_duplicates: vec![duplicate.clone()],
            ..Summary::default()
        });

        for rendered in human_renderings(&legacy) {
            assert!(rendered.contains("Top duplicates"));
            assert!(rendered.contains("legacy/a.rs:1-4"));
        }

        legacy.summary.assessment.production_duplication = Some(ProductionDuplication {
            corpus: "production-source".to_string(),
            complete: true,
            ..ProductionDuplication::default()
        });
        for rendered in human_renderings(&legacy) {
            assert!(!rendered.contains("Top duplicates"));
            assert!(!rendered.contains("legacy/a.rs:1-4"));
        }
    }

    #[test]
    fn human_reports_label_the_risk_algorithm_recorded_by_the_report() {
        let mut report = report_with_summary(Summary {
            top_risks: vec![RiskEntry {
                path: "legacy.rs".to_string(),
                score: 0.75,
                ..RiskEntry::default()
            }],
            ..Summary::default()
        });
        report.analysis_profile = Some(ScanProfile {
            findings: Some(FindingProfile {
                risk_algorithm_version: 4,
                ..FindingProfile::default()
            }),
            ..ScanProfile::default()
        });

        for rendered in human_renderings(&report) {
            assert!(rendered.contains("Top risks · algorithm 4"));
            assert!(!rendered.contains("Top risks · algorithm 5"));
        }
    }

    #[test]
    fn terminal_report_groups_composition_health_and_ranked_details() {
        let mut report = report_with_summary(Summary {
            languages: vec![LanguageStat {
                name: "Rust".to_string(),
                source: true,
                files: 1,
                ..LanguageStat::default()
            }],
            complexity: ComplexitySummary {
                cyclomatic_total: 1,
                functions: 1,
                cyclomatic_threshold: 20,
                mi_avg: 80.0,
                mi_min: 80.0,
                ..ComplexitySummary::default()
            },
            markers: [("TODO".to_string(), 1)].into_iter().collect(),
            top_risks: vec![RiskEntry {
                path: "src/lib.rs".to_string(),
                algorithm_version: 5,
                score: 0.75,
                ..RiskEntry::default()
            }],
            assessment: Assessment {
                fits_context_known: true,
                fits_context: true,
                cleanup_worth: "low".to_string(),
                cleanup_worth_complete: true,
                ..Assessment::default()
            },
            test_presence: Some(TestPresence {
                frameworks: vec![TestFramework {
                    name: "cargo-test".to_string(),
                    evidence: "Cargo.toml".to_string(),
                }],
                ..TestPresence::default()
            }),
            ..Summary::default()
        });
        report.work_scope = Some(WorkScope::default());
        report.execution.config_mode = "defaults".to_string();

        let rendered = render(&report, Format::Table, false, RenderOptions::default()).unwrap();
        let positions = [
            "Source languages",
            "Work scope",
            "Markers",
            "Configured test discovery",
            "Duplication",
            "Function complexity",
            "Top risks",
            "Assessment",
            "Overview",
            "Tip: no config found",
        ]
        .map(|heading| {
            rendered
                .find(heading)
                .unwrap_or_else(|| panic!("missing terminal heading: {heading}"))
        });

        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn terminal_report_uses_semantic_color_without_changing_plain_text() {
        let report = report_with_summary(Summary {
            duplication: DuplicationSummary {
                exact_groups: 1,
                duplicated_lines: 20,
                duplicated_pct: 20.0,
                analyzed_lines: 100,
                ..DuplicationSummary::default()
            },
            markers: [("TODO".to_string(), 1), ("BUG".to_string(), 1)]
                .into_iter()
                .collect(),
            top_risks: vec![RiskEntry {
                path: "src/high-risk.rs".to_string(),
                score: 0.75,
                ..RiskEntry::default()
            }],
            assessment: Assessment {
                fits_context_known: true,
                fits_context: true,
                token_budget: 200_000,
                cleanup_worth: "high".to_string(),
                cleanup_worth_complete: true,
                ..Assessment::default()
            },
            ..Summary::default()
        });

        let colored = render(&report, Format::Table, true, RenderOptions::default()).unwrap();
        let plain = render(&report, Format::Table, false, RenderOptions::default()).unwrap();

        assert!(colored.contains("\u{1b}[32mfits"));
        assert!(colored.contains("\u{1b}[31mhigh"));
        assert!(colored.contains("\u{1b}[33m20.0%"));
        assert!(colored.contains("\u{1b}[31mBUG 1"));
        assert!(colored.contains("\u{1b}[34mTODO 1"));
        assert!(!plain.contains('\u{1b}'));
        assert!(plain.contains("Context          fits (budget 200,000)"));
        assert!(plain.contains("Cleanup worth    high"));
        assert!(plain.find("BUG 1").unwrap() < plain.find("TODO 1").unwrap());
    }

    #[test]
    fn human_reports_distinguish_missing_and_global_only_configuration() {
        let mut report = report_with_summary(Summary::default());
        report.execution.config_mode = "defaults".to_string();

        let table = render(&report, Format::Table, false, RenderOptions::default()).unwrap();
        assert!(
            table.contains(
                "Tip: no config found — global/project settings can sharpen this report."
            )
        );
        assert!(!table.contains("Configuration tip"));
        assert!(!table.contains("Inspect effective settings"));

        let markdown = render(&report, Format::Markdown, false, RenderOptions::default()).unwrap();
        assert!(markdown.contains("Configuration tip"));
        assert!(markdown.contains("No RepoScout configuration was found"));
        assert!(markdown.contains("global config can establish reusable defaults"));
        assert!(markdown.contains("project `reposcout.toml`"));

        report.execution.config_mode = "user".to_string();
        report.execution.global_config = Some(PathBuf::from("/config/reposcout.toml"));
        let table = render(&report, Format::Table, false, RenderOptions::default()).unwrap();
        assert!(table.contains(
            "Tip: global config active — a project config can sharpen this report further."
        ));
        assert!(!table.contains("Tip: no config found"));

        let markdown = render(&report, Format::Markdown, false, RenderOptions::default()).unwrap();
        assert!(markdown.contains("global RepoScout configuration is active"));
        assert!(!markdown.contains("No RepoScout configuration was found"));
    }

    #[test]
    fn human_reports_omit_configuration_guidance_when_loaded_or_suppressed() {
        let mut report = report_with_summary(Summary::default());
        report.execution.config_mode = "project".to_string();
        report.execution.project_config = Some(PathBuf::from("/repo/reposcout.toml"));
        for rendered in human_renderings(&report) {
            assert!(!rendered.contains("Configuration tip"));
            assert!(!rendered.contains("Tip: no config found"));
            assert!(!rendered.contains("Tip: global config active"));
        }

        report.execution.config_mode = "user".to_string();
        report.execution.global_config = Some(PathBuf::from("/config/reposcout.toml"));
        report.execution.project_config = None;
        let options = RenderOptions {
            suppress_config_guidance: true,
            ..RenderOptions::default()
        };
        for format in [Format::Table, Format::Markdown] {
            let rendered = render(&report, format, false, options).unwrap();
            assert!(!rendered.contains("Configuration tip"));
            assert!(!rendered.contains("Tip: global config active"));
        }
    }

    #[test]
    fn human_duplication_breakdown_omits_languages_without_duplication() {
        let mut report = report_with_summary(Summary {
            duplication: DuplicationSummary {
                exact_groups: 1,
                by_language: vec![
                    LanguageDuplication {
                        name: "Rust".to_string(),
                        exact_groups: 1,
                        duplicated_lines: 4,
                        duplicated_tokens: 12,
                        ..LanguageDuplication::default()
                    },
                    LanguageDuplication {
                        name: "ZeroLang".to_string(),
                        files: 2,
                        lines: 40,
                        tokens: 100,
                        ..LanguageDuplication::default()
                    },
                ],
                ..DuplicationSummary::default()
            },
            ..Summary::default()
        });

        for rendered in human_renderings(&report) {
            assert!(rendered.contains("Duplication by language"));
            assert!(rendered.contains("Rust"));
            assert!(!rendered.contains("ZeroLang"));
        }

        report.summary.duplication.by_language = vec![LanguageDuplication {
            name: "ZeroLang".to_string(),
            files: 2,
            lines: 40,
            tokens: 100,
            ..LanguageDuplication::default()
        }];
        for rendered in human_renderings(&report) {
            assert!(!rendered.contains("Duplication by language"));
            assert!(!rendered.contains("ZeroLang"));
        }
    }
}
