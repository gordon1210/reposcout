//! Rendering for inspectable layered configuration.

use crate::cli::ConfigOutputFormat;
use crate::config::{ConfigInspection, ConfigSource};
use crate::dup::DuplicationFormatScope;
use anyhow::{Context, Result};
use std::fmt::Write as _;

/// Render inspectable configuration as a human table or JSON.
///
/// # Errors
///
/// Returns an error when JSON serialization fails.
pub fn render(
    inspection: &ConfigInspection,
    format: ConfigOutputFormat,
    pretty_json: bool,
) -> Result<String> {
    match format {
        ConfigOutputFormat::Table => Ok(render_table(inspection)),
        ConfigOutputFormat::Json => super::json_string(inspection, pretty_json)
            .context("failed to render configuration JSON"),
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "configuration inspection exhaustively renders provenance and every file-configurable field as one stable diagnostic contract"
)]
fn render_table(inspection: &ConfigInspection) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "RepoScout configuration");
    let _ = writeln!(out, "Precedence: {}", inspection.precedence.join(" > "));
    let _ = writeln!(out, "Mode: {}", inspection.config_mode);
    render_source(&mut out, "Global", inspection.sources.global.as_ref());
    render_source(&mut out, "Project", inspection.sources.project.as_ref());

    let config = &inspection.effective;
    let _ = writeln!(out, "\nEffective values");
    value(&mut out, "execution_profile", &config.execution_profile);
    value(
        &mut out,
        "analyzers",
        &format!(
            "tokens={}, lines={}, complexity={}, imports={}, markers={}, duplication={}, churn={}",
            config.analyzers.tokens,
            config.analyzers.lines,
            config.analyzers.complexity,
            config.analyzers.imports,
            config.analyzers.markers,
            config.analyzers.duplication,
            config.analyzers.churn
        ),
    );
    value(
        &mut out,
        "safety_limits",
        &format_list(&config.safety_limits),
    );
    value(&mut out, "encoding", &config.encoding);
    value(&mut out, "jobs", &config.jobs.to_string());
    value(&mut out, "use_cache", &config.use_cache.to_string());
    value(&mut out, "top", &config.top.to_string());
    value(
        &mut out,
        "max_complexity",
        &config.max_complexity.to_string(),
    );
    value(
        &mut out,
        "include_hidden",
        &config.include_hidden.to_string(),
    );
    value(
        &mut out,
        "respect_gitignore",
        &config.respect_gitignore.to_string(),
    );
    value(
        &mut out,
        "exclude_lockfiles",
        &config.exclude_lockfiles.to_string(),
    );
    value(&mut out, "excludes", &format_list(&config.excludes));
    value(&mut out, "markers", &format_list(&config.markers));
    value(&mut out, "health_scope", &config.health_scope.to_string());
    value(
        &mut out,
        "health_includes",
        &format_list(
            &config
                .health_includes
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        ),
    );
    value(
        &mut out,
        "health_excludes",
        &format_list(&config.health_excludes),
    );
    value(
        &mut out,
        "min_dup_tokens",
        &config.min_dup_tokens.to_string(),
    );
    value(&mut out, "min_dup_lines", &config.min_dup_lines.to_string());
    value(
        &mut out,
        "near_dup_min_similarity",
        &config.near_dup_min_similarity.to_string(),
    );
    value(
        &mut out,
        "duplication_mode",
        &config.duplication_mode.to_string(),
    );
    value(
        &mut out,
        "duplication_format_scope",
        match config.duplication_format_scope {
            DuplicationFormatScope::Exact => "exact",
            DuplicationFormatScope::Compatible => "compatible",
            DuplicationFormatScope::All => "all",
        },
    );
    value(
        &mut out,
        "duplication_include_artifacts",
        &config.duplication_include_artifacts.to_string(),
    );
    value(
        &mut out,
        "duplication_report_snippets",
        &config.duplication_report_snippets.to_string(),
    );
    value(
        &mut out,
        "churn_max_commits",
        &config.churn_max_commits.to_string(),
    );
    value(
        &mut out,
        "max_churn_deltas_per_commit",
        &config.max_churn_deltas_per_commit.to_string(),
    );
    value(
        &mut out,
        "max_churn_total_deltas",
        &config.max_churn_total_deltas.to_string(),
    );
    value(
        &mut out,
        "max_churn_output_bytes",
        &config.max_churn_output_bytes.to_string(),
    );
    value(
        &mut out,
        "load_repository_ignores",
        &config.load_repository_ignores.to_string(),
    );
    value(
        &mut out,
        "max_file_bytes",
        &config.max_file_bytes.to_string(),
    );
    value(
        &mut out,
        "max_total_bytes",
        &config.max_total_bytes.to_string(),
    );
    value(&mut out, "max_files", &config.max_files.to_string());
    value(
        &mut out,
        "max_git_blob_bytes",
        &config.max_git_blob_bytes.to_string(),
    );
    value(
        &mut out,
        "max_scan_seconds",
        &config.max_scan_seconds.to_string(),
    );
    value(&mut out, "context", &config.context.to_string());
    value(
        &mut out,
        "context_budget",
        &config.context_budget.to_string(),
    );
    value(
        &mut out,
        "context_max_files",
        &config.context_max_files.to_string(),
    );
    out
}

fn render_source(out: &mut String, label: &str, source: Option<&ConfigSource>) {
    let Some(source) = source else {
        let _ = writeln!(out, "{label}: unavailable");
        return;
    };
    let path = super::terminal_text(&source.path.to_string_lossy());
    if source.ignored {
        let _ = writeln!(out, "{label}: {path} (ignored by caller)");
    } else if source.loaded {
        let keys = if source.keys.is_empty() {
            "no explicit keys".to_string()
        } else {
            source.keys.join(", ")
        };
        let _ = writeln!(out, "{label}: {path} (loaded: {keys})");
    } else {
        let _ = writeln!(out, "{label}: {path} (not found)");
    }
}

fn value(out: &mut String, key: &str, rendered: &str) {
    let _ = writeln!(out, "  {key:<30} {}", super::terminal_text(rendered));
}

fn format_list(values: &[String]) -> String {
    if values.is_empty() {
        "[]".to_string()
    } else {
        format!("[{}]", values.join(", "))
    }
}
