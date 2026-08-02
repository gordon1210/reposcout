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
use crate::numeric::usize_to_f64;
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

struct PlanningSeeds {
    focus: Vec<PathBuf>,
    unmatched_focus: Vec<PathBuf>,
    explicit_files: HashSet<String>,
    changed_files: HashSet<String>,
    seed_files: HashSet<String>,
    focus_directories: HashSet<String>,
    focus_source_stems: HashSet<String>,
}

struct GraphNeighborhood {
    dependencies: HashMap<String, Option<String>>,
    dependents: HashMap<String, GraphReach>,
    direct_dependents: usize,
    transitive_dependents: usize,
    graph_eligible_seed_files: usize,
    graph_covered_seed_files: usize,
}

struct CandidateCollection {
    candidates: Vec<Candidate>,
    skipped_files: usize,
    matching_tests: usize,
}

struct CandidateSelection {
    selected_tokens: usize,
    selected: Vec<ContextFile>,
    outline_only: Vec<(ContextFile, String)>,
    omitted: Vec<ContextOmission>,
    omitted_files: usize,
    omitted_tokens: usize,
}

struct CandidateScore {
    value: f64,
    reasons: Vec<String>,
    evidence: Vec<ContextEvidence>,
}

struct CandidateInputs<'a> {
    seeds: &'a PlanningSeeds,
    neighborhood: &'a GraphNeighborhood,
    graph: Option<&'a GraphSignals>,
    changes: Option<&'a ChangeSeeds>,
    risk_by_path: &'a HashMap<&'a str, &'a RiskEntry>,
}

#[derive(Clone, Copy)]
enum SeedRelation {
    Neither,
    Focus,
    Change,
    FocusAndChange,
}

impl SeedRelation {
    fn new(focused: bool, changed: bool) -> Self {
        match (focused, changed) {
            (false, false) => Self::Neither,
            (true, false) => Self::Focus,
            (false, true) => Self::Change,
            (true, true) => Self::FocusAndChange,
        }
    }

    fn focused(self) -> bool {
        matches!(self, Self::Focus | Self::FocusAndChange)
    }

    fn changed(self) -> bool {
        matches!(self, Self::Change | Self::FocusAndChange)
    }
}

struct CandidateFacts<'a> {
    path: &'a str,
    seed: SeedRelation,
    dependency: Option<&'a Option<String>>,
    dependent: Option<&'a GraphReach>,
    shares_focus_directory: bool,
    matching_test: bool,
    support: Option<(&'static str, f64)>,
    risk: Option<&'a RiskEntry>,
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

#[derive(Clone, Copy)]
pub(crate) struct PlanningPaths<'a> {
    pub root: &'a Path,
    pub target: &'a Path,
}

fn planning_seeds(
    files: &[FileReport],
    paths: PlanningPaths<'_>,
    configured_focus: &[PathBuf],
    changes: Option<&ChangeSeeds>,
) -> Result<PlanningSeeds> {
    let (focus, unmatched_focus) =
        resolve_focus(configured_focus, files, paths.root, paths.target)?;
    let focus_keys = focus
        .iter()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .collect::<Vec<_>>();
    let explicit_files = files
        .iter()
        .filter(|file| matches_focus(&path_key(&file.path), &focus_keys))
        .map(|file| path_key(&file.path))
        .collect::<HashSet<_>>();
    let changed_keys: HashSet<String> = changes
        .map(|changes| changes.paths.iter().map(|path| path_key(path)).collect())
        .unwrap_or_default();
    let changed_files = files
        .iter()
        .filter(|file| changed_keys.contains(&path_key(&file.path)))
        .map(|file| path_key(&file.path))
        .collect::<HashSet<_>>();
    let seed_files = explicit_files
        .union(&changed_keys)
        .cloned()
        .collect::<HashSet<_>>();
    let focus_directories = seed_files
        .iter()
        .filter_map(|path| Path::new(path).parent())
        .map(path_key)
        .collect::<HashSet<_>>();
    let focus_source_stems = files
        .iter()
        .filter(|file| seed_files.contains(&path_key(&file.path)))
        .filter(|file| {
            lang::detect(&file.path).is_some_and(lang::LangInfo::is_code)
                && !testcov::is_test_file(&path_key(&file.path))
        })
        .map(|file| testcov::source_stem(&path_key(&file.path)))
        .chain(
            changed_keys
                .iter()
                .filter(|path| {
                    lang::detect(Path::new(path)).is_some_and(lang::LangInfo::is_code)
                        && !testcov::is_test_file(path)
                })
                .map(|path| testcov::source_stem(path)),
        )
        .collect();

    Ok(PlanningSeeds {
        focus,
        unmatched_focus,
        explicit_files,
        changed_files,
        seed_files,
        focus_directories,
        focus_source_stems,
    })
}

fn graph_neighborhood(
    graph: Option<&GraphSignals>,
    seed_files: &HashSet<String>,
) -> GraphNeighborhood {
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
    let dependents = dependent_reach(graph, seed_files);
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
        .filter(|path| lang::detect(Path::new(path)).is_some_and(lang::LangInfo::is_first_class))
        .count();
    let graph_covered_seed_files = graph.map_or(0, |graph| {
        seed_files
            .iter()
            .filter(|path| graph.files.contains_key(path.as_str()))
            .count()
    });

    GraphNeighborhood {
        dependencies,
        dependents,
        direct_dependents,
        transitive_dependents,
        graph_eligible_seed_files,
        graph_covered_seed_files,
    }
}

fn collect_candidates(
    files: &[FileReport],
    risks: &[RiskEntry],
    graph: Option<&GraphSignals>,
    seeds: &PlanningSeeds,
    neighborhood: &GraphNeighborhood,
    changes: Option<&ChangeSeeds>,
) -> CandidateCollection {
    let risk_by_path = risks
        .iter()
        .map(|risk| (risk.path.as_str(), risk))
        .collect::<HashMap<_, _>>();
    let inputs = CandidateInputs {
        seeds,
        neighborhood,
        graph,
        changes,
        risk_by_path: &risk_by_path,
    };
    let mut skipped_files = 0usize;
    let mut matching_tests = 0usize;
    let mut candidates = Vec::new();
    for file in files {
        let (candidate, matching_test) = build_candidate(file, &inputs);
        matching_tests = matching_tests.saturating_add(usize::from(matching_test));
        match candidate {
            Some(candidate) => candidates.push(candidate),
            None => skipped_files = skipped_files.saturating_add(1),
        }
    }
    CandidateCollection {
        candidates,
        skipped_files,
        matching_tests,
    }
}

fn build_candidate(file: &FileReport, inputs: &CandidateInputs<'_>) -> (Option<Candidate>, bool) {
    let path = path_key(&file.path);
    let focused = inputs.seeds.explicit_files.contains(&path);
    let changed = inputs.seeds.changed_files.contains(&path);
    let dependency = inputs.neighborhood.dependencies.get(&path);
    let dependent = inputs.neighborhood.dependents.get(&path);
    let shares_focus_directory = !focused
        && !changed
        && Path::new(&path)
            .parent()
            .is_some_and(|parent| inputs.seeds.focus_directories.contains(&path_key(parent)));
    let matching_test = !inputs.seeds.focus_source_stems.is_empty()
        && testcov::is_test_file(&path)
        && testcov::test_stem_keys(&path)
            .iter()
            .any(|key| inputs.seeds.focus_source_stems.contains(key));
    let is_code = lang::detect(&file.path).is_some_and(lang::LangInfo::is_code);
    let support = support_role(&path);
    let risk = inputs.risk_by_path.get(path.as_str()).copied();

    if file.skip_hint.is_some() && !focused && !changed {
        return (None, matching_test);
    }
    if !is_code
        && support.is_none()
        && !focused
        && !changed
        && dependency.is_none()
        && dependent.is_none()
        && !matching_test
    {
        return (None, matching_test);
    }

    let facts = CandidateFacts {
        path: &path,
        seed: SeedRelation::new(focused, changed),
        dependency,
        dependent,
        shares_focus_directory,
        matching_test,
        support,
        risk,
    };
    let mut score = CandidateScore {
        value: if is_code { 10.0 } else { 0.0 },
        reasons: Vec::new(),
        evidence: Vec::new(),
    };
    score_relationships(&mut score, file, &facts, inputs.changes);
    score_file_signals(&mut score, file, &facts, inputs.graph);
    if score.reasons.is_empty() {
        score.reasons.push(if testcov::is_test_file(&path) {
            "test source".to_string()
        } else {
            "representative source file".to_string()
        });
    }

    (
        Some(Candidate {
            path: file.path.clone(),
            path_key: path,
            tokens: file.tokens,
            score: round_score(score.value),
            reasons: score.reasons,
            evidence: score.evidence,
        }),
        matching_test,
    )
}

fn score_relationships(
    score: &mut CandidateScore,
    file: &FileReport,
    facts: &CandidateFacts<'_>,
    changes: Option<&ChangeSeeds>,
) {
    if facts.seed.focused() {
        score.value += 1_200.0;
        score.reasons.push("focus path".to_string());
        if let Some(hint) = &file.skip_hint {
            score
                .reasons
                .push(format!("focus overrides {hint} skip hint"));
        }
    }
    if facts.seed.changed() {
        score.value += 1_100.0;
        let change_scope = changes.map_or("diff", |changes| changes.scope.as_str());
        score
            .reasons
            .push(format!("changed in {change_scope} scope"));
        score.evidence.push(ContextEvidence {
            role: "changed".to_string(),
            confidence: "high".to_string(),
            distance: Some(0),
            resolver: None,
        });
        if let Some(hint) = &file.skip_hint {
            score
                .reasons
                .push(format!("change overrides {hint} skip hint"));
        }
    }
    if let Some(resolver) = facts.dependency {
        score.value += 600.0;
        score.reasons.push(if changes.is_some() {
            "direct dependency of change".to_string()
        } else {
            "direct dependency of focus".to_string()
        });
        score
            .evidence
            .push(graph_evidence("dependency", 1, resolver.clone()));
    }
    score_dependents(score, facts, changes);
    score_nearby_and_tests(score, facts, changes);
}

fn score_dependents(
    score: &mut CandidateScore,
    facts: &CandidateFacts<'_>,
    changes: Option<&ChangeSeeds>,
) {
    let Some(reach) = facts.dependent else {
        return;
    };
    if reach.distance == 1 {
        score.value += 550.0;
        score.reasons.push(if changes.is_some() {
            "direct dependent of change".to_string()
        } else {
            "direct dependent of focus".to_string()
        });
    } else {
        score.value += 425.0 / usize_to_f64(reach.distance);
        let seed = if changes.is_some() { "change" } else { "focus" };
        score.reasons.push(format!(
            "transitive dependent of {seed} ({} hops)",
            reach.distance
        ));
    }
    score.evidence.push(graph_evidence(
        "dependent",
        reach.distance,
        reach.resolver.clone(),
    ));
}

fn score_nearby_and_tests(
    score: &mut CandidateScore,
    facts: &CandidateFacts<'_>,
    changes: Option<&ChangeSeeds>,
) {
    if facts.matching_test {
        score.value += 500.0;
        score.reasons.push(if changes.is_some() {
            "matching test for change".to_string()
        } else {
            "matching test for focus".to_string()
        });
        score.evidence.push(ContextEvidence {
            role: "matching-test".to_string(),
            confidence: "partial".to_string(),
            distance: None,
            resolver: None,
        });
    }
    if facts.shares_focus_directory {
        let elevated_nearby_risk =
            changes.is_some() && facts.risk.is_some_and(|risk| risk.score >= 0.4);
        score.value += if changes.is_some() && !elevated_nearby_risk {
            75.0
        } else {
            250.0
        };
        score.reasons.push(if elevated_nearby_risk {
            "nearby elevated-risk code".to_string()
        } else if changes.is_some() {
            "shares changed-file directory".to_string()
        } else {
            "shares focus directory".to_string()
        });
        score.evidence.push(ContextEvidence {
            role: "nearby".to_string(),
            confidence: "partial".to_string(),
            distance: None,
            resolver: None,
        });
    }
}

fn score_file_signals(
    score: &mut CandidateScore,
    file: &FileReport,
    facts: &CandidateFacts<'_>,
    graph: Option<&GraphSignals>,
) {
    if let Some((role, weight)) = facts.support {
        score.value += weight;
        if !facts.path.contains('/') {
            score.value += 100.0;
        }
        score.reasons.push(role.to_string());
    }
    if is_entrypoint(facts.path) {
        score.value += 300.0;
        score.reasons.push("entrypoint".to_string());
    }
    if let Some(signal) = graph.and_then(|graph| graph.files.get(facts.path)) {
        if signal.fan_in > 0 {
            score.value += usize_to_f64(signal.fan_in.min(20) * 18);
            score
                .reasons
                .push(format!("depended on by {} graph files", signal.fan_in));
        }
        if signal.fan_out > 0 {
            score.value += usize_to_f64(signal.fan_out.min(20) * 5);
            score
                .reasons
                .push(format!("connects {} internal dependencies", signal.fan_out));
        }
    }
    if let Some(risk) = facts.risk {
        score.value += risk.score * 200.0;
        if risk.score >= 0.7 {
            score.reasons.push(format!("high risk ({:.2})", risk.score));
        } else if risk.score >= 0.4 {
            score
                .reasons
                .push(format!("elevated risk ({:.2})", risk.score));
        }
    }
    if let Some(churn) = &file.churn
        && churn.commits >= 5
    {
        score.value += usize_to_f64(churn.commits.min(20)) * 4.0;
        score
            .reasons
            .push(format!("active history ({} commits)", churn.commits));
    }
    if let Some(complexity) = &file.complexity
        && complexity.cyclomatic >= 10
    {
        score.value += f64::from(complexity.cyclomatic.min(100)) * 1.5;
        score
            .reasons
            .push(format!("complex control flow ({})", complexity.cyclomatic));
    }
}

fn select_candidates(
    mut candidates: Vec<Candidate>,
    cfg: &Config,
    seed_files: &HashSet<String>,
) -> (usize, CandidateSelection) {
    candidates.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.tokens.cmp(&right.tokens))
            .then_with(|| left.path_key.cmp(&right.path_key))
    });
    let candidate_files = candidates.len();
    let mut selection = CandidateSelection {
        selected_tokens: 0,
        selected: Vec::new(),
        outline_only: Vec::new(),
        omitted: Vec::new(),
        omitted_files: 0,
        omitted_tokens: 0,
    };

    for candidate in candidates {
        let reason = omission_reason(&candidate, &selection, cfg);
        if let Some(reason) = reason {
            selection.omitted_files = selection.omitted_files.saturating_add(1);
            selection.omitted_tokens = selection.omitted_tokens.saturating_add(candidate.tokens);
            if seed_files.contains(&candidate.path_key) {
                selection
                    .outline_only
                    .push((context_file(&candidate), reason.to_string()));
            }
            if selection.omitted.len() < MAX_OMISSION_DETAILS {
                selection.omitted.push(ContextOmission {
                    path: candidate.path,
                    tokens: candidate.tokens,
                    reason: reason.to_string(),
                });
            }
            continue;
        }
        selection.selected_tokens = selection.selected_tokens.saturating_add(candidate.tokens);
        selection.selected.push(context_file(&candidate));
    }
    (candidate_files, selection)
}

fn omission_reason<'a>(
    candidate: &Candidate,
    selection: &CandidateSelection,
    cfg: &'a Config,
) -> Option<&'a str> {
    if selection.selected.len() >= cfg.context_max_files {
        Some("file limit reached")
    } else if candidate.tokens > cfg.context_budget {
        Some("file exceeds total token budget")
    } else if selection.selected_tokens.saturating_add(candidate.tokens) > cfg.context_budget {
        Some("does not fit remaining token budget")
    } else {
        None
    }
}

fn context_file(candidate: &Candidate) -> ContextFile {
    ContextFile {
        path: candidate.path.clone(),
        tokens: candidate.tokens,
        score: candidate.score,
        reasons: candidate.reasons.clone(),
        evidence: candidate.evidence.clone(),
        symbols: Vec::new(),
    }
}

fn attach_selection_outlines(
    selection: &mut CandidateSelection,
    files: &[FileReport],
    outlines: &BTreeMap<PathBuf, Vec<SymbolOutline>>,
    max_complexity: u32,
) -> (Vec<ContextOutlineOnly>, OutlineStats) {
    let mut stats = OutlineStats::default();
    attach_outlines(
        &mut selection.selected,
        files,
        outlines,
        max_complexity,
        &mut stats,
    );
    let mut outline_only_files = selection
        .outline_only
        .iter()
        .map(|(file, _)| file.clone())
        .collect::<Vec<_>>();
    attach_outlines(
        &mut outline_only_files,
        files,
        outlines,
        max_complexity,
        &mut stats,
    );
    let outline_only = std::mem::take(&mut selection.outline_only)
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
        .collect();
    (outline_only, stats)
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
    let seeds = planning_seeds(files, paths, &cfg.context_focus, changes)?;
    let neighborhood = graph_neighborhood(graph, &seeds.seed_files);
    let CandidateCollection {
        candidates,
        skipped_files,
        matching_tests,
    } = collect_candidates(files, risks, graph, &seeds, &neighborhood, changes);
    let (candidate_files, mut selection) = select_candidates(candidates, cfg, &seeds.seed_files);
    let (outline_only, outline_stats) =
        attach_selection_outlines(&mut selection, files, outlines, cfg.max_complexity);

    let mut changed_paths = changes
        .map(|changes| changes.paths.iter().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    changed_paths.sort();

    Ok(ContextPlan {
        strategy_version: STRATEGY_VERSION,
        planning_ms: 0,
        budget_tokens: cfg.context_budget,
        selected_tokens: selection.selected_tokens,
        candidate_files,
        omitted_files: selection.omitted_files,
        omitted_tokens: selection.omitted_tokens,
        skipped_files,
        seed_files: seeds.seed_files.len(),
        graph_eligible_seed_files: neighborhood.graph_eligible_seed_files,
        graph_covered_seed_files: neighborhood.graph_covered_seed_files,
        direct_dependents: neighborhood.direct_dependents,
        transitive_dependents: neighborhood.transitive_dependents,
        matching_tests,
        focus: seeds.focus,
        unmatched_focus: seeds.unmatched_focus,
        change_scope: changes.map(|changes| changes.scope.clone()),
        changed_files: changed_paths,
        graph_languages: graph
            .map(|graph| graph.languages.clone())
            .unwrap_or_default(),
        graph_unresolved_imports: graph.map_or(0, |graph| graph.unresolved_imports),
        graph_parse_errors: graph.map_or(0, |graph| graph.parse_errors),
        graph_config_errors: graph.map_or(0, |graph| graph.config_errors),
        outline_symbols: outline_stats.symbols,
        outline_bytes: outline_stats.bytes,
        outline_omitted_symbols: outline_stats.omitted_symbols,
        outline_details_omitted: false,
        planning_diagnostics: None,
        files: selection.selected,
        outline_only,
        omitted: selection.omitted,
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
    serde_json::to_vec(symbol).map_or_else(
        |_| {
            symbol.name.len()
                + symbol.kind.len()
                + symbol.signature.len()
                + symbol.reasons.iter().map(String::len).sum::<usize>()
        },
        |payload| payload.len(),
    )
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
mod tests;
