//! Focused renderers for `reposcout explain FILE`.

use crate::model::{ExplainReport, FindingRecord};
use crate::report::{
    Format, human_bytes, markdown_code_span, markdown_text, terminal_text, thousands,
};
use anyhow::{Result, anyhow};
use owo_colors::OwoColorize;
use serde_json::Value;
use std::fmt::Write as _;

/// Render a focused file explanation.
///
/// # Errors
///
/// Returns an error when the requested format is unsupported or serialization
/// fails.
pub fn render(
    report: &ExplainReport,
    format: Format,
    color: bool,
    pretty_json: bool,
) -> Result<String> {
    match format {
        Format::Table => Ok(table(report, color)),
        Format::Json => Ok(super::json_string(report, pretty_json)?),
        Format::Markdown => Ok(markdown(report)),
        Format::Ndjson => {
            let mut value = serde_json::to_value(report)?;
            if let Value::Object(object) = &mut value {
                object.insert("kind".to_string(), Value::String("explain".to_string()));
            }
            Ok(serde_json::to_string(&value)?)
        }
        Format::Sarif => Err(anyhow!("explain does not support SARIF output")),
        Format::Dot | Format::Mermaid => Err(anyhow!("explain does not support graph-only output")),
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "the focused table renderer is a linear user-facing section contract with no branching business logic"
)]
fn table(report: &ExplainReport, color: bool) -> String {
    let mut out = String::new();
    let title = format!(
        "reposcout explain  {}",
        terminal_text(&report.path.display().to_string())
    );
    let title = if color {
        format!("{}", title.cyan().bold())
    } else {
        title
    };
    let _ = writeln!(out, "{title}");
    let _ = writeln!(out);
    section(&mut out, "Discovery", color);
    kv(&mut out, "Status", &report.discovery.status);
    kv(&mut out, "Reason", &report.discovery.reason);
    if let Some(rule) = &report.discovery.rule {
        kv(
            &mut out,
            "Rule",
            &format!("{} `{}`", rule.kind, rule.pattern),
        );
        kv(&mut out, "Source", &rule.source);
    }
    let _ = writeln!(out);

    section(&mut out, "Repository context", color);
    kv(&mut out, "Files", &thousands(report.repository.files));
    kv(&mut out, "Tokens", &thousands(report.repository.tokens));
    kv(
        &mut out,
        "Source files",
        &thousands(report.repository.source_files),
    );
    if let Some(test_files) = report.repository.test_files {
        kv(&mut out, "Configured test files", &thousands(test_files));
    }
    let _ = writeln!(out);

    if let Some(file) = &report.file {
        section(&mut out, "File", color);
        kv(&mut out, "Language", &file.language);
        kv(&mut out, "Size", &human_bytes(file.bytes));
        kv(&mut out, "Tokens", &thousands(file.tokens));
        kv(
            &mut out,
            "Lines",
            &format!(
                "{} LOC · {} SLOC",
                thousands(file.loc),
                thousands(file.sloc)
            ),
        );
        if let Some(hint) = &file.skip_hint {
            kv(&mut out, "Skip hint", hint);
        }
        let _ = writeln!(out);
    }

    if let Some(risk) = &report.risk {
        section(&mut out, "Risk", color);
        kv(
            &mut out,
            "Algorithm",
            &format!("version {}", risk.algorithm_version),
        );
        kv(&mut out, "Score", &format!("{:.2}", risk.score));
        kv(
            &mut out,
            "Factors",
            &format!(
                "size {:.2} · complexity {:.2} · churn {:.2}",
                risk.size_factor, risk.complexity_factor, risk.churn_factor
            ),
        );
        if !risk.reasons.is_empty() {
            kv(
                &mut out,
                "Reasons",
                &risk
                    .reasons
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join(", "),
            );
        }
        let _ = writeln!(out);
    }

    section(&mut out, "Testing", color);
    kv(&mut out, "Classification", &report.testing.classification);
    if !report.testing.frameworks.is_empty() {
        kv(
            &mut out,
            "Frameworks",
            &report
                .testing
                .frameworks
                .iter()
                .map(|framework| format!("{} ({})", framework.name, framework.evidence))
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    if report.testing.has_inline_tests {
        kv(&mut out, "Inline tests", "yes");
    }
    let _ = writeln!(out);

    section(&mut out, "Dependency graph", color);
    if report.graph.supported {
        kv(
            &mut out,
            "Fan-in / fan-out",
            &format!("{} / {}", report.graph.fan_in, report.graph.fan_out),
        );
        if !report.graph.dependencies.is_empty() {
            kv(&mut out, "Imports", &report.graph.dependencies.join(", "));
        }
        if !report.graph.dependents.is_empty() {
            kv(&mut out, "Imported by", &report.graph.dependents.join(", "));
        }
        if report.graph.unresolved_imports > 0 {
            kv(
                &mut out,
                "Unresolved",
                &thousands(report.graph.unresolved_imports),
            );
        }
    } else {
        kv(&mut out, "Status", "not supported for this file type");
    }
    let _ = writeln!(out);

    section(&mut out, "Findings", color);
    if report.findings.is_empty() {
        kv(&mut out, "", "none");
    } else {
        for finding in &report.findings {
            kv(
                &mut out,
                &format!("{} {}", finding.severity, finding.kind),
                &format!("{} — {}", location(finding), finding.message),
            );
        }
    }
    out
}

#[expect(
    clippy::too_many_lines,
    reason = "the focused Markdown renderer mirrors the compact table sections in one readable output contract"
)]
fn markdown(report: &ExplainReport) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# reposcout explain {}",
        markdown_code_span(&report.path.display().to_string())
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "## Discovery");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "- Status: **{}** — {}",
        markdown_text(&report.discovery.status),
        markdown_text(&report.discovery.reason)
    );
    if let Some(rule) = &report.discovery.rule {
        let _ = writeln!(
            out,
            "- Rule: **{}** {} from {}",
            markdown_text(&rule.kind),
            markdown_code_span(&rule.pattern),
            markdown_code_span(&rule.source)
        );
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "## Repository context");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "- {} files, {} tokens, {} source files.",
        thousands(report.repository.files),
        thousands(report.repository.tokens),
        thousands(report.repository.source_files)
    );
    if let Some(test_files) = report.repository.test_files {
        let _ = writeln!(
            out,
            "- Configured test files: **{}**.",
            thousands(test_files)
        );
    }
    let _ = writeln!(out);

    if let Some(file) = &report.file {
        let _ = writeln!(out, "## File");
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "- {} · {} · {} tokens · {} LOC / {} SLOC",
            markdown_text(&file.language),
            human_bytes(file.bytes),
            thousands(file.tokens),
            thousands(file.loc),
            thousands(file.sloc)
        );
        let _ = writeln!(out);
    }

    if let Some(risk) = &report.risk {
        let _ = writeln!(out, "## Risk");
        let _ = writeln!(out);
        let _ = writeln!(out, "- Algorithm: **version {}**.", risk.algorithm_version);
        let _ = writeln!(
            out,
            "- Score **{:.2}**: size {:.2}, complexity {:.2}, churn {:.2}.",
            risk.score, risk.size_factor, risk.complexity_factor, risk.churn_factor
        );
        if !risk.reasons.is_empty() {
            let reasons = risk
                .reasons
                .iter()
                .map(|reason| markdown_text(reason))
                .collect::<Vec<_>>()
                .join(", ");
            let _ = writeln!(out, "- Reasons: {reasons}.");
        }
        let _ = writeln!(out);
    }

    let _ = writeln!(out, "## Testing");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "- Classification: **{}**.",
        markdown_text(&report.testing.classification)
    );
    for framework in &report.testing.frameworks {
        let _ = writeln!(
            out,
            "- Runner: **{}** from {}.",
            markdown_text(&framework.name),
            markdown_code_span(&framework.evidence)
        );
    }
    if report.testing.has_inline_tests {
        let _ = writeln!(out, "- Inline Rust tests are present.");
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "## Dependency graph");
    let _ = writeln!(out);
    if report.graph.supported {
        let _ = writeln!(
            out,
            "- Fan-in **{}**; fan-out **{}**; unresolved imports **{}**.",
            report.graph.fan_in, report.graph.fan_out, report.graph.unresolved_imports
        );
        for path in &report.graph.dependencies {
            let _ = writeln!(out, "- Imports {}", markdown_code_span(path));
        }
        for path in &report.graph.dependents {
            let _ = writeln!(out, "- Imported by {}", markdown_code_span(path));
        }
    } else {
        let _ = writeln!(out, "- Not supported for this file type.");
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "## Findings");
    let _ = writeln!(out);
    if report.findings.is_empty() {
        let _ = writeln!(out, "None.");
    } else {
        for finding in &report.findings {
            let _ = writeln!(
                out,
                "- **{} {}** at {} — {}",
                markdown_text(&finding.severity),
                markdown_text(&finding.kind),
                markdown_code_span(&location(finding)),
                markdown_text(&finding.message)
            );
        }
    }
    out
}

fn section(out: &mut String, name: &str, color: bool) {
    if color {
        let _ = writeln!(out, "{}", name.cyan().bold());
    } else {
        let _ = writeln!(out, "{name}");
    }
}

fn kv(out: &mut String, key: &str, value: &str) {
    let key = terminal_text(key);
    let value = terminal_text(value);
    let _ = writeln!(out, "  {key:<18} {value}");
}

fn location(finding: &FindingRecord) -> String {
    let location = &finding.primary_location;
    if location.start_line == 0 {
        location.path.display().to_string()
    } else if location.end_line > location.start_line {
        format!(
            "{}:{}-{}",
            location.path.display(),
            location.start_line,
            location.end_line
        )
    } else {
        format!("{}:{}", location.path.display(), location.start_line)
    }
}
