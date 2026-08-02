//! Compact raw work-scope evidence projected from analysis that already ran.
//!
//! This module must remain projection-only: it may summarize scanner, context,
//! impact, and graph facts, but it must not read source or trigger analyzers.

use crate::graph::GraphSignals;
use crate::lang;
use crate::model::{
    ContextPlan, ImpactAnalysis, ScanDiagnostics, Summary, WorkScope, WorkScopeChanges,
    WorkScopeComponent, WorkScopeConfidence, WorkScopeContext, WorkScopeCoverage, WorkScopeFocus,
    WorkScopeImpact, WorkScopeInventory, WorkScopeSeeds, WorkScopeStructure,
};
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

pub(crate) const STRATEGY_VERSION: u32 = 2;
pub(crate) const MAX_PATH_ENTRIES: usize = 25;
pub(crate) const MAX_COMPONENTS: usize = 10;

pub(crate) struct Inputs<'a> {
    pub summary: &'a Summary,
    pub diagnostics: &'a ScanDiagnostics,
    pub context: Option<&'a ContextPlan>,
    pub graph: Option<&'a GraphSignals>,
    pub impact: Option<&'a ImpactAnalysis>,
    pub diff_scope: Option<&'a str>,
    pub changed: &'a HashSet<PathBuf>,
}

pub(crate) fn build(inputs: &Inputs<'_>) -> WorkScope {
    let mut path_budget = PathBudget::new(MAX_PATH_ENTRIES);
    let focus = project_focus(inputs.context, &mut path_budget);
    let changes = project_changes(inputs.diff_scope, inputs.changed, &mut path_budget);
    let paths_omitted = focus
        .as_ref()
        .map_or(0, |focus| focus.omitted)
        .saturating_add(changes.as_ref().map_or(0, |changes| changes.omitted));
    let seeds = (focus.is_some() || changes.is_some()).then_some(WorkScopeSeeds {
        focus,
        changes,
        paths_omitted,
    });

    let mut basis = vec![if inputs.diff_scope.is_some() {
        "diff".to_string()
    } else {
        "repository".to_string()
    }];
    if seeds
        .as_ref()
        .and_then(|seeds| seeds.focus.as_ref())
        .is_some()
    {
        basis.push("focus".to_string());
    }

    let context = inputs.context.map(project_context);
    let impact = project_impact(inputs.context, inputs.impact, inputs.changed);
    let graph_seed_paths = graph_seed_paths(inputs.context, inputs.changed, inputs.graph);
    let structure = inputs
        .graph
        .map(|graph| project_structure(graph, &graph_seed_paths, &mut path_budget));
    let graph_unresolved_imports = inputs.graph.map_or_else(
        || {
            inputs.context.map_or_else(
                || inputs.impact.map_or(0, |impact| impact.unresolved_imports),
                |context| context.graph_unresolved_imports,
            )
        },
        |graph| graph.unresolved_imports,
    );
    let graph_parse_errors = inputs.graph.map_or_else(
        || {
            inputs.context.map_or_else(
                || inputs.impact.map_or(0, |impact| impact.parse_errors),
                |context| context.graph_parse_errors,
            )
        },
        |graph| graph.parse_errors,
    );
    let graph_config_errors = inputs.graph.map_or_else(
        || {
            inputs.context.map_or_else(
                || inputs.impact.map_or(0, |impact| impact.config_errors),
                |context| context.graph_config_errors,
            )
        },
        |graph| graph.config_errors,
    );

    WorkScope {
        strategy_version: STRATEGY_VERSION,
        basis,
        diff_scope: inputs.diff_scope.map(str::to_string),
        inventory: WorkScopeInventory {
            discovery_files: inputs.diagnostics.discovered_files,
            primary_files: inputs.diagnostics.analyzed_files,
            source_files: inputs.summary.source.files,
            source_tokens: inputs.summary.source.tokens,
        },
        production_duplication: inputs.summary.assessment.production_duplication.clone(),
        seeds,
        context,
        impact,
        structure,
        confidence: WorkScopeConfidence {
            primary: project_coverage(inputs.diagnostics, inputs.diff_scope.is_some()),
            planning_universe: inputs
                .context
                .and_then(|context| context.planning_diagnostics.as_ref())
                .map(|diagnostics| project_coverage(diagnostics, false)),
            graph_unresolved_imports,
            graph_parse_errors,
            graph_config_errors,
            type2_analysis_partial: inputs.diagnostics.type2_analysis_partial,
            unavailable_signals: inputs.summary.assessment.unavailable_signals.clone(),
        },
    }
}

fn project_focus(
    context: Option<&ContextPlan>,
    path_budget: &mut PathBudget,
) -> Option<WorkScopeFocus> {
    let context = context?;
    if context.focus.is_empty() && context.unmatched_focus.is_empty() {
        return None;
    }
    let resolved = normalized_paths(context.focus.iter());
    let unmatched = normalized_paths(context.unmatched_focus.iter());
    let total = resolved.len().saturating_add(unmatched.len());
    let paths = path_budget.take(resolved);
    let unmatched_paths = path_budget.take(unmatched);
    let shown = paths.len().saturating_add(unmatched_paths.len());
    Some(WorkScopeFocus {
        total,
        resolved: context.focus.len(),
        unresolved: context.unmatched_focus.len(),
        shown,
        omitted: total.saturating_sub(shown),
        paths,
        unmatched_paths,
    })
}

fn project_changes(
    diff_scope: Option<&str>,
    changed: &HashSet<PathBuf>,
    path_budget: &mut PathBudget,
) -> Option<WorkScopeChanges> {
    let scope = diff_scope?;
    let paths = normalized_paths(changed.iter());
    let total = paths.len();
    let paths = path_budget.take(paths);
    Some(WorkScopeChanges {
        scope: scope.to_string(),
        total,
        shown: paths.len(),
        omitted: total.saturating_sub(paths.len()),
        paths,
    })
}

fn project_context(context: &ContextPlan) -> WorkScopeContext {
    WorkScopeContext {
        budget_tokens: context.budget_tokens,
        selected_files: context.files.len(),
        selected_tokens: context.selected_tokens,
        candidate_files: context.candidate_files,
        outline_only_files: context.outline_only.len(),
        outline_only_tokens: context
            .outline_only
            .iter()
            .map(|file| file.source_tokens)
            .sum(),
        outline_symbols: context.outline_symbols,
        outline_bytes: context.outline_bytes,
        outline_omitted_symbols: context.outline_omitted_symbols,
        omitted_files: context.omitted_files,
        omitted_tokens: context.omitted_tokens,
        skipped_files: context.skipped_files,
        truncated: context.omitted_files > 0 || context.outline_omitted_symbols > 0,
    }
}

fn project_impact(
    context: Option<&ContextPlan>,
    impact: Option<&ImpactAnalysis>,
    changed: &HashSet<PathBuf>,
) -> Option<WorkScopeImpact> {
    if let Some(context) = context
        && context.seed_files > 0
    {
        return Some(WorkScopeImpact {
            seed_files: context.seed_files,
            graph_eligible_seed_files: context.graph_eligible_seed_files,
            graph_covered_seed_files: context.graph_covered_seed_files,
            direct_dependents: context.direct_dependents,
            transitive_dependents: context.transitive_dependents,
            matching_tests: context.matching_tests,
            matching_tests_known: true,
        });
    }
    let impact = impact?;
    Some(WorkScopeImpact {
        seed_files: changed.len(),
        graph_eligible_seed_files: changed
            .iter()
            .filter(|path| lang::detect(path).is_some_and(lang::LangInfo::is_first_class))
            .count(),
        graph_covered_seed_files: impact.graph_changed_files.len(),
        direct_dependents: impact.direct_dependents.len(),
        transitive_dependents: impact.transitive_dependents.len(),
        matching_tests: 0,
        matching_tests_known: false,
    })
}

fn project_structure(
    graph: &GraphSignals,
    seeds: &HashSet<String>,
    path_budget: &mut PathBudget,
) -> WorkScopeStructure {
    let mut ordered_paths = graph.files.keys().cloned().collect::<Vec<_>>();
    ordered_paths.sort();
    let mut visited = HashSet::with_capacity(ordered_paths.len());
    let mut components = Vec::<ComponentFacts>::new();
    for start in ordered_paths {
        if !visited.insert(start.clone()) {
            continue;
        }
        let mut queue = VecDeque::from([start]);
        let mut paths = Vec::new();
        while let Some(path) = queue.pop_front() {
            paths.push(path.clone());
            let Some(signal) = graph.files.get(&path) else {
                continue;
            };
            for neighbor in signal.dependencies.iter().chain(&signal.dependents) {
                if graph.files.contains_key(neighbor) && visited.insert(neighbor.clone()) {
                    queue.push_back(neighbor.clone());
                }
            }
        }
        paths.sort();
        let seed_paths = paths
            .iter()
            .filter(|path| seeds.contains(path.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        components.push(ComponentFacts { paths, seed_paths });
    }
    components.sort_by(|left, right| {
        right
            .seed_paths
            .len()
            .cmp(&left.seed_paths.len())
            .then_with(|| right.paths.len().cmp(&left.paths.len()))
            .then_with(|| left.paths.first().cmp(&right.paths.first()))
    });

    let total = components.len();
    let largest_component_files = components
        .iter()
        .map(|component| component.paths.len())
        .max()
        .unwrap_or(0);
    let has_seeded_components = components
        .iter()
        .any(|component| !component.seed_paths.is_empty());
    let entries = components
        .iter()
        .filter(|component| !has_seeded_components || !component.seed_paths.is_empty())
        .take(MAX_COMPONENTS)
        .map(|component| {
            let representatives = if component.seed_paths.is_empty() {
                component.paths.first().cloned().into_iter().collect()
            } else {
                component.seed_paths.clone()
            };
            let representative_total = representatives.len();
            let representative_paths = path_budget.take(representatives);
            WorkScopeComponent {
                files: component.paths.len(),
                seed_files: component.seed_paths.len(),
                representative_paths_omitted: representative_total
                    .saturating_sub(representative_paths.len()),
                representative_paths,
            }
        })
        .collect::<Vec<_>>();

    WorkScopeStructure {
        graph_files: graph.files.len(),
        components: total,
        largest_component_files,
        shown: entries.len(),
        omitted: total.saturating_sub(entries.len()),
        entries,
    }
}

fn graph_seed_paths(
    context: Option<&ContextPlan>,
    changed: &HashSet<PathBuf>,
    graph: Option<&GraphSignals>,
) -> HashSet<String> {
    let Some(graph) = graph else {
        return HashSet::new();
    };
    let mut seeds = changed
        .iter()
        .map(|path| normalize(path))
        .filter(|path| graph.files.contains_key(path))
        .collect::<HashSet<_>>();
    if let Some(context) = context {
        let focus = context
            .focus
            .iter()
            .map(|path| normalize(path))
            .collect::<Vec<_>>();
        for path in graph.files.keys() {
            if focus.iter().any(|focus| matches_focus(path, focus)) {
                seeds.insert(path.clone());
            }
        }
    }
    seeds
}

fn project_coverage(diagnostics: &ScanDiagnostics, diff_scoped: bool) -> WorkScopeCoverage {
    WorkScopeCoverage {
        discovered_files: diagnostics.discovered_files,
        analyzed_files: diagnostics.analyzed_files,
        unsupported_files: diagnostics.unsupported_files,
        unreadable_files: diagnostics.unreadable_files,
        walker_errors: diagnostics.walker_errors,
        diff_scoped,
        oversized_files: diagnostics.oversized_files,
        oversized_bytes: diagnostics.oversized_bytes,
        files_omitted_by_limit: diagnostics.files_omitted_by_limit,
        bytes_omitted_by_limit: diagnostics.bytes_omitted_by_limit,
        omitted_count_incomplete: diagnostics.files_omitted_count_incomplete,
        duration_limit_reached: diagnostics.duration_limit_reached,
        truncated: diagnostics.scan_truncated,
    }
}

fn normalized_paths<'a>(paths: impl Iterator<Item = &'a PathBuf>) -> Vec<String> {
    let mut paths = paths.map(|path| normalize(path)).collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    paths
}

fn normalize(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn matches_focus(path: &str, focus: &str) -> bool {
    path == focus
        || path
            .strip_prefix(focus)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

struct ComponentFacts {
    paths: Vec<String>,
    seed_paths: Vec<String>,
}

struct PathBudget {
    remaining: usize,
}

impl PathBudget {
    fn new(limit: usize) -> Self {
        Self { remaining: limit }
    }

    fn take(&mut self, paths: Vec<String>) -> Vec<String> {
        let shown = paths.into_iter().take(self.remaining).collect::<Vec<_>>();
        self.remaining = self.remaining.saturating_sub(shown.len());
        shown
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::GraphFileSignal;
    use crate::model::{Assessment, ProductionDuplication, SourceSummary};
    use std::collections::HashMap;

    fn summary() -> Summary {
        Summary {
            source: SourceSummary {
                files: 12,
                tokens: 24_000,
                ..SourceSummary::default()
            },
            assessment: Assessment {
                unavailable_signals: vec!["churn".to_string()],
                production_duplication: Some(ProductionDuplication {
                    corpus: "production-source".to_string(),
                    duplicated_lines: 12,
                    analyzed_lines: 120,
                    duplicated_pct: 10.0,
                    complete: true,
                }),
                ..Assessment::default()
            },
            ..Summary::default()
        }
    }

    fn diagnostics() -> ScanDiagnostics {
        ScanDiagnostics {
            discovered_files: 15,
            analyzed_files: 14,
            unsupported_files: 1,
            ..ScanDiagnostics::default()
        }
    }

    #[test]
    fn repository_scope_uses_primary_inventory_without_inventing_optional_analysis() {
        let scope = build(&Inputs {
            summary: &summary(),
            diagnostics: &diagnostics(),
            context: None,
            graph: None,
            impact: None,
            diff_scope: None,
            changed: &HashSet::new(),
        });

        assert_eq!(scope.strategy_version, STRATEGY_VERSION);
        assert_eq!(scope.basis, ["repository"]);
        assert_eq!(scope.inventory.source_files, 12);
        assert_eq!(scope.inventory.source_tokens, 24_000);
        let production = scope
            .production_duplication
            .expect("production duplication evidence");
        assert_eq!(production.corpus, "production-source");
        assert!((production.duplicated_pct - 10.0).abs() < f64::EPSILON);
        assert!(production.complete);
        assert!(scope.seeds.is_none());
        assert!(scope.context.is_none());
        assert!(scope.impact.is_none());
        assert!(scope.structure.is_none());
        assert_eq!(scope.confidence.unavailable_signals, ["churn"]);
    }

    #[test]
    fn seed_path_bound_preserves_totals_and_omissions() {
        let context = ContextPlan {
            focus: (0..20)
                .map(|index| PathBuf::from(format!("src/focus-{index}.rs")))
                .collect(),
            unmatched_focus: (0..10)
                .map(|index| PathBuf::from(format!("missing-{index}")))
                .collect(),
            ..ContextPlan::default()
        };
        let changed_paths = (0..10)
            .map(|index| PathBuf::from(format!("src/changed-{index}.rs")))
            .collect::<HashSet<_>>();

        let scope = build(&Inputs {
            summary: &summary(),
            diagnostics: &diagnostics(),
            context: Some(&context),
            graph: None,
            impact: None,
            diff_scope: Some("working"),
            changed: &changed_paths,
        });
        let seeds = scope.seeds.expect("seed facts");
        let focus = seeds.focus.expect("focus facts");
        let changes = seeds.changes.expect("change facts");

        assert_eq!(scope.basis, ["diff", "focus"]);
        assert!(scope.confidence.primary.diff_scoped);
        assert_eq!(focus.total, 30);
        assert_eq!(focus.shown, MAX_PATH_ENTRIES);
        assert_eq!(focus.omitted, 5);
        assert_eq!(changes.total, 10);
        assert_eq!(changes.shown, 0);
        assert_eq!(changes.omitted, 10);
        assert_eq!(seeds.paths_omitted, 15);
    }

    #[test]
    fn context_projection_keeps_uncapped_omission_totals() {
        let context = ContextPlan {
            budget_tokens: 1_000,
            selected_tokens: 900,
            candidate_files: 8,
            omitted_files: 5,
            omitted_tokens: 12_000,
            skipped_files: 2,
            outline_symbols: 7,
            outline_bytes: 512,
            outline_omitted_symbols: 3,
            seed_files: 2,
            graph_eligible_seed_files: 2,
            graph_covered_seed_files: 1,
            direct_dependents: 4,
            transitive_dependents: 6,
            matching_tests: 2,
            files: vec![
                crate::model::ContextFile::default(),
                crate::model::ContextFile::default(),
            ],
            ..ContextPlan::default()
        };

        let scope = build(&Inputs {
            summary: &summary(),
            diagnostics: &diagnostics(),
            context: Some(&context),
            graph: None,
            impact: None,
            diff_scope: None,
            changed: &HashSet::new(),
        });
        let projected = scope.context.expect("context facts");
        let impact = scope.impact.expect("impact facts");

        assert_eq!(projected.selected_files, 2);
        assert_eq!(projected.omitted_files, 5);
        assert_eq!(projected.omitted_tokens, 12_000);
        assert_eq!(projected.outline_bytes, 512);
        assert!(projected.truncated);
        assert_eq!(impact.direct_dependents, 4);
        assert_eq!(impact.transitive_dependents, 6);
        assert_eq!(impact.matching_tests, 2);
        assert!(impact.matching_tests_known);
    }

    #[test]
    fn graph_structure_reports_weak_components_and_prioritizes_seeded_component() {
        let mut files = HashMap::new();
        files.insert(
            "src/a.rs".to_string(),
            GraphFileSignal {
                dependencies: vec!["src/b.rs".to_string()],
                ..GraphFileSignal::default()
            },
        );
        files.insert(
            "src/b.rs".to_string(),
            GraphFileSignal {
                dependents: vec!["src/a.rs".to_string()],
                ..GraphFileSignal::default()
            },
        );
        files.insert("src/c.rs".to_string(), GraphFileSignal::default());
        files.insert(
            "src/d.rs".to_string(),
            GraphFileSignal {
                dependencies: vec!["src/e.rs".to_string()],
                ..GraphFileSignal::default()
            },
        );
        files.insert(
            "src/e.rs".to_string(),
            GraphFileSignal {
                dependents: vec!["src/d.rs".to_string()],
                ..GraphFileSignal::default()
            },
        );
        let graph = GraphSignals {
            files,
            ..GraphSignals::default()
        };
        let context = ContextPlan {
            focus: vec![PathBuf::from("src/a.rs")],
            ..ContextPlan::default()
        };

        let scope = build(&Inputs {
            summary: &summary(),
            diagnostics: &diagnostics(),
            context: Some(&context),
            graph: Some(&graph),
            impact: None,
            diff_scope: None,
            changed: &HashSet::new(),
        });
        let structure = scope.structure.expect("graph structure");

        assert_eq!(structure.graph_files, 5);
        assert_eq!(structure.components, 3);
        assert_eq!(structure.largest_component_files, 2);
        assert_eq!(structure.entries[0].seed_files, 1);
        assert_eq!(
            structure.entries[0].representative_paths,
            ["src/a.rs".to_string()]
        );
    }
}
