//! Deterministic, token-budgeted reading plans for agents and humans.
//!
//! This module ranks already-analyzed files. It never reads source, performs
//! network I/O, or creates a second analysis pipeline; optional graph signals
//! are accepted at the module seam and enrich focus neighborhoods when the
//! graph supports the language.

use crate::config::Config;
use crate::graph::{GraphSignals, is_entrypoint};
use crate::lang;
use crate::metrics::testcov;
use crate::model::{
    ContextEvidence, ContextFile, ContextOmission, ContextOutlineOnly, ContextPlan, FileReport,
    RiskEntry, SymbolOutline,
};
use anyhow::{Result, bail};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Component, Path, PathBuf};

const STRATEGY_VERSION: u32 = 2;
const MAX_OMISSION_DETAILS: usize = 10;
const MAX_OUTLINE_BYTES: usize = 16 * 1024;
const MAX_OUTLINE_BYTES_PER_FILE: usize = 2 * 1024;
const MAX_OUTLINE_SYMBOLS_PER_FILE: usize = 16;
const MAX_PRIVATE_SYMBOLS_PER_FILE: usize = 4;

#[derive(Debug, Clone)]
pub(crate) struct ChangeSeeds {
    pub scope: String,
    pub paths: HashSet<PathBuf>,
}

struct Candidate {
    path: PathBuf,
    path_key: String,
    tokens: usize,
    score: f64,
    reasons: Vec<String>,
    evidence: Vec<ContextEvidence>,
}

#[derive(Default)]
struct OutlineStats {
    symbols: usize,
    bytes: usize,
    omitted_symbols: usize,
}

#[derive(Clone)]
struct GraphReach {
    distance: usize,
    resolver: Option<String>,
}

pub(crate) struct PlanningPaths<'a> {
    pub root: &'a Path,
    pub target: &'a Path,
}

pub(crate) fn build_for_target(
    files: &[FileReport],
    risks: &[RiskEntry],
    outlines: &BTreeMap<PathBuf, Vec<SymbolOutline>>,
    graph: Option<&GraphSignals>,
    paths: PlanningPaths<'_>,
    cfg: &Config,
    changes: Option<&ChangeSeeds>,
) -> Result<ContextPlan> {
    let (focus, unmatched_focus) =
        resolve_focus(&cfg.context_focus, files, paths.root, paths.target)?;
    let focus_keys = focus
        .iter()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .collect::<Vec<_>>();

    let explicit_focused_files = files
        .iter()
        .filter(|file| matches_focus(&path_key(&file.path), &focus_keys))
        .map(|file| path_key(&file.path))
        .collect::<HashSet<_>>();
    let changed_keys = changes
        .map(|changes| {
            changes
                .paths
                .iter()
                .map(|path| path_key(path))
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();
    let changed_files = files
        .iter()
        .filter(|file| changed_keys.contains(&path_key(&file.path)))
        .map(|file| path_key(&file.path))
        .collect::<HashSet<_>>();
    let seed_files = explicit_focused_files
        .union(&changed_keys)
        .cloned()
        .collect::<HashSet<_>>();
    let focus_directories = seed_files
        .iter()
        .filter_map(|path| Path::new(path).parent())
        .map(path_key)
        .collect::<HashSet<_>>();

    let mut dependencies = HashMap::new();
    if let Some(graph) = graph {
        let mut ordered_seeds = seed_files.iter().collect::<Vec<_>>();
        ordered_seeds.sort();
        for path in ordered_seeds {
            if let Some(signal) = graph.files.get(path) {
                for dependency in &signal.dependencies {
                    dependencies
                        .entry(dependency.clone())
                        .or_insert_with(|| signal.dependency_resolvers.get(dependency).cloned());
                }
            }
        }
    }
    let dependents = dependent_reach(graph, &seed_files);
    let direct_dependents = dependents
        .values()
        .filter(|reach| reach.distance == 1)
        .count();
    let transitive_dependents = dependents
        .values()
        .filter(|reach| reach.distance > 1)
        .count();
    let graph_eligible_seed_files = seed_files
        .iter()
        .filter(|path| {
            lang::detect(Path::new(path)).is_some_and(|language| language.is_first_class())
        })
        .count();
    let graph_covered_seed_files = graph
        .map(|graph| {
            seed_files
                .iter()
                .filter(|path| graph.files.contains_key(path.as_str()))
                .count()
        })
        .unwrap_or(0);

    let focus_source_stems = files
        .iter()
        .filter(|file| seed_files.contains(&path_key(&file.path)))
        .filter(|file| {
            lang::detect(&file.path).is_some_and(|info| info.is_code())
                && !testcov::is_test_file(&path_key(&file.path))
        })
        .map(|file| testcov::source_stem(&path_key(&file.path)))
        .chain(
            changed_keys
                .iter()
                .filter(|path| {
                    lang::detect(Path::new(path)).is_some_and(|info| info.is_code())
                        && !testcov::is_test_file(path)
                })
                .map(|path| testcov::source_stem(path)),
        )
        .collect::<HashSet<_>>();
    let risk_by_path = risks
        .iter()
        .map(|risk| (risk.path.as_str(), risk))
        .collect::<HashMap<_, _>>();

    let mut skipped_files = 0usize;
    let mut matching_tests = 0usize;
    let mut candidates = Vec::new();
    for file in files {
        let path = path_key(&file.path);
        let focused = explicit_focused_files.contains(&path);
        let changed = changed_files.contains(&path);
        let dependency = dependencies.get(&path);
        let dependent = dependents.get(&path);
        let shares_focus_directory = !focused
            && !changed
            && Path::new(&path)
                .parent()
                .is_some_and(|parent| focus_directories.contains(&path_key(parent)));
        let matching_test = !focus_source_stems.is_empty()
            && testcov::is_test_file(&path)
            && testcov::test_stem_keys(&path)
                .iter()
                .any(|key| focus_source_stems.contains(key));
        if matching_test {
            matching_tests = matching_tests.saturating_add(1);
        }
        let is_code = lang::detect(&file.path).is_some_and(|info| info.is_code());
        let support = support_role(&path);
        let risk = risk_by_path.get(path.as_str()).copied();

        if file.skip_hint.is_some() && !focused && !changed {
            skipped_files += 1;
            continue;
        }
        if !is_code
            && support.is_none()
            && !focused
            && !changed
            && dependency.is_none()
            && dependent.is_none()
            && !matching_test
        {
            skipped_files += 1;
            continue;
        }

        let mut score = if is_code { 10.0 } else { 0.0 };
        let mut reasons = Vec::new();
        let mut evidence = Vec::new();
        if focused {
            score += 1_200.0;
            reasons.push("focus path".to_string());
            if let Some(hint) = &file.skip_hint {
                reasons.push(format!("focus overrides {hint} skip hint"));
            }
        }
        if changed {
            score += 1_100.0;
            let scope = changes
                .map(|changes| changes.scope.as_str())
                .unwrap_or("diff");
            reasons.push(format!("changed in {scope} scope"));
            evidence.push(ContextEvidence {
                role: "changed".to_string(),
                confidence: "high".to_string(),
                distance: Some(0),
                resolver: None,
            });
            if let Some(hint) = &file.skip_hint {
                reasons.push(format!("change overrides {hint} skip hint"));
            }
        }
        if let Some(resolver) = dependency {
            score += 600.0;
            reasons.push(if changes.is_some() {
                "direct dependency of change".to_string()
            } else {
                "direct dependency of focus".to_string()
            });
            evidence.push(graph_evidence("dependency", 1, resolver.clone()));
        }
        if let Some(reach) = dependent {
            if reach.distance == 1 {
                score += 550.0;
                reasons.push(if changes.is_some() {
                    "direct dependent of change".to_string()
                } else {
                    "direct dependent of focus".to_string()
                });
            } else {
                score += 425.0 / reach.distance as f64;
                let seed = if changes.is_some() { "change" } else { "focus" };
                reasons.push(format!(
                    "transitive dependent of {seed} ({} hops)",
                    reach.distance
                ));
            }
            evidence.push(graph_evidence(
                "dependent",
                reach.distance,
                reach.resolver.clone(),
            ));
        }
        if matching_test {
            score += 500.0;
            reasons.push(if changes.is_some() {
                "matching test for change".to_string()
            } else {
                "matching test for focus".to_string()
            });
            evidence.push(ContextEvidence {
                role: "matching-test".to_string(),
                confidence: "partial".to_string(),
                distance: None,
                resolver: None,
            });
        }
        if shares_focus_directory {
            let elevated_nearby_risk =
                changes.is_some() && risk.is_some_and(|risk| risk.score >= 0.4);
            score += if changes.is_some() && !elevated_nearby_risk {
                75.0
            } else {
                250.0
            };
            reasons.push(if elevated_nearby_risk {
                "nearby elevated-risk code".to_string()
            } else if changes.is_some() {
                "shares changed-file directory".to_string()
            } else {
                "shares focus directory".to_string()
            });
            evidence.push(ContextEvidence {
                role: "nearby".to_string(),
                confidence: "partial".to_string(),
                distance: None,
                resolver: None,
            });
        }
        if let Some((role, weight)) = support {
            score += weight;
            if !path.contains('/') {
                score += 100.0;
            }
            reasons.push(role.to_string());
        }
        if is_entrypoint(&path) {
            score += 300.0;
            reasons.push("entrypoint".to_string());
        }
        if let Some(signal) = graph.and_then(|graph| graph.files.get(&path)) {
            if signal.fan_in > 0 {
                score += (signal.fan_in.min(20) * 18) as f64;
                reasons.push(format!("depended on by {} graph files", signal.fan_in));
            }
            if signal.fan_out > 0 {
                score += (signal.fan_out.min(20) * 5) as f64;
                reasons.push(format!("connects {} internal dependencies", signal.fan_out));
            }
        }
        if let Some(risk) = risk {
            score += risk.score * 200.0;
            if risk.score >= 0.7 {
                reasons.push(format!("high risk ({:.2})", risk.score));
            } else if risk.score >= 0.4 {
                reasons.push(format!("elevated risk ({:.2})", risk.score));
            }
        }
        if let Some(churn) = &file.churn
            && churn.commits >= 5
        {
            score += churn.commits.min(20) as f64 * 4.0;
            reasons.push(format!("active history ({} commits)", churn.commits));
        }
        if let Some(complexity) = &file.complexity
            && complexity.cyclomatic >= 10
        {
            score += complexity.cyclomatic.min(100) as f64 * 1.5;
            reasons.push(format!("complex control flow ({})", complexity.cyclomatic));
        }
        if reasons.is_empty() {
            reasons.push(if testcov::is_test_file(&path) {
                "test source".to_string()
            } else {
                "representative source file".to_string()
            });
        }

        candidates.push(Candidate {
            path: file.path.clone(),
            path_key: path,
            tokens: file.tokens,
            score: round_score(score),
            reasons,
            evidence,
        });
    }

    candidates.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.tokens.cmp(&b.tokens))
            .then_with(|| a.path_key.cmp(&b.path_key))
    });
    let candidate_files = candidates.len();
    let mut selected_tokens = 0usize;
    let mut selected = Vec::new();
    let mut outline_only = Vec::new();
    let mut omitted = Vec::new();
    let mut omitted_files = 0usize;
    let mut omitted_tokens = 0usize;

    for candidate in candidates {
        let reason = if selected.len() >= cfg.context_max_files {
            Some("file limit reached")
        } else if candidate.tokens > cfg.context_budget {
            Some("file exceeds total token budget")
        } else if selected_tokens.saturating_add(candidate.tokens) > cfg.context_budget {
            Some("does not fit remaining token budget")
        } else {
            None
        };
        if let Some(reason) = reason {
            omitted_files += 1;
            omitted_tokens = omitted_tokens.saturating_add(candidate.tokens);
            if seed_files.contains(&candidate.path_key) {
                outline_only.push((
                    ContextFile {
                        path: candidate.path.clone(),
                        tokens: candidate.tokens,
                        score: candidate.score,
                        reasons: candidate.reasons.clone(),
                        evidence: candidate.evidence.clone(),
                        symbols: Vec::new(),
                    },
                    reason.to_string(),
                ));
            }
            if omitted.len() < MAX_OMISSION_DETAILS {
                omitted.push(ContextOmission {
                    path: candidate.path,
                    tokens: candidate.tokens,
                    reason: reason.to_string(),
                });
            }
            continue;
        }
        selected_tokens += candidate.tokens;
        selected.push(ContextFile {
            path: candidate.path,
            tokens: candidate.tokens,
            score: candidate.score,
            reasons: candidate.reasons,
            evidence: candidate.evidence,
            symbols: Vec::new(),
        });
    }

    let mut outline_stats = OutlineStats::default();
    attach_outlines(
        &mut selected,
        files,
        outlines,
        cfg.max_complexity,
        &mut outline_stats,
    );
    let mut outline_only_files = outline_only
        .iter()
        .map(|(file, _)| file.clone())
        .collect::<Vec<_>>();
    attach_outlines(
        &mut outline_only_files,
        files,
        outlines,
        cfg.max_complexity,
        &mut outline_stats,
    );
    let outline_only = outline_only
        .into_iter()
        .zip(outline_only_files)
        .filter_map(|((source, reason), outlined)| {
            (!outlined.symbols.is_empty()).then_some(ContextOutlineOnly {
                path: source.path,
                source_tokens: source.tokens,
                score: source.score,
                reason,
                reasons: source.reasons,
                evidence: source.evidence,
                symbols: outlined.symbols,
            })
        })
        .collect::<Vec<_>>();

    let mut changed_paths = changes
        .map(|changes| changes.paths.iter().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    changed_paths.sort();

    Ok(ContextPlan {
        strategy_version: STRATEGY_VERSION,
        planning_ms: 0,
        budget_tokens: cfg.context_budget,
        selected_tokens,
        candidate_files,
        omitted_files,
        omitted_tokens,
        skipped_files,
        seed_files: seed_files.len(),
        graph_eligible_seed_files,
        graph_covered_seed_files,
        direct_dependents,
        transitive_dependents,
        matching_tests,
        focus,
        unmatched_focus,
        change_scope: changes.map(|changes| changes.scope.clone()),
        changed_files: changed_paths,
        graph_languages: graph
            .map(|graph| graph.languages.clone())
            .unwrap_or_default(),
        graph_unresolved_imports: graph.map(|graph| graph.unresolved_imports).unwrap_or(0),
        graph_parse_errors: graph.map(|graph| graph.parse_errors).unwrap_or(0),
        graph_config_errors: graph.map(|graph| graph.config_errors).unwrap_or(0),
        outline_symbols: outline_stats.symbols,
        outline_bytes: outline_stats.bytes,
        outline_omitted_symbols: outline_stats.omitted_symbols,
        planning_diagnostics: None,
        files: selected,
        outline_only,
        omitted,
    })
}

#[cfg(test)]
fn build(
    files: &[FileReport],
    risks: &[RiskEntry],
    outlines: &BTreeMap<PathBuf, Vec<SymbolOutline>>,
    graph: Option<&GraphSignals>,
    root: &Path,
    cfg: &Config,
    changes: Option<&ChangeSeeds>,
) -> ContextPlan {
    build_for_target(
        files,
        risks,
        outlines,
        graph,
        PlanningPaths { root, target: root },
        cfg,
        changes,
    )
    .unwrap()
}

fn dependent_reach(
    graph: Option<&GraphSignals>,
    seeds: &HashSet<String>,
) -> HashMap<String, GraphReach> {
    let Some(graph) = graph else {
        return HashMap::new();
    };
    let mut ordered_seeds = seeds.iter().cloned().collect::<Vec<_>>();
    ordered_seeds.sort();
    let mut queue = VecDeque::new();
    let mut distances = HashMap::new();
    for seed in ordered_seeds {
        distances.insert(seed.clone(), 0usize);
        queue.push_back(seed);
    }
    let mut reach = HashMap::new();
    while let Some(path) = queue.pop_front() {
        let distance = distances.get(&path).copied().unwrap_or(0);
        let Some(signal) = graph.files.get(&path) else {
            continue;
        };
        for dependent in &signal.dependents {
            let next_distance = distance.saturating_add(1);
            let should_update = distances
                .get(dependent)
                .is_none_or(|known| next_distance < *known);
            if !should_update {
                continue;
            }
            distances.insert(dependent.clone(), next_distance);
            queue.push_back(dependent.clone());
            if !seeds.contains(dependent) {
                reach.insert(
                    dependent.clone(),
                    GraphReach {
                        distance: next_distance,
                        resolver: (next_distance == 1)
                            .then(|| signal.dependent_resolvers.get(dependent).cloned())
                            .flatten(),
                    },
                );
            }
        }
    }
    reach
}

fn graph_evidence(role: &str, distance: usize, resolver: Option<String>) -> ContextEvidence {
    let confidence = if distance == 1 && resolver.as_deref().is_some_and(resolver_is_precise) {
        "high"
    } else {
        "partial"
    };
    ContextEvidence {
        role: role.to_string(),
        confidence: confidence.to_string(),
        distance: Some(distance),
        resolver,
    }
}

fn resolver_is_precise(resolver: &str) -> bool {
    matches!(
        resolver,
        "relative"
            | "python-relative"
            | "tsconfig-paths"
            | "tsconfig-base-url"
            | "package-imports"
            | "package-exports"
            | "package-entrypoint"
            | "composer-psr-4"
            | "composer-psr-0"
            | "php-include"
    )
}

fn attach_outlines(
    selected: &mut [ContextFile],
    files: &[FileReport],
    outlines: &BTreeMap<PathBuf, Vec<SymbolOutline>>,
    max_complexity: u32,
    stats: &mut OutlineStats,
) {
    let files_by_path = files
        .iter()
        .map(|file| (&file.path, file))
        .collect::<HashMap<_, _>>();
    for selected_file in selected {
        let Some(source) = outlines.get(&selected_file.path) else {
            continue;
        };
        let report = files_by_path.get(&selected_file.path).copied();
        let mut candidates = source.clone();
        for symbol in &mut candidates {
            if symbol.reasons.is_empty() {
                symbol.reasons.push(
                    if symbol.exported {
                        "exported/public declaration"
                    } else {
                        "representative file-local declaration"
                    }
                    .to_string(),
                );
            }
            if matches!(symbol.kind.as_str(), "function" | "method")
                && let Some(function) =
                    report
                        .and_then(|file| file.complexity.as_ref())
                        .and_then(|complexity| {
                            complexity.functions.iter().find(|function| {
                                function.line == symbol.line
                                    && symbol
                                        .name
                                        .rsplit('.')
                                        .next()
                                        .is_some_and(|name| name == function.name)
                            })
                        })
                && function.cyclomatic > max_complexity
            {
                symbol.reasons.push(format!(
                    "complexity finding (cyclomatic {})",
                    function.cyclomatic
                ));
            }
        }
        candidates.sort_by(|left, right| {
            symbol_has_finding(right)
                .cmp(&symbol_has_finding(left))
                .then_with(|| right.exported.cmp(&left.exported))
                .then_with(|| left.line.cmp(&right.line))
                .then_with(|| left.name.cmp(&right.name))
        });

        let mut file_bytes = 0usize;
        let mut private_symbols = 0usize;
        for symbol in candidates {
            if !symbol.exported {
                if private_symbols >= MAX_PRIVATE_SYMBOLS_PER_FILE {
                    stats.omitted_symbols += 1;
                    continue;
                }
                private_symbols += 1;
            }
            let bytes = outline_payload_bytes(&symbol);
            if selected_file.symbols.len() >= MAX_OUTLINE_SYMBOLS_PER_FILE
                || file_bytes.saturating_add(bytes) > MAX_OUTLINE_BYTES_PER_FILE
                || stats.bytes.saturating_add(bytes) > MAX_OUTLINE_BYTES
            {
                stats.omitted_symbols += 1;
                continue;
            }
            file_bytes += bytes;
            stats.bytes += bytes;
            stats.symbols += 1;
            selected_file.symbols.push(symbol);
        }
    }
}

fn symbol_has_finding(symbol: &SymbolOutline) -> bool {
    symbol
        .reasons
        .iter()
        .any(|reason| reason.starts_with("complexity finding"))
}

fn outline_payload_bytes(symbol: &SymbolOutline) -> usize {
    serde_json::to_vec(symbol)
        .map(|payload| payload.len())
        .unwrap_or_else(|_| {
            symbol.name.len()
                + symbol.kind.len()
                + symbol.signature.len()
                + symbol.reasons.iter().map(String::len).sum::<usize>()
        })
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn resolve_focus(
    requested: &[PathBuf],
    files: &[FileReport],
    root: &Path,
    target: &Path,
) -> Result<(Vec<PathBuf>, Vec<PathBuf>)> {
    let file_keys = files
        .iter()
        .map(|file| path_key(&file.path))
        .collect::<Vec<_>>();
    let target_relative = target.strip_prefix(root).unwrap_or(Path::new(""));
    let target_base = if target.is_file() {
        target_relative.parent().unwrap_or(Path::new(""))
    } else {
        target_relative
    };
    let mut resolved = Vec::new();
    let mut unmatched = Vec::new();

    for path in requested {
        if path.is_absolute() {
            let Ok(relative) = path.strip_prefix(root) else {
                unmatched.push(path.clone());
                continue;
            };
            let candidate = normalize_relative(relative);
            if focus_candidate_matches(&candidate, &file_keys) {
                resolved.push(candidate);
            } else {
                unmatched.push(path.clone());
            }
            continue;
        }

        let root_candidate = normalize_relative(path);
        let target_candidate = normalize_relative(&target_base.join(path));
        let mut matches = [root_candidate.clone(), target_candidate]
            .into_iter()
            .filter(|candidate| {
                !candidate.as_os_str().is_empty() && focus_candidate_matches(candidate, &file_keys)
            })
            .collect::<Vec<_>>();
        matches.sort();
        matches.dedup();

        match matches.as_slice() {
            [candidate] => resolved.push(candidate.clone()),
            [] => unmatched.push(root_candidate),
            candidates => {
                bail!(
                    "focus path '{}' is ambiguous between {}; use a repo-relative path",
                    path.display(),
                    candidates
                        .iter()
                        .map(|candidate| candidate.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }
    }

    resolved.sort();
    resolved.dedup();
    unmatched.sort();
    unmatched.dedup();
    Ok((resolved, unmatched))
}

fn focus_candidate_matches(candidate: &Path, file_keys: &[String]) -> bool {
    let candidate = path_key(candidate);
    file_keys.iter().any(|path| {
        path == &candidate
            || path
                .strip_prefix(&candidate)
                .is_some_and(|suffix| suffix.starts_with('/'))
    })
}

fn normalize_relative(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir | Component::RootDir | Component::Prefix(_) => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

fn matches_focus(path: &str, focus: &[String]) -> bool {
    focus.iter().any(|focus| {
        path == focus
            || path
                .strip_prefix(focus)
                .is_some_and(|suffix| suffix.starts_with('/'))
    })
}

fn support_role(path: &str) -> Option<(&'static str, f64)> {
    let filename = path.rsplit('/').next().unwrap_or(path);
    let lower = filename.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "agents.md" | "claude.md" | "gemini.md" | "codex.md"
    ) {
        return Some(("repository instructions", 700.0));
    }
    if lower == "handoff.md" {
        return Some(("repository handoff", 400.0));
    }
    if lower.starts_with("readme") || lower.starts_with("contributing") {
        return Some(("project overview", 350.0));
    }
    if matches!(
        lower.as_str(),
        "cargo.toml"
            | "package.json"
            | "pyproject.toml"
            | "go.mod"
            | "pom.xml"
            | "build.gradle"
            | "build.gradle.kts"
            | "makefile"
            | "justfile"
            | "reposcout.toml"
            | ".reposcout.toml"
    ) || lower.starts_with("tsconfig")
    {
        return Some(("project manifest or build configuration", 325.0));
    }
    None
}

fn round_score(score: f64) -> f64 {
    (score * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::GraphFileSignal;
    use crate::model::{Churn, Complexity, FunctionComplexity, Halstead};
    use std::collections::BTreeMap;

    fn file(path: &str, tokens: usize) -> FileReport {
        FileReport {
            path: PathBuf::from(path),
            language: "Rust".to_string(),
            bytes: 0,
            tokens,
            loc: 1,
            sloc: 1,
            comment_lines: 0,
            comment_ratio: 0.0,
            line_metrics_approximate: false,
            complexity: None,
            imports: Vec::new(),
            markers: BTreeMap::new(),
            marker_occurrences: Vec::new(),
            churn: None,
            approximate: false,
            symbols: None,
            skip_hint: None,
            has_inline_tests: false,
        }
    }

    #[test]
    fn selection_never_exceeds_token_or_file_budgets() {
        let files = [
            file("src/main.rs", 60),
            file("src/lib.rs", 40),
            file("src/extra.rs", 20),
        ];
        let cfg = Config {
            context: true,
            context_budget: 70,
            context_max_files: 2,
            ..Config::default()
        };

        let plan = build(
            &files,
            &[],
            &BTreeMap::new(),
            None,
            Path::new("/repo"),
            &cfg,
            None,
        );

        assert!(plan.selected_tokens <= 70);
        assert!(plan.files.len() <= 2);
        assert_eq!(plan.selected_tokens, 60);
        assert_eq!(
            plan.files
                .iter()
                .map(|file| file.path.as_path())
                .collect::<Vec<_>>(),
            [Path::new("src/lib.rs"), Path::new("src/extra.rs")]
        );
    }

    #[test]
    fn focus_prioritizes_direct_graph_neighbors_and_matching_tests() {
        let files = [
            file("src/focus.ts", 20),
            file("src/dependency.ts", 20),
            file("src/dependent.ts", 20),
            file("tests/focus.test.ts", 20),
            file("src/unrelated.ts", 20),
        ];
        let mut graph = GraphSignals {
            languages: vec!["TypeScript".to_string()],
            ..GraphSignals::default()
        };
        graph.files.insert(
            "src/focus.ts".to_string(),
            GraphFileSignal {
                dependencies: vec!["src/dependency.ts".to_string()],
                dependents: vec!["src/dependent.ts".to_string()],
                ..GraphFileSignal::default()
            },
        );
        let cfg = Config {
            context: true,
            context_budget: 80,
            context_max_files: 4,
            context_focus: vec![PathBuf::from("src/focus.ts")],
            ..Config::default()
        };

        let plan = build(
            &files,
            &[],
            &BTreeMap::new(),
            Some(&graph),
            Path::new("/repo"),
            &cfg,
            None,
        );
        let selected = plan
            .files
            .iter()
            .map(|file| file.path.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert_eq!(selected[0], "src/focus.ts");
        assert!(selected.contains(&"src/dependency.ts".to_string()));
        assert!(selected.contains(&"src/dependent.ts".to_string()));
        assert!(selected.contains(&"tests/focus.test.ts".to_string()));
        assert!(!selected.contains(&"src/unrelated.ts".to_string()));
    }

    #[test]
    fn focus_resolves_relative_to_a_nested_scan_target() {
        let files = [
            file("packages/app/math.rs", 20),
            file("packages/app/sibling.rs", 20),
        ];
        let cfg = Config {
            context: true,
            context_focus: vec![PathBuf::from("math.rs")],
            ..Config::default()
        };

        let plan = build_for_target(
            &files,
            &[],
            &BTreeMap::new(),
            None,
            PlanningPaths {
                root: Path::new("/repo"),
                target: Path::new("/repo/packages/app"),
            },
            &cfg,
            None,
        )
        .unwrap();

        assert_eq!(plan.focus, [PathBuf::from("packages/app/math.rs")]);
        assert!(plan.unmatched_focus.is_empty());
        assert_eq!(plan.files[0].path, PathBuf::from("packages/app/math.rs"));
    }

    #[test]
    fn unmatched_focus_is_reported_without_inventing_a_seed() {
        let files = [file("packages/app/math.rs", 20)];
        let cfg = Config {
            context: true,
            context_focus: vec![PathBuf::from("missing.rs")],
            ..Config::default()
        };

        let plan = build_for_target(
            &files,
            &[],
            &BTreeMap::new(),
            None,
            PlanningPaths {
                root: Path::new("/repo"),
                target: Path::new("/repo/packages/app"),
            },
            &cfg,
            None,
        )
        .unwrap();

        assert!(plan.focus.is_empty());
        assert_eq!(plan.unmatched_focus, [PathBuf::from("missing.rs")]);
        assert!(
            plan.files[0]
                .reasons
                .iter()
                .all(|reason| reason != "focus path")
        );
    }

    #[test]
    fn ambiguous_target_relative_focus_is_rejected() {
        let files = [file("math.rs", 20), file("packages/app/math.rs", 20)];
        let cfg = Config {
            context: true,
            context_focus: vec![PathBuf::from("math.rs")],
            ..Config::default()
        };

        let error = build_for_target(
            &files,
            &[],
            &BTreeMap::new(),
            None,
            PlanningPaths {
                root: Path::new("/repo"),
                target: Path::new("/repo/packages/app"),
            },
            &cfg,
            None,
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("ambiguous"), "error was: {error}");
        assert!(error.contains("math.rs"), "error was: {error}");
        assert!(error.contains("packages/app/math.rs"), "error was: {error}");
    }

    #[test]
    fn generated_files_are_skipped_unless_explicitly_focused() {
        let mut generated = file("src/generated.rs", 10);
        generated.skip_hint = Some("generated".to_string());
        let normal = file("src/lib.rs", 10);
        let mut cfg = Config {
            context: true,
            ..Config::default()
        };

        let plan = build(
            &[generated.clone(), normal.clone()],
            &[],
            &BTreeMap::new(),
            None,
            Path::new("/repo"),
            &cfg,
            None,
        );
        assert!(plan.files.iter().all(|file| file.path != generated.path));

        cfg.context_focus = vec![generated.path.clone()];
        let focused = build(
            &[generated, normal],
            &[],
            &BTreeMap::new(),
            None,
            Path::new("/repo"),
            &cfg,
            None,
        );
        assert_eq!(focused.files[0].path, PathBuf::from("src/generated.rs"));
    }

    #[test]
    fn focus_prefers_same_directory_when_graph_signals_are_unavailable() {
        let files = [
            file("src/focus.rs", 20),
            file("src/sibling.rs", 20),
            file("other/unrelated.rs", 20),
        ];
        let cfg = Config {
            context: true,
            context_budget: 40,
            context_max_files: 2,
            context_focus: vec![PathBuf::from("src/focus.rs")],
            ..Config::default()
        };

        let plan = build(
            &files,
            &[],
            &BTreeMap::new(),
            None,
            Path::new("/repo"),
            &cfg,
            None,
        );

        assert_eq!(plan.files[0].path, PathBuf::from("src/focus.rs"));
        assert_eq!(plan.files[1].path, PathBuf::from("src/sibling.rs"));
        assert!(
            plan.files[1]
                .reasons
                .contains(&"shares focus directory".to_string())
        );
    }

    #[test]
    fn repository_instructions_outrank_ordinary_focus_siblings() {
        let files = [
            file("src/focus.rs", 10),
            file("src/sibling.rs", 10),
            file("AGENTS.md", 10),
        ];
        let cfg = Config {
            context: true,
            context_budget: 20,
            context_max_files: 2,
            context_focus: vec![PathBuf::from("src/focus.rs")],
            ..Config::default()
        };

        let plan = build(
            &files,
            &[],
            &BTreeMap::new(),
            None,
            Path::new("/repo"),
            &cfg,
            None,
        );

        assert_eq!(plan.files[0].path, PathBuf::from("src/focus.rs"));
        assert_eq!(plan.files[1].path, PathBuf::from("AGENTS.md"));
        assert!(
            plan.files[1]
                .reasons
                .contains(&"repository instructions".to_string())
        );
    }

    #[test]
    fn risk_churn_and_complexity_produce_explainable_reasons() {
        let mut risky = file("src/risky.rs", 10);
        risky.churn = Some(Churn {
            commits: 9,
            authors: 2,
            ..Churn::default()
        });
        risky.complexity = Some(Complexity {
            cyclomatic: 15,
            cognitive: 0,
            max_nesting: 0,
            halstead: Halstead::default(),
            maintainability_index: 0.0,
            functions: Vec::new(),
        });
        let risk = RiskEntry {
            path: "src/risky.rs".to_string(),
            score: 0.8,
            ..RiskEntry::default()
        };
        let cfg = Config {
            context: true,
            ..Config::default()
        };

        let plan = build(
            &[risky],
            &[risk],
            &BTreeMap::new(),
            None,
            Path::new("/repo"),
            &cfg,
            None,
        );
        let reasons = &plan.files[0].reasons;

        assert!(reasons.iter().any(|reason| reason.contains("high risk")));
        assert!(
            reasons
                .iter()
                .any(|reason| reason.contains("active history"))
        );
        assert!(
            reasons
                .iter()
                .any(|reason| reason.contains("complex control"))
        );
    }

    #[test]
    fn change_seeds_select_dependencies_tests_and_transitive_dependents_with_evidence() {
        let files = [
            file("src/changed.ts", 10),
            file("src/dependency.ts", 10),
            file("src/direct.ts", 10),
            file("src/transitive.ts", 10),
            file("tests/changed.test.ts", 10),
            file("src/unrelated.ts", 10),
        ];
        let mut graph = GraphSignals {
            languages: vec!["TypeScript".to_string()],
            ..GraphSignals::default()
        };
        graph.files.insert(
            "src/changed.ts".to_string(),
            GraphFileSignal {
                dependencies: vec!["src/dependency.ts".to_string()],
                dependents: vec!["src/direct.ts".to_string()],
                dependency_resolvers: BTreeMap::from([(
                    "src/dependency.ts".to_string(),
                    "relative".to_string(),
                )]),
                dependent_resolvers: BTreeMap::from([(
                    "src/direct.ts".to_string(),
                    "relative".to_string(),
                )]),
                ..GraphFileSignal::default()
            },
        );
        graph.files.insert(
            "src/direct.ts".to_string(),
            GraphFileSignal {
                dependents: vec!["src/transitive.ts".to_string()],
                dependent_resolvers: BTreeMap::from([(
                    "src/transitive.ts".to_string(),
                    "package-exports".to_string(),
                )]),
                ..GraphFileSignal::default()
            },
        );
        let changes = ChangeSeeds {
            scope: "working".to_string(),
            paths: HashSet::from([PathBuf::from("src/changed.ts")]),
        };
        let cfg = Config {
            context: true,
            context_budget: 50,
            context_max_files: 5,
            ..Config::default()
        };

        let plan = build(
            &files,
            &[],
            &BTreeMap::new(),
            Some(&graph),
            Path::new("/repo"),
            &cfg,
            Some(&changes),
        );
        let selected = plan
            .files
            .iter()
            .map(|file| file.path.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert_eq!(plan.change_scope.as_deref(), Some("working"));
        assert_eq!(plan.changed_files, [PathBuf::from("src/changed.ts")]);
        assert_eq!(selected[0], "src/changed.ts");
        for expected in [
            "src/dependency.ts",
            "src/direct.ts",
            "src/transitive.ts",
            "tests/changed.test.ts",
        ] {
            assert!(
                selected.contains(&expected.to_string()),
                "missing {expected}"
            );
        }
        assert!(!selected.contains(&"src/unrelated.ts".to_string()));

        let evidence = |path: &str, role: &str| {
            plan.files
                .iter()
                .find(|file| file.path == Path::new(path))
                .and_then(|file| file.evidence.iter().find(|item| item.role == role))
                .cloned()
                .unwrap_or_else(|| panic!("missing {role} evidence for {path}"))
        };
        let dependency = evidence("src/dependency.ts", "dependency");
        assert_eq!(dependency.confidence, "high");
        assert_eq!(dependency.resolver.as_deref(), Some("relative"));
        let direct = evidence("src/direct.ts", "dependent");
        assert_eq!(direct.confidence, "high");
        assert_eq!(direct.distance, Some(1));
        let transitive = evidence("src/transitive.ts", "dependent");
        assert_eq!(transitive.confidence, "partial");
        assert_eq!(transitive.distance, Some(2));
        assert_eq!(
            evidence("tests/changed.test.ts", "matching-test").confidence,
            "partial"
        );
    }

    #[test]
    fn structural_outlines_have_a_hard_payload_bound_and_report_omissions() {
        let files = [file("src/lib.rs", 10)];
        let symbols = (0..30)
            .map(|index| SymbolOutline {
                name: format!("PublicType{index}"),
                kind: "type".to_string(),
                signature: format!("pub struct PublicType{index}<T: LongTraitName>"),
                line: index + 1,
                exported: true,
                reasons: vec!["exported/public declaration".to_string()],
            })
            .collect::<Vec<_>>();
        let outlines = BTreeMap::from([(PathBuf::from("src/lib.rs"), symbols)]);
        let cfg = Config {
            context: true,
            context_budget: 100,
            context_max_files: 1,
            ..Config::default()
        };

        let plan = build(&files, &[], &outlines, None, Path::new("/repo"), &cfg, None);
        let retained = &plan.files[0].symbols;
        let measured = retained.iter().map(outline_payload_bytes).sum::<usize>();

        assert!(!retained.is_empty());
        assert!(retained.len() <= MAX_OUTLINE_SYMBOLS_PER_FILE);
        assert!(measured <= MAX_OUTLINE_BYTES_PER_FILE);
        assert_eq!(plan.outline_symbols, retained.len());
        assert_eq!(plan.outline_bytes, measured);
        assert!(plan.outline_omitted_symbols > 0);
        assert!(
            retained
                .iter()
                .all(|symbol| !symbol.reasons.is_empty() && !symbol.signature.contains('{'))
        );
    }

    #[test]
    fn oversized_focus_retains_a_bounded_outline_without_spending_source_tokens() {
        let files = [file("src/large.rs", 500)];
        let outlines = BTreeMap::from([(
            PathBuf::from("src/large.rs"),
            vec![SymbolOutline {
                name: "ImportantType".to_string(),
                kind: "struct".to_string(),
                signature: "pub struct ImportantType".to_string(),
                line: 1,
                exported: true,
                reasons: Vec::new(),
            }],
        )]);
        let cfg = Config {
            context: true,
            context_budget: 100,
            context_focus: vec![PathBuf::from("src/large.rs")],
            ..Config::default()
        };

        let plan = build(&files, &[], &outlines, None, Path::new("/repo"), &cfg, None);

        assert!(plan.files.is_empty());
        assert_eq!(plan.selected_tokens, 0);
        assert_eq!(plan.outline_only.len(), 1);
        assert_eq!(plan.outline_only[0].path, PathBuf::from("src/large.rs"));
        assert_eq!(plan.outline_only[0].source_tokens, 500);
        assert_eq!(plan.outline_only[0].symbols[0].name, "ImportantType");
        assert_eq!(plan.outline_symbols, 1);
        assert!(plan.outline_bytes > 0);
        assert_eq!(plan.omitted[0].reason, "file exceeds total token budget");
    }

    #[test]
    fn change_plans_prefer_nearby_risk_over_ordinary_siblings() {
        let files = [
            file("src/changed.rs", 10),
            file("src/nearby_risk.rs", 10),
            file("src/plain.rs", 10),
            file("other/distant_risk.rs", 10),
        ];
        let risks = [
            RiskEntry {
                path: "src/nearby_risk.rs".to_string(),
                score: 0.8,
                ..RiskEntry::default()
            },
            RiskEntry {
                path: "other/distant_risk.rs".to_string(),
                score: 0.8,
                ..RiskEntry::default()
            },
        ];
        let changes = ChangeSeeds {
            scope: "working".to_string(),
            paths: HashSet::from([PathBuf::from("src/changed.rs")]),
        };
        let cfg = Config {
            context: true,
            context_budget: 20,
            context_max_files: 2,
            ..Config::default()
        };

        let plan = build(
            &files,
            &risks,
            &BTreeMap::new(),
            None,
            Path::new("/repo"),
            &cfg,
            Some(&changes),
        );

        assert_eq!(plan.files[0].path, PathBuf::from("src/changed.rs"));
        assert_eq!(plan.files[1].path, PathBuf::from("src/nearby_risk.rs"));
        assert!(
            plan.files[1]
                .reasons
                .contains(&"nearby elevated-risk code".to_string())
        );
    }

    #[test]
    fn complexity_reasons_attach_only_to_the_matching_callable_outline() {
        let mut source = file("src/service.ts", 10);
        source.complexity = Some(Complexity {
            functions: vec![FunctionComplexity {
                name: "run".to_string(),
                line: 1,
                cyclomatic: 30,
                ..FunctionComplexity::default()
            }],
            ..Complexity::default()
        });
        let outlines = BTreeMap::from([(
            PathBuf::from("src/service.ts"),
            vec![
                SymbolOutline {
                    name: "Service".to_string(),
                    kind: "class".to_string(),
                    line: 1,
                    exported: true,
                    ..SymbolOutline::default()
                },
                SymbolOutline {
                    name: "Service.run".to_string(),
                    kind: "method".to_string(),
                    line: 1,
                    exported: true,
                    ..SymbolOutline::default()
                },
            ],
        )]);
        let cfg = Config {
            context: true,
            max_complexity: 20,
            ..Config::default()
        };

        let plan = build(
            &[source],
            &[],
            &outlines,
            None,
            Path::new("/repo"),
            &cfg,
            None,
        );
        let symbols = &plan.files[0].symbols;
        let class = symbols
            .iter()
            .find(|symbol| symbol.name == "Service")
            .unwrap();
        let method = symbols
            .iter()
            .find(|symbol| symbol.name == "Service.run")
            .unwrap();

        assert!(!symbol_has_finding(class));
        assert!(symbol_has_finding(method));
    }

    #[test]
    fn evidence_confidence_distinguishes_configured_and_inferred_resolvers() {
        for resolver in [
            "relative",
            "python-relative",
            "tsconfig-paths",
            "package-exports",
            "composer-psr-4",
            "composer-psr-0",
            "php-include",
        ] {
            assert_eq!(
                graph_evidence("dependency", 1, Some(resolver.to_string())).confidence,
                "high",
                "{resolver} should be precise"
            );
        }
        for resolver in [
            "python-absolute",
            "python-src-root",
            "package-subpath",
            "package-index",
            "heuristic-alias",
            "php-namespace-heuristic",
        ] {
            assert_eq!(
                graph_evidence("dependency", 1, Some(resolver.to_string())).confidence,
                "partial",
                "{resolver} should remain heuristic"
            );
        }
        assert_eq!(
            graph_evidence("dependent", 2, Some("relative".to_string())).confidence,
            "partial"
        );
    }
}
