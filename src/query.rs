//! Task-oriented repository queries for machine consumers.
//!
//! This module owns query semantics and hides the scan/cache plumbing. The CLI
//! is the first consumer; a future protocol can reuse this interface without
//! becoming a second implementation of repository analysis.

use crate::config::Config;
use crate::model::{
    CapabilitiesReport, ChangeSummaryCapability, SCHEMA_VERSION, SymbolMatch, SymbolQueryReport,
    WorkScopeCapability,
};
use crate::scan;
use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const MAX_SYMBOL_RESULTS: usize = 100;
pub const MAX_GRAPH_DEPTH: usize = 64;

pub fn capabilities() -> CapabilitiesReport {
    CapabilitiesReport {
        schema_version: SCHEMA_VERSION.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        default_operation: "scan".to_string(),
        default_invocation: "reposcout [PATH]".to_string(),
        commands: [
            "tokens",
            "complexity",
            "dup",
            "churn",
            "metrics",
            "explain",
            "locate",
            "update",
            "cache",
            "config",
            "capabilities",
            "daemon",
        ]
        .into_iter()
        .map(str::to_string)
        .collect(),
        output_formats: [
            "table", "json", "markdown", "ndjson", "sarif", "dot", "mermaid",
        ]
        .into_iter()
        .map(str::to_string)
        .collect(),
        symbol_query_formats: ["table", "json", "markdown", "ndjson"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        symbol_kinds: crate::metrics::symbols::OUTLINE_KINDS
            .iter()
            .map(|kind| (*kind).to_string())
            .collect(),
        execution_profiles: ["full", "agent", "safe"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        daemon_profiles: ["lite", "full", "safe"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        first_class_languages: crate::lang::FIRST_CLASS_LANGUAGE_NAMES
            .iter()
            .map(|name| (*name).to_string())
            .collect(),
        recognized_languages: crate::lang::RECOGNIZED_LANGUAGE_NAMES
            .iter()
            .map(|name| (*name).to_string())
            .collect(),
        default_health_languages: crate::lang::SOURCE_LANGUAGE_NAMES
            .iter()
            .map(|name| (*name).to_string())
            .collect(),
        optional_health_formats: crate::lang::OPTIONAL_HEALTH_FORMAT_NAMES
            .iter()
            .map(|name| (*name).to_string())
            .collect(),
        health_scopes: ["source", "all"].into_iter().map(str::to_string).collect(),
        health_exclude_flag: "--health-exclude".to_string(),
        duplication_include_artifacts_flag: "--dup-include-artifacts".to_string(),
        machine_interfaces: ["cli-json", "cli-ndjson", "debug-log-ndjson"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        error_formats: ["text", "json"].into_iter().map(str::to_string).collect(),
        max_graph_depth: MAX_GRAPH_DEPTH,
        max_symbol_results: MAX_SYMBOL_RESULTS,
        change_summary: ChangeSummaryCapability {
            flag: "--change-summary".to_string(),
            requires_one_of: ["--since", "--staged", "--working"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            implies: ["summary", "context", "impact"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            formats: ["table", "json", "markdown", "ndjson"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            max_path_entries: crate::change_summary::MAX_PATH_ENTRIES,
            max_gap_entries: crate::change_summary::MAX_GAP_ENTRIES,
            max_validations: crate::change_summary::MAX_VALIDATIONS,
        },
        work_scope: WorkScopeCapability {
            strategy_version: crate::work_scope::STRATEGY_VERSION,
            max_path_entries: crate::work_scope::MAX_PATH_ENTRIES,
            max_components: crate::work_scope::MAX_COMPONENTS,
        },
        type2_max_seed_pairs_per_pool: crate::dup::fuzzy::MAX_SEED_PAIRS_PER_POOL,
        type2_max_matches_per_pool: crate::dup::fuzzy::MAX_MATCHES_PER_POOL,
        type2_max_overlap_checks_per_pool: crate::dup::fuzzy::MAX_OVERLAP_CHECKS_PER_POOL,
    }
}

#[derive(Debug, Clone)]
pub struct LocateOptions {
    pub query: String,
    pub exact: bool,
    pub kind: Option<String>,
    pub language: Option<String>,
    pub limit: usize,
}

/// Locate declarations matching a qualified or simple symbol name.
///
/// # Errors
///
/// Returns an error for an empty or invalid query, or when repository
/// discovery and per-file declaration analysis cannot complete.
pub fn locate(
    target: &Path,
    cfg: &Config,
    exclusions: &[PathBuf],
    options: &LocateOptions,
) -> Result<SymbolQueryReport> {
    let query = options.query.trim();
    if query.is_empty() {
        return Err(anyhow!("symbol query cannot be empty"));
    }
    if options.limit == 0 || options.limit > MAX_SYMBOL_RESULTS {
        return Err(anyhow!(
            "--limit must be between 1 and {MAX_SYMBOL_RESULTS}"
        ));
    }

    // Declaration lookup needs no whole-repository duplication or Git walk.
    // Keep the configured per-file profile so this query can reuse ordinary
    // scan entries instead of replacing them with a query-only cache profile.
    let query_config = declaration_query_config(cfg);

    let artifacts = scan::run_with_artifacts(
        target,
        &query_config,
        exclusions,
        scan::ArtifactRequirements {
            symbol_outlines: true,
            graph_facts: false,
        },
    )?;
    let languages = artifacts
        .report
        .files
        .iter()
        .map(|file| (file.path.clone(), file.language.clone()))
        .collect::<HashMap<_, _>>();
    let normalized_kind = normalize_filter(options.kind.as_deref());
    let normalized_language = normalize_filter(options.language.as_deref());
    let mut matches = Vec::new();
    for (path, symbols) in &artifacts.symbol_outlines {
        let language = languages.get(path).cloned().unwrap_or_default();
        if normalized_language
            .as_deref()
            .is_some_and(|filter| !language.eq_ignore_ascii_case(filter))
        {
            continue;
        }
        for symbol in symbols {
            if normalized_kind
                .as_deref()
                .is_some_and(|kind| !symbol.kind.eq_ignore_ascii_case(kind))
            {
                continue;
            }
            let Some(rank) = match_rank(&symbol.name, query, options.exact) else {
                continue;
            };
            matches.push(SymbolMatch {
                path: path.clone(),
                language: language.clone(),
                name: symbol.name.clone(),
                kind: symbol.kind.clone(),
                signature: symbol.signature.clone(),
                line: symbol.line,
                exported: symbol.exported,
                rank,
            });
        }
    }
    matches.sort_by(|left, right| {
        left.rank
            .cmp(&right.rank)
            .then_with(|| right.exported.cmp(&left.exported))
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.line.cmp(&right.line))
            .then_with(|| left.name.cmp(&right.name))
    });
    let total_matches = matches.len();
    matches.truncate(options.limit);

    Ok(SymbolQueryReport {
        schema_version: SCHEMA_VERSION.to_string(),
        root: artifacts.report.root,
        target: artifacts.report.target,
        generated_at: chrono::Utc::now().to_rfc3339(),
        query: query.to_string(),
        match_mode: if options.exact { "exact" } else { "ranked" }.to_string(),
        kind: normalized_kind,
        language: normalized_language,
        total_matches,
        returned_matches: matches.len(),
        truncated: total_matches > matches.len(),
        first_class_files: artifacts
            .report
            .files
            .iter()
            .filter(|file| {
                crate::lang::detect(&file.path).is_some_and(super::lang::LangInfo::is_first_class)
            })
            .count(),
        execution: artifacts.report.execution,
        matches,
    })
}

fn declaration_query_config(cfg: &Config) -> Config {
    let mut query_config = cfg.clone();
    query_config.enabled.duplication = false;
    query_config.enabled.churn = false;
    query_config.context = false;
    query_config.by_dir = None;
    query_config.diff_scope = None;
    query_config.baseline_path = None;
    query_config.graph = false;
    query_config.graph_focus.clear();
    query_config.impact = false;
    query_config.review = None;
    query_config
}

fn normalize_filter(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn match_rank(candidate: &str, query: &str, exact: bool) -> Option<usize> {
    let simple = candidate
        .rsplit(['.', ':', '\\'])
        .find(|part| !part.is_empty())
        .unwrap_or(candidate);
    if candidate == query {
        return Some(0);
    }
    if !exact && candidate.eq_ignore_ascii_case(query) {
        return Some(1);
    }
    if simple == query {
        return Some(2);
    }
    if !exact && simple.eq_ignore_ascii_case(query) {
        return Some(3);
    }
    if exact {
        return None;
    }
    let candidate_lower = candidate.to_lowercase();
    let query_lower = query.to_lowercase();
    if candidate_lower.starts_with(&query_lower) || simple.to_lowercase().starts_with(&query_lower)
    {
        Some(4)
    } else if candidate_lower.contains(&query_lower) {
        Some(5)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn ranking_prefers_qualified_then_simple_exact_matches() {
        assert_eq!(
            match_rank("Client.request", "Client.request", false),
            Some(0)
        );
        assert_eq!(match_rank("Client.request", "request", false), Some(2));
        assert_eq!(match_rank("Client.request", "REQUEST", false), Some(3));
        assert_eq!(match_rank("Client.request", "req", false), Some(4));
        assert_eq!(match_rank("Client.request", "quest", false), Some(5));
        assert_eq!(match_rank("Client.request", "request", true), Some(2));
        assert_eq!(match_rank("Client.request", "REQUEST", true), None);
        assert_eq!(match_rank("Client.request", "CLIENT.REQUEST", true), None);
        assert_eq!(match_rank("Client.request", "quest", true), None);
    }

    #[test]
    fn advertised_commands_match_the_clap_surface() {
        let mut advertised = capabilities().commands;
        advertised.sort();
        let mut accepted = crate::cli::Cli::command()
            .get_subcommands()
            .map(|command| command.get_name().to_string())
            .collect::<Vec<_>>();
        accepted.sort();

        assert_eq!(advertised, accepted);
    }

    #[test]
    fn capabilities_advertise_duplication_artifact_opt_in() {
        assert_eq!(
            capabilities().duplication_include_artifacts_flag,
            "--dup-include-artifacts"
        );
    }
}
