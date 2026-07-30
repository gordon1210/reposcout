use crate::model::{ChangeGapCounts, ChangeSummary, ScanReport};
use anyhow::{Context, Result};
use serde::Serialize;
use std::fmt::Write as _;

use super::{markdown_code_span, markdown_text, terminal_text};

#[derive(Serialize)]
struct Projection<'a> {
    schema_version: &'a str,
    report_kind: &'static str,
    root: &'a std::path::Path,
    target: &'a std::path::Path,
    generated_at: &'a str,
    encoding: &'a str,
    analysis_profile: &'a Option<crate::model::ScanProfile>,
    execution: &'a crate::model::ExecutionMetadata,
    diagnostics: &'a crate::model::ScanDiagnostics,
    change_summary: &'a ChangeSummary,
}

fn projection(report: &ScanReport) -> Result<Projection<'_>> {
    let change_summary = report
        .change_summary
        .as_ref()
        .context("change-summary output requires change-summary analysis")?;
    Ok(Projection {
        schema_version: &report.schema_version,
        report_kind: "change-summary",
        root: &report.root,
        target: &report.target,
        generated_at: &report.generated_at,
        encoding: &report.encoding,
        analysis_profile: &report.analysis_profile,
        execution: &report.execution,
        diagnostics: &report.diagnostics,
        change_summary,
    })
}

pub fn json(report: &ScanReport) -> Result<String> {
    serde_json::to_string(&projection(report)?).context("failed to render change-summary JSON")
}

pub fn ndjson(report: &ScanReport) -> Result<String> {
    let mut rendered = serde_json::to_string(&projection(report)?)
        .context("failed to render change-summary NDJSON")?;
    rendered.push('\n');
    Ok(rendered)
}

pub fn table(report: &ScanReport) -> Result<String> {
    let summary = projection(report)?.change_summary;
    let mut out = String::new();
    writeln!(out, "RepoScout change summary ({})", summary.scope).unwrap();
    writeln!(out, "Confidence: {}", summary.executive.confidence).unwrap();
    writeln!(out, "Changed files: {}", summary.executive.changed_files).unwrap();
    writeln!(
        out,
        "Graph coverage: {}/{} eligible",
        summary.coverage.graph_covered_changed, summary.coverage.graph_eligible_changed
    )
    .unwrap();
    writeln!(
        out,
        "Known dependents: {} direct, {} transitive",
        summary.impact.direct_total, summary.impact.transitive_total
    )
    .unwrap();
    writeln!(out, "Matching tests: {}", summary.tests.total).unwrap();
    writeln!(
        out,
        "Coverage: observed {}, discovery {}, tests {}",
        summary.coverage.observed_scope_confidence,
        summary.coverage.discovery_completeness,
        summary.coverage.test_mapping_confidence
    )
    .unwrap();
    render_gap_counts(&mut out, "Relevant gaps", &summary.coverage.relevant_gaps);
    render_gap_counts(
        &mut out,
        "Outside-scope gaps",
        &summary.coverage.outside_known_scope_gaps,
    );
    if !summary.reading_order.is_empty() {
        writeln!(out, "\nReading order").unwrap();
        for file in &summary.reading_order {
            writeln!(
                out,
                "  {} [{}]",
                terminal_text(&file.path),
                file.roles.join(", ")
            )
            .unwrap();
        }
    }
    if !summary.validations.is_empty() {
        writeln!(out, "\nSuggested validation").unwrap();
        for validation in &summary.validations {
            writeln!(
                out,
                "  {}{} — {}",
                validation.kind,
                validation
                    .target
                    .as_deref()
                    .map(|target| format!(": {}", terminal_text(target)))
                    .unwrap_or_default(),
                validation.reason
            )
            .unwrap();
        }
    }
    append_table_omissions(&mut out, summary);
    Ok(out)
}

pub fn markdown(report: &ScanReport) -> Result<String> {
    let summary = projection(report)?.change_summary;
    let mut out = String::new();
    writeln!(out, "# RepoScout change summary\n").unwrap();
    writeln!(out, "- Scope: {}", markdown_code_span(&summary.scope)).unwrap();
    writeln!(
        out,
        "- Confidence: **{}**",
        markdown_text(&summary.executive.confidence)
    )
    .unwrap();
    writeln!(
        out,
        "- Changed files: {} ({} graph-eligible)",
        summary.executive.changed_files, summary.executive.graph_eligible_changed_files
    )
    .unwrap();
    writeln!(
        out,
        "- Known dependents: {} direct, {} transitive",
        summary.impact.direct_total, summary.impact.transitive_total
    )
    .unwrap();
    writeln!(out, "- Matching tests: {}", summary.tests.total).unwrap();
    writeln!(
        out,
        "- Coverage: observed `{}`, discovery `{}`, tests `{}`",
        markdown_text(&summary.coverage.observed_scope_confidence),
        markdown_text(&summary.coverage.discovery_completeness),
        markdown_text(&summary.coverage.test_mapping_confidence)
    )
    .unwrap();
    if !summary.reading_order.is_empty() {
        writeln!(out, "\n## Reading order\n").unwrap();
        for file in &summary.reading_order {
            writeln!(
                out,
                "- {} — {}",
                markdown_code_span(&file.path),
                file.roles
                    .iter()
                    .map(|role| markdown_text(role))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
            .unwrap();
        }
    }
    if !summary.coverage.gaps.is_empty() {
        writeln!(out, "\n## Coverage gaps\n").unwrap();
        for gap in &summary.coverage.gaps {
            writeln!(
                out,
                "- {} ({}): unreadable={}, parse={}, unresolved={}, config={}",
                markdown_code_span(&gap.path),
                markdown_code_span(&gap.scope),
                gap.unreadable,
                gap.parse_errors,
                gap.unresolved_imports,
                gap.config_errors
            )
            .unwrap();
        }
    }
    if !summary.validations.is_empty() {
        writeln!(out, "\n## Suggested validation\n").unwrap();
        for validation in &summary.validations {
            writeln!(
                out,
                "- **{}**{} — {}",
                markdown_text(&validation.kind),
                validation
                    .target
                    .as_deref()
                    .map(|target| format!(": {}", markdown_code_span(target)))
                    .unwrap_or_default(),
                markdown_text(&validation.reason)
            )
            .unwrap();
        }
    }
    append_markdown_omissions(&mut out, summary);
    Ok(out)
}

fn render_gap_counts(out: &mut String, label: &str, counts: &ChangeGapCounts) {
    writeln!(
        out,
        "{label}: {} unreadable, {} parse, {} unresolved, {} config",
        counts.unreadable_files,
        counts.parse_errors,
        counts.unresolved_imports,
        counts.config_errors
    )
    .unwrap();
}

fn total_omissions(summary: &ChangeSummary) -> usize {
    summary
        .changed
        .omitted
        .saturating_add(summary.reading_order_omitted)
        .saturating_add(summary.impact.omitted)
        .saturating_add(summary.tests.omitted)
        .saturating_add(summary.coverage.gaps_omitted)
        .saturating_add(summary.validations_omitted)
}

fn append_table_omissions(out: &mut String, summary: &ChangeSummary) {
    let omitted = total_omissions(summary);
    if omitted > 0 {
        writeln!(out, "\nBounded details omitted: {omitted}").unwrap();
        writeln!(
            out,
            "  changed {}, reading {}, impact {}, tests {}, gaps {}, validations {}",
            summary.changed.omitted,
            summary.reading_order_omitted,
            summary.impact.omitted,
            summary.tests.omitted,
            summary.coverage.gaps_omitted,
            summary.validations_omitted
        )
        .unwrap();
    }
}

fn append_markdown_omissions(out: &mut String, summary: &ChangeSummary) {
    let omitted = total_omissions(summary);
    if omitted > 0 {
        writeln!(out, "\n## Bounded details\n").unwrap();
        writeln!(out, "- Total omitted entries: {omitted}").unwrap();
        writeln!(out, "- Changed files omitted: {}", summary.changed.omitted).unwrap();
        writeln!(
            out,
            "- Reading-order files omitted: {}",
            summary.reading_order_omitted
        )
        .unwrap();
        writeln!(out, "- Impact files omitted: {}", summary.impact.omitted).unwrap();
        writeln!(out, "- Matching tests omitted: {}", summary.tests.omitted).unwrap();
        writeln!(
            out,
            "- Coverage gaps omitted: {}",
            summary.coverage.gaps_omitted
        )
        .unwrap();
        writeln!(
            out,
            "- Validation entries omitted: {}",
            summary.validations_omitted
        )
        .unwrap();
    }
}
