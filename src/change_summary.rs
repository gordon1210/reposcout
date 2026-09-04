use crate::graph::GraphDiagnosticFact;
use crate::lang;
use crate::metrics::testcov;
use crate::model::{
    ChangeCoverage, ChangeExecutive, ChangeFile, ChangeFileList, ChangeGap, ChangeGapCounts,
    ChangeImpactFile, ChangeImpactSummary, ChangeReadingFile, ChangeSummary, ChangeTestFile,
    ChangeTestList, ChangeValidation, ContextEvidence, ContextPlan, FileReport, ImpactAnalysis,
    ScanDiagnostics,
};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

pub(crate) const STRATEGY_VERSION: u32 = 2;
pub(crate) const MAX_PATH_ENTRIES: usize = 100;
pub(crate) const MAX_GAP_ENTRIES: usize = 25;
pub(crate) const MAX_VALIDATIONS: usize = 10;

#[derive(Clone, Copy)]
pub(crate) struct Inputs<'a> {
    pub scope: &'a str,
    pub changed: &'a HashSet<PathBuf>,
    pub context: Option<&'a ContextPlan>,
    pub files: &'a [FileReport],
    pub impact: Option<&'a ImpactAnalysis>,
    pub graph_diagnostics: &'a [GraphDiagnosticFact],
    pub scan_diagnostics: &'a ScanDiagnostics,
    pub discovery_diagnostics: &'a ScanDiagnostics,
}

#[expect(
    clippy::too_many_lines,
    reason = "change-summary construction is one deterministic bounded projection whose shared path budget and confidence evidence must stay in visible order"
)]
pub(crate) fn build(inputs: Inputs<'_>) -> ChangeSummary {
    let mut changed_paths = inputs
        .changed
        .iter()
        .map(|path| normalize(path))
        .collect::<Vec<_>>();
    changed_paths.sort();
    changed_paths.dedup();
    let changed_set = changed_paths.iter().cloned().collect::<HashSet<_>>();

    let graph_eligible = changed_paths
        .iter()
        .filter(|path| lang::detect(Path::new(path)).is_some_and(lang::LangInfo::is_first_class))
        .cloned()
        .collect::<HashSet<_>>();
    let graph_covered: HashSet<String> = inputs
        .impact
        .map(|impact| impact.graph_changed_files.iter().cloned().collect())
        .unwrap_or_default();
    let direct = inputs
        .impact
        .map(|impact| impact.direct_dependents.clone())
        .unwrap_or_default();
    let transitive = inputs
        .impact
        .map(|impact| impact.transitive_dependents.clone())
        .unwrap_or_default();
    let direct_set = direct.iter().cloned().collect::<HashSet<_>>();
    let transitive_set = transitive.iter().cloned().collect::<HashSet<_>>();

    let mut context_entries = if changed_paths.is_empty() {
        Vec::new()
    } else {
        context_entries(inputs.context)
    };
    let changed_sources = changed_paths
        .iter()
        .filter(|path| {
            lang::detect(Path::new(path)).is_some_and(lang::LangInfo::is_code)
                && !testcov::is_test_file(path)
        })
        .cloned()
        .collect::<Vec<_>>();
    let test_files = matching_tests(inputs.files, &changed_sources);
    let matching_test_paths = test_files
        .iter()
        .map(|test| test.path.clone())
        .collect::<BTreeSet<_>>();
    merge_matching_tests(&mut context_entries, &matching_test_paths);
    let selected_context = context_entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect::<HashSet<_>>();

    let known_impact = direct_set
        .union(&transitive_set)
        .cloned()
        .chain(graph_covered.iter().cloned())
        .collect::<HashSet<_>>();
    let (relevant_gap_entries, outside_gap_entries, relevant_gaps, outside_gaps) = classify_gaps(
        inputs.graph_diagnostics,
        &changed_set,
        &known_impact,
        &selected_context,
    );

    let observed_scope_confidence = if graph_eligible.is_empty() {
        "not-applicable"
    } else if graph_covered.is_empty() {
        "none"
    } else if graph_covered.len() < graph_eligible.len() || !relevant_gaps.is_empty() {
        "partial"
    } else {
        "high"
    }
    .to_string();
    let scan_truncated = diagnostics_incomplete(inputs.scan_diagnostics)
        || diagnostics_incomplete(inputs.discovery_diagnostics);
    let discovery_completeness = if graph_covered.is_empty() {
        "none"
    } else if !outside_gaps.is_empty() || !relevant_gaps.is_empty() || scan_truncated {
        "partial"
    } else {
        "high"
    }
    .to_string();
    let test_mapping_confidence = if changed_sources.is_empty() {
        "not-applicable"
    } else if test_files.is_empty() {
        "none"
    } else {
        "partial"
    }
    .to_string();
    let (executive_confidence, executive_reasons) = executive_confidence(
        &observed_scope_confidence,
        &discovery_completeness,
        &test_mapping_confidence,
        graph_eligible.len(),
        graph_covered.len(),
        &relevant_gaps,
        &outside_gaps,
        scan_truncated,
    );

    let mut remaining_paths = MAX_PATH_ENTRIES;
    let changed_files = changed_paths
        .iter()
        .take(remaining_paths)
        .map(|path| ChangeFile {
            path: path.clone(),
            graph_eligible: graph_eligible.contains(path),
            graph_covered: graph_covered.contains(path),
        })
        .collect::<Vec<_>>();
    remaining_paths = remaining_paths.saturating_sub(changed_files.len());
    let changed = ChangeFileList {
        total: changed_paths.len(),
        shown: changed_files.len(),
        omitted: changed_paths.len().saturating_sub(changed_files.len()),
        files: changed_files,
    };

    let mut gap_details = Vec::new();
    gap_details.extend(
        relevant_gap_entries
            .iter()
            .take(MAX_GAP_ENTRIES.min(remaining_paths))
            .cloned(),
    );
    remaining_paths = remaining_paths.saturating_sub(gap_details.len());

    let shown_tests = test_files
        .iter()
        .take(remaining_paths)
        .cloned()
        .collect::<Vec<_>>();
    remaining_paths = remaining_paths.saturating_sub(shown_tests.len());
    let tests = ChangeTestList {
        total: test_files.len(),
        shown: shown_tests.len(),
        omitted: test_files.len().saturating_sub(shown_tests.len()),
        files: shown_tests,
    };

    let direct_files = direct
        .iter()
        .take(remaining_paths)
        .map(|path| ChangeImpactFile {
            path: path.clone(),
            distance: 1,
            confidence: "partial".to_string(),
            resolver: context_resolver(&context_entries, path),
        })
        .collect::<Vec<_>>();
    remaining_paths = remaining_paths.saturating_sub(direct_files.len());

    let reading_order_total = context_entries.len();
    let reading_order = context_entries
        .iter()
        .take(remaining_paths)
        .cloned()
        .collect::<Vec<_>>();
    let reading_order_shown = reading_order.len();
    let reading_order_omitted = reading_order_total.saturating_sub(reading_order_shown);
    remaining_paths = remaining_paths.saturating_sub(reading_order.len());

    let transitive_files = transitive
        .iter()
        .take(remaining_paths)
        .map(|path| ChangeImpactFile {
            path: path.clone(),
            distance: context_distance(&context_entries, path).unwrap_or(2),
            confidence: "partial".to_string(),
            resolver: context_resolver(&context_entries, path),
        })
        .collect::<Vec<_>>();
    remaining_paths = remaining_paths.saturating_sub(transitive_files.len());

    let outside_room = MAX_GAP_ENTRIES
        .saturating_sub(gap_details.len())
        .min(remaining_paths);
    let outside_details = outside_gap_entries
        .iter()
        .take(outside_room)
        .cloned()
        .collect::<Vec<_>>();
    remaining_paths = remaining_paths.saturating_sub(outside_details.len());
    gap_details.extend(outside_details);

    let mut impact_files = direct_files;
    impact_files.extend(transitive_files);
    let impact_total = direct.len().saturating_add(transitive.len());
    let impact = ChangeImpactSummary {
        direct_total: direct.len(),
        transitive_total: transitive.len(),
        shown: impact_files.len(),
        omitted: impact_total.saturating_sub(impact_files.len()),
        files: impact_files,
    };

    let gap_total = relevant_gap_entries
        .len()
        .saturating_add(outside_gap_entries.len());
    let coverage = ChangeCoverage {
        observed_scope_confidence,
        discovery_completeness,
        test_mapping_confidence,
        graph_eligible_changed: graph_eligible.len(),
        graph_covered_changed: graph_covered.len(),
        non_graph_changed: changed_paths.len().saturating_sub(graph_eligible.len()),
        relevant_gaps,
        outside_known_scope_gaps: outside_gaps,
        gaps_omitted: gap_total.saturating_sub(gap_details.len()),
        gaps: gap_details,
    };

    let all_validations = validation_candidates(
        &changed_paths,
        &graph_eligible,
        &test_files,
        &relevant_gap_entries,
    );
    let validation_limit = MAX_VALIDATIONS.min(remaining_paths);
    let validations = all_validations
        .iter()
        .take(validation_limit)
        .cloned()
        .collect::<Vec<_>>();

    ChangeSummary {
        strategy_version: STRATEGY_VERSION,
        scope: inputs.scope.to_string(),
        executive: ChangeExecutive {
            changed_files: changed_paths.len(),
            graph_eligible_changed_files: graph_eligible.len(),
            known_direct_dependents: direct.len(),
            known_transitive_dependents: transitive.len(),
            matching_tests: matching_test_paths.len(),
            confidence: executive_confidence,
            reasons: executive_reasons,
        },
        changed,
        reading_order,
        reading_order_total,
        reading_order_shown,
        reading_order_omitted,
        impact,
        tests,
        coverage,
        validations_omitted: all_validations.len().saturating_sub(validations.len()),
        validations,
    }
}

fn normalize(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn context_entries(context: Option<&ContextPlan>) -> Vec<ChangeReadingFile> {
    let mut entries = BTreeMap::<String, ChangeReadingFile>::new();
    if let Some(context) = context {
        for file in &context.files {
            merge_context_entry(&mut entries, normalize(&file.path), &file.evidence);
        }
        for file in &context.outline_only {
            merge_context_entry(&mut entries, normalize(&file.path), &file.evidence);
        }
        for path in &context.changed_files {
            let path = normalize(path);
            let entry = entries
                .entry(path.clone())
                .or_insert_with(|| ChangeReadingFile {
                    path,
                    confidence: "high".to_string(),
                    ..ChangeReadingFile::default()
                });
            add_role(&mut entry.roles, "changed");
            entry.confidence = "high".to_string();
        }
    }
    let order = context
        .into_iter()
        .flat_map(|context| {
            context
                .files
                .iter()
                .map(|file| normalize(&file.path))
                .chain(
                    context
                        .outline_only
                        .iter()
                        .map(|file| normalize(&file.path)),
                )
                .chain(context.changed_files.iter().map(|path| normalize(path)))
        })
        .collect::<Vec<_>>();
    let mut ordered = Vec::new();
    let mut seen = HashSet::new();
    for path in order {
        if seen.insert(path.clone())
            && let Some(entry) = entries.remove(&path)
        {
            ordered.push(entry);
        }
    }
    ordered.extend(entries.into_values());
    ordered
}

fn merge_context_entry(
    entries: &mut BTreeMap<String, ChangeReadingFile>,
    path: String,
    evidence: &[ContextEvidence],
) {
    let entry = entries
        .entry(path.clone())
        .or_insert_with(|| ChangeReadingFile {
            path,
            confidence: "partial".to_string(),
            ..ChangeReadingFile::default()
        });
    for item in evidence {
        add_role(&mut entry.roles, &item.role);
        if matches!(item.role.as_str(), "changed" | "focus") {
            entry.confidence = "high".to_string();
        }
        if entry.distance.is_none() {
            entry.distance = item.distance;
        }
        if entry.resolver.is_none() {
            entry.resolver.clone_from(&item.resolver);
        }
    }
    entry.roles.sort_by_key(|role| role_rank(role));
    entry.roles.dedup();
}

fn add_role(roles: &mut Vec<String>, role: &str) {
    if !roles.iter().any(|existing| existing == role) {
        roles.push(role.to_string());
    }
}

fn role_rank(role: &str) -> usize {
    match role {
        "changed" => 0,
        "focus" => 1,
        "matching-test" => 2,
        "dependency" => 3,
        "dependent" => 4,
        "nearby" => 5,
        _ => 6,
    }
}

fn matched_sources(test: &str, changed_sources: &[String]) -> Vec<String> {
    let keys = testcov::test_stem_keys(test);
    changed_sources
        .iter()
        .filter(|source| keys.iter().any(|key| key == &testcov::source_stem(source)))
        .cloned()
        .collect()
}

fn matching_tests(files: &[FileReport], changed_sources: &[String]) -> Vec<ChangeTestFile> {
    if changed_sources.is_empty() {
        return Vec::new();
    }
    let mut tests = files
        .iter()
        .map(|file| normalize(&file.path))
        .filter(|path| testcov::is_test_file(path))
        .filter_map(|path| {
            let matched_sources = matched_sources(&path, changed_sources);
            (!matched_sources.is_empty()).then_some(ChangeTestFile {
                path,
                matched_sources,
                confidence: "partial".to_string(),
            })
        })
        .collect::<Vec<_>>();
    tests.sort_by(|left, right| left.path.cmp(&right.path));
    tests.dedup_by(|left, right| left.path == right.path);
    tests
}

fn merge_matching_tests(entries: &mut Vec<ChangeReadingFile>, tests: &BTreeSet<String>) {
    for path in tests {
        if let Some(entry) = entries.iter_mut().find(|entry| entry.path == *path) {
            add_role(&mut entry.roles, "matching-test");
            entry.roles.sort_by_key(|role| role_rank(role));
        } else {
            entries.push(ChangeReadingFile {
                path: path.clone(),
                roles: vec!["matching-test".to_string()],
                confidence: "partial".to_string(),
                ..ChangeReadingFile::default()
            });
        }
    }
}

fn context_distance(entries: &[ChangeReadingFile], path: &str) -> Option<usize> {
    entries
        .iter()
        .find(|entry| entry.path == path)
        .and_then(|entry| entry.distance)
}

fn context_resolver(entries: &[ChangeReadingFile], path: &str) -> Option<String> {
    entries
        .iter()
        .find(|entry| entry.path == path)
        .and_then(|entry| entry.resolver.clone())
}

fn classify_gaps(
    facts: &[GraphDiagnosticFact],
    changed: &HashSet<String>,
    known_impact: &HashSet<String>,
    selected_context: &HashSet<String>,
) -> (
    Vec<ChangeGap>,
    Vec<ChangeGap>,
    ChangeGapCounts,
    ChangeGapCounts,
) {
    let mut relevant = Vec::new();
    let mut outside = Vec::new();
    let mut relevant_counts = ChangeGapCounts::default();
    let mut outside_counts = ChangeGapCounts::default();
    for fact in facts {
        let scope = if changed.contains(&fact.path) {
            "changed"
        } else if known_impact.contains(&fact.path)
            || fact.config_errors > 0
                && known_impact
                    .iter()
                    .any(|path| config_governs(&fact.path, path))
        {
            "known-impact"
        } else if selected_context.contains(&fact.path)
            || fact.config_errors > 0
                && selected_context
                    .iter()
                    .any(|path| config_governs(&fact.path, path))
        {
            "selected-context"
        } else {
            "outside-known-scope"
        };
        let gap = ChangeGap {
            path: fact.path.clone(),
            scope: scope.to_string(),
            unreadable: fact.unreadable,
            parse_errors: fact.parse_errors,
            unresolved_imports: fact.unresolved_imports,
            config_errors: fact.config_errors,
        };
        if scope == "outside-known-scope" {
            add_gap_counts(&mut outside_counts, &gap);
            outside.push(gap);
        } else {
            add_gap_counts(&mut relevant_counts, &gap);
            relevant.push(gap);
        }
    }
    relevant.sort_by(|left, right| {
        gap_scope_rank(&left.scope)
            .cmp(&gap_scope_rank(&right.scope))
            .then_with(|| left.path.cmp(&right.path))
    });
    outside.sort_by(|left, right| left.path.cmp(&right.path));
    (relevant, outside, relevant_counts, outside_counts)
}

fn config_governs(config: &str, path: &str) -> bool {
    let parent = Path::new(config)
        .parent()
        .map(normalize)
        .unwrap_or_default();
    parent.is_empty()
        || path == parent
        || path
            .strip_prefix(&parent)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn gap_scope_rank(scope: &str) -> usize {
    match scope {
        "changed" => 0,
        "known-impact" => 1,
        "selected-context" => 2,
        _ => 3,
    }
}

fn add_gap_counts(counts: &mut ChangeGapCounts, gap: &ChangeGap) {
    counts.unreadable_files = counts
        .unreadable_files
        .saturating_add(usize::from(gap.unreadable));
    counts.parse_errors = counts.parse_errors.saturating_add(gap.parse_errors);
    counts.unresolved_imports = counts
        .unresolved_imports
        .saturating_add(gap.unresolved_imports);
    counts.config_errors = counts.config_errors.saturating_add(gap.config_errors);
}

fn diagnostics_incomplete(diagnostics: &ScanDiagnostics) -> bool {
    diagnostics.scan_truncated || diagnostics.walker_errors > 0
}

#[expect(
    clippy::too_many_arguments,
    reason = "confidence combines independent coverage dimensions whose names are clearer at the call site than in a positional tuple"
)]
fn executive_confidence(
    observed: &str,
    discovery: &str,
    test_mapping: &str,
    graph_eligible: usize,
    graph_covered: usize,
    relevant: &ChangeGapCounts,
    outside: &ChangeGapCounts,
    scan_truncated: bool,
) -> (String, Vec<String>) {
    let mut reasons = Vec::new();
    if graph_eligible == 0 {
        reasons.push("no-graph-eligible-changes".to_string());
    } else if graph_covered == 0 {
        reasons.push("no-graph-covered-changes".to_string());
    } else if graph_covered < graph_eligible {
        reasons.push("changed-graph-coverage-incomplete".to_string());
    }
    if !relevant.is_empty() {
        reasons.push("relevant-graph-gaps".to_string());
    }
    if !outside.is_empty() {
        reasons.push("repository-graph-gaps".to_string());
    }
    if scan_truncated {
        reasons.push("scan-truncated".to_string());
    }
    if test_mapping == "partial" {
        reasons.push("test-mapping-heuristic".to_string());
    } else if test_mapping == "none" {
        reasons.push("no-matching-tests".to_string());
    }
    let confidence = if graph_covered == 0 {
        "none"
    } else if observed == "high" && discovery == "high" {
        "high"
    } else {
        "partial"
    };
    (confidence.to_string(), reasons)
}

fn validation_candidates(
    changed: &[String],
    graph_eligible: &HashSet<String>,
    tests: &[ChangeTestFile],
    gaps: &[ChangeGap],
) -> Vec<ChangeValidation> {
    let mut validations = Vec::new();
    for test in tests {
        validations.push(ChangeValidation {
            kind: "mapped-test".to_string(),
            target: Some(test.path.clone()),
            reason: "Existing test naming matches a changed source file; consider running it."
                .to_string(),
            confidence: "partial".to_string(),
        });
    }
    for path in changed {
        if is_project_configuration(path) {
            validations.push(ChangeValidation {
                kind: "project-configuration".to_string(),
                target: Some(path.clone()),
                reason: "A changed manifest, build file, or tool configuration needs specialist validation."
                    .to_string(),
                confidence: "high".to_string(),
            });
        } else if !graph_eligible.contains(path) {
            validations.push(ChangeValidation {
                kind: "inspect-non-graph-change".to_string(),
                target: Some(path.clone()),
                reason:
                    "This changed file cannot participate in first-class dependency impact analysis."
                        .to_string(),
                confidence: "high".to_string(),
            });
        }
    }
    for gap in gaps.iter().filter(|gap| gap.scope != "outside-known-scope") {
        validations.push(ChangeValidation {
            kind: "specialist-review".to_string(),
            target: Some(gap.path.clone()),
            reason: "A relevant parser or resolver gap limits automated impact evidence."
                .to_string(),
            confidence: "partial".to_string(),
        });
    }
    validations
}

fn is_project_configuration(path: &str) -> bool {
    let name = Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        name.as_str(),
        "cargo.toml"
            | "package.json"
            | "composer.json"
            | "go.mod"
            | "makefile"
            | "dockerfile"
            | "compose.yml"
            | "compose.yaml"
            | "docker-compose.yml"
            | "docker-compose.yaml"
            | "tsconfig.json"
            | "jsconfig.json"
    ) || name.ends_with(".config.js")
        || name.ends_with(".config.ts")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ContextFile, ImpactAnalysis, ScanDiagnostics};

    #[test]
    fn explicit_focus_stays_high_confidence_in_change_reading_order() {
        let context = ContextPlan {
            files: vec![ContextFile {
                path: PathBuf::from("src/focus.rs"),
                evidence: vec![ContextEvidence {
                    role: "focus".to_string(),
                    confidence: "high".to_string(),
                    distance: Some(0),
                    resolver: None,
                }],
                ..ContextFile::default()
            }],
            ..ContextPlan::default()
        };

        let entries = context_entries(Some(&context));

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].roles, ["focus"]);
        assert_eq!(entries[0].confidence, "high");
        assert_eq!(entries[0].distance, Some(0));
    }

    #[test]
    fn planning_truncation_lowers_discovery_completeness() {
        let changed = HashSet::from([PathBuf::from("src/lib.rs")]);
        let impact = ImpactAnalysis {
            changed_files: vec!["src/lib.rs".to_string()],
            graph_changed_files: vec!["src/lib.rs".to_string()],
            confidence: "high".to_string(),
            ..ImpactAnalysis::default()
        };
        let primary_diagnostics = ScanDiagnostics::default();
        let discovery_diagnostics = ScanDiagnostics {
            scan_truncated: true,
            ..ScanDiagnostics::default()
        };

        let summary = build(Inputs {
            scope: "working",
            changed: &changed,
            context: None,
            files: &[],
            impact: Some(&impact),
            graph_diagnostics: &[],
            scan_diagnostics: &primary_diagnostics,
            discovery_diagnostics: &discovery_diagnostics,
        });

        assert_eq!(summary.coverage.observed_scope_confidence, "high");
        assert_eq!(summary.coverage.discovery_completeness, "partial");
        assert!(
            summary
                .executive
                .reasons
                .contains(&"scan-truncated".to_string())
        );
    }
}
