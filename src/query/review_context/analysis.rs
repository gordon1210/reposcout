use super::changes::sha256;
use crate::graph::{self, GraphReadLimits};
use crate::metrics::{testcov, tokens::TokenCounter};
use crate::model::{
    CallReferenceStatus, GraphEdge, ReviewAnalysisStatus, ReviewChangeBasis, ReviewContextChange,
    ReviewContextFile, ReviewContextRelation,
};
use crate::scan::{ReviewCapture, ReviewRevision};
use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

#[derive(Default)]
struct Candidate {
    roles: BTreeSet<String>,
    distance: usize,
    via: Option<PathBuf>,
}

pub(super) fn analyze(
    captured: &mut ReviewCapture,
    changes: &[ReviewContextChange],
    counter: &TokenCounter,
    limits: GraphReadLimits,
) -> Result<(Vec<ReviewContextFile>, Vec<ReviewContextRelation>)> {
    super::check_deadline(captured.deadline)?;
    let mut context = Vec::new();
    let mut relations = Vec::new();
    for (index, revision) in captured.revisions.iter_mut().enumerate() {
        super::check_deadline(captured.deadline)?;
        let side = if index == 0 { "base" } else { "head" };
        let changed_sides: BTreeMap<_, _> = changes
            .iter()
            .filter_map(|change| {
                if index == 0 {
                    change.base.as_ref()
                } else {
                    change.head.as_ref()
                }
            })
            .map(|file| (file.path.clone(), file))
            .collect();
        let seeds = changed_sides.keys().cloned().collect();
        let inputs = revision.graph_inputs();
        let files: Vec<_> = revision
            .files()
            .iter()
            .filter(|(path, _)| ReviewRevision::supports_graph_path(path))
            .filter_map(|(_, file)| file.report.clone())
            .collect();
        let graph = graph::build_with_inputs(&files, &captured.root, limits, &inputs);
        super::check_deadline(captured.deadline)?;
        let calls = graph::resolve_call_references(
            &captured.root,
            &inputs.source_facts,
            &inputs.resolver_configs,
            limits,
        );
        super::check_deadline(captured.deadline)?;
        coverage(revision, &graph, &inputs, &seeds, &calls.coverage);
        let mut candidates = neighborhood(&seeds, &graph.edge_list);
        config_candidates(&mut candidates, &seeds, &graph);
        call_candidates(
            &mut candidates,
            &mut relations,
            side,
            &changed_sides,
            calls.edges,
        );
        type_candidates(&mut candidates, &mut relations, side, &seeds, &graph);
        for edge in graph.edge_list {
            super::check_deadline(captured.deadline)?;
            if candidates.contains_key(Path::new(&edge.source))
                && candidates.contains_key(Path::new(&edge.target))
            {
                relations.push(ReviewContextRelation {
                    side: side.into(),
                    kind: "import".into(),
                    edge,
                    change_basis: None,
                    symbol: None,
                    type_relation: None,
                });
            }
        }
        for (path, evidence) in candidates {
            super::check_deadline(captured.deadline)?;
            context.push(describe(revision, side, &path, evidence, counter));
        }
    }
    context.sort_by(|left, right| {
        (priority(left), left.distance, &left.path, &left.side).cmp(&(
            priority(right),
            right.distance,
            &right.path,
            &right.side,
        ))
    });
    let relations = order_relations(relations, changes);
    super::check_deadline(captured.deadline)?;
    Ok((context, relations))
}

fn order_relations(
    mut relations: Vec<ReviewContextRelation>,
    changes: &[ReviewContextChange],
) -> Vec<ReviewContextRelation> {
    let incomplete_paths: BTreeSet<_> = changes
        .iter()
        .flat_map(|change| {
            [
                ("base", change.base.as_ref()),
                ("head", change.head.as_ref()),
            ]
            .into_iter()
            .filter_map(|(side, file)| {
                let file = file?;
                mapping_is_incomplete(file).then_some((side, file.path.as_path()))
            })
        })
        .collect();
    let priority = |relation: &ReviewContextRelation| match relation.kind.as_str() {
        "symbol-reference" if relation.change_basis == Some(ReviewChangeBasis::ChangedFile) => 2,
        "symbol-reference" => 0,
        "type-relationship" => 1,
        _ => 3,
    };
    let direction = |relation: &ReviewContextRelation| {
        if relation.change_basis != Some(ReviewChangeBasis::ChangedFile) {
            0
        } else if relation.edge.source == relation.edge.target {
            2
        } else {
            u8::from(
                !incomplete_paths
                    .contains(&(relation.side.as_str(), Path::new(&relation.edge.target))),
            )
        }
    };
    relations.sort_by(|left, right| {
        (
            &left.side,
            priority(left),
            direction(left),
            &left.edge.source,
            &left.edge.target,
        )
            .cmp(&(
                &right.side,
                priority(right),
                direction(right),
                &right.edge.source,
                &right.edge.target,
            ))
    });
    let head = relations.partition_point(|relation| relation.side == "base");
    let mut head = relations.split_off(head).into_iter();
    let mut base = relations.into_iter();
    let mut ordered = Vec::with_capacity(base.len() + head.len());
    loop {
        match (head.next(), base.next()) {
            (None, None) => return ordered,
            (head, base) => ordered.extend(head.into_iter().chain(base)),
        }
    }
}

fn coverage(
    revision: &mut ReviewRevision,
    graph: &crate::model::DepGraph,
    inputs: &graph::GraphInputs,
    seeds: &BTreeSet<PathBuf>,
    calls: &crate::model::CallResolutionCoverage,
) {
    let coverage = &mut revision.coverage;
    coverage.graph_files = graph.nodes;
    coverage.changed_graph_files = graph
        .files
        .iter()
        .filter(|file| seeds.contains(Path::new(&file.path)))
        .count();
    coverage.changed_files_without_graph = seeds.len().saturating_sub(coverage.changed_graph_files);
    coverage.unresolved_imports = graph.unresolved_imports;
    coverage.unresolved_type_relations = graph.unresolved_symbol_relations;
    coverage.parse_errors = graph.parse_errors;
    coverage.config_errors = graph.config_errors;
    coverage.call_resolution = calls.clone();
    for facts in inputs
        .source_facts
        .values()
        .filter_map(|facts| facts.call_references.as_ref())
    {
        match facts.status {
            CallReferenceStatus::Available => {}
            CallReferenceStatus::Unsupported => coverage.unsupported_call_files += 1,
            _ => coverage.incomplete_call_files += 1,
        }
    }
}

fn config_candidates(
    candidates: &mut BTreeMap<PathBuf, Candidate>,
    seeds: &BTreeSet<PathBuf>,
    graph: &crate::model::DepGraph,
) {
    for config in &graph.config_files {
        let path = PathBuf::from(config);
        if seeds.contains(&path) {
            let directory = path.parent().unwrap_or(Path::new(""));
            for file in &graph.files {
                if Path::new(&file.path).starts_with(directory) {
                    add(
                        candidates,
                        Path::new(&file.path),
                        "resolver-config-scope",
                        1,
                        Some(&path),
                    );
                }
            }
        }
    }
}

fn changed_symbol(
    symbol: &crate::model::CallSymbolIdentity,
    changed: &BTreeMap<PathBuf, &crate::model::ReviewChangedSide>,
) -> bool {
    changed.get(Path::new(&symbol.path)).is_some_and(|file| {
        file.definitions
            .iter()
            .any(|definition| definition.declaration_span == symbol.declaration_span)
    })
}

fn call_candidates(
    candidates: &mut BTreeMap<PathBuf, Candidate>,
    relations: &mut Vec<ReviewContextRelation>,
    side: &str,
    changed: &BTreeMap<PathBuf, &crate::model::ReviewChangedSide>,
    edges: Vec<crate::model::ResolvedCallReference>,
) {
    for edge in edges {
        let basis =
            if changed_symbol(&edge.target, changed) || changed_symbol(&edge.source, changed) {
                Some(ReviewChangeBasis::ChangedDefinition)
            } else if incompletely_mapped_file(&edge.target, changed)
                || incompletely_mapped_file(&edge.source, changed)
            {
                Some(ReviewChangeBasis::ChangedFile)
            } else {
                None
            };
        if let Some(basis) = basis {
            let (source_role, target_role) = match basis {
                ReviewChangeBasis::ChangedDefinition => {
                    ("concrete-reference-source", "concrete-reference-target")
                }
                ReviewChangeBasis::ChangedFile => {
                    ("file-reference-source", "file-reference-target")
                }
            };
            add(
                candidates,
                Path::new(&edge.source.path),
                source_role,
                1,
                Some(Path::new(&edge.target.path)),
            );
            add(
                candidates,
                Path::new(&edge.target.path),
                target_role,
                1,
                Some(Path::new(&edge.source.path)),
            );
            relations.push(ReviewContextRelation {
                side: side.into(),
                kind: "symbol-reference".into(),
                edge: GraphEdge {
                    source: edge.source.path.clone(),
                    target: edge.target.path.clone(),
                    resolver: edge.resolver.clone(),
                },
                change_basis: Some(basis),
                symbol: Some(edge),
                type_relation: None,
            });
        }
    }
}

fn incompletely_mapped_file(
    symbol: &crate::model::CallSymbolIdentity,
    changed: &BTreeMap<PathBuf, &crate::model::ReviewChangedSide>,
) -> bool {
    changed
        .get(Path::new(&symbol.path))
        .is_some_and(|file| mapping_is_incomplete(file))
}

fn mapping_is_incomplete(file: &crate::model::ReviewChangedSide) -> bool {
    file.status == "captured" && file.mapping_status != ReviewAnalysisStatus::Available
}

fn type_candidates(
    candidates: &mut BTreeMap<PathBuf, Candidate>,
    relations: &mut Vec<ReviewContextRelation>,
    side: &str,
    seeds: &BTreeSet<PathBuf>,
    graph: &crate::model::DepGraph,
) {
    let symbols: BTreeMap<_, _> = graph
        .symbols
        .iter()
        .map(|symbol| (&symbol.id, symbol))
        .collect();
    let edges: Vec<_> = graph
        .symbol_edges
        .iter()
        .filter_map(|edge| {
            Some((
                GraphEdge {
                    source: symbols.get(&edge.source)?.path.clone(),
                    target: symbols.get(&edge.target)?.path.clone(),
                    resolver: edge.resolver.clone(),
                },
                edge,
            ))
        })
        .collect();
    let file_edges: Vec<_> = edges.iter().map(|(edge, _)| edge.clone()).collect();
    for (path, evidence) in neighborhood(seeds, &file_edges) {
        for role in evidence.roles {
            if role != "changed" {
                add(
                    candidates,
                    &path,
                    &format!("type-{role}"),
                    evidence.distance,
                    evidence.via.as_deref(),
                );
            }
        }
    }
    for (edge, symbol) in edges {
        if candidates.contains_key(Path::new(&edge.source))
            && candidates.contains_key(Path::new(&edge.target))
        {
            relations.push(ReviewContextRelation {
                side: side.into(),
                kind: "type-relationship".into(),
                edge,
                change_basis: None,
                symbol: None,
                type_relation: Some(symbol.clone()),
            });
        }
    }
}

fn neighborhood(seeds: &BTreeSet<PathBuf>, edges: &[GraphEdge]) -> BTreeMap<PathBuf, Candidate> {
    let mut candidates = BTreeMap::new();
    let mut incoming = BTreeMap::<PathBuf, BTreeSet<PathBuf>>::new();
    for edge in edges {
        incoming
            .entry(PathBuf::from(&edge.target))
            .or_default()
            .insert(PathBuf::from(&edge.source));
        if seeds.contains(Path::new(&edge.source)) {
            add(
                &mut candidates,
                Path::new(&edge.target),
                "dependency",
                1,
                Some(Path::new(&edge.source)),
            );
        }
    }
    let mut distances = BTreeMap::new();
    let mut pending = VecDeque::new();
    for seed in seeds {
        add(&mut candidates, seed, "changed", 0, None);
        distances.insert(seed.clone(), 0usize);
        pending.push_back(seed.clone());
    }
    while let Some(path) = pending.pop_front() {
        let distance = distances[&path] + 1;
        for dependent in incoming.get(&path).into_iter().flatten() {
            if distances.contains_key(dependent) {
                continue;
            }
            distances.insert(dependent.clone(), distance);
            add(
                &mut candidates,
                dependent,
                if distance == 1 {
                    "direct-dependent"
                } else {
                    "transitive-dependent"
                },
                distance,
                Some(&path),
            );
            pending.push_back(dependent.clone());
        }
    }
    candidates
}

fn add(
    candidates: &mut BTreeMap<PathBuf, Candidate>,
    path: &Path,
    role: &str,
    distance: usize,
    via: Option<&Path>,
) {
    let item = candidates
        .entry(path.to_path_buf())
        .or_insert_with(|| Candidate {
            distance,
            via: via.map(Path::to_path_buf),
            ..Candidate::default()
        });
    if distance < item.distance {
        item.distance = distance;
        item.via = via.map(Path::to_path_buf);
    }
    item.roles.insert(role.into());
}

fn describe(
    revision: &ReviewRevision,
    side: &str,
    path: &Path,
    evidence: Candidate,
    counter: &TokenCounter,
) -> ReviewContextFile {
    let file = revision.files().get(path);
    let mut tests = Vec::new();
    if testcov::is_test_file(&path.to_string_lossy()) {
        tests.push("filename-convention".into());
    }
    if file
        .and_then(|file| file.report.as_ref())
        .is_some_and(|report| report.has_inline_tests)
    {
        tests.push("rust-inline-syntax".into());
    }
    ReviewContextFile {
        side: side.into(),
        snapshot: revision.tree.clone(),
        path: path.to_path_buf(),
        status: file
            .map_or_else(
                || {
                    revision
                        .sources
                        .failures
                        .get(path)
                        .map_or("not-captured", |failure| {
                            crate::scan::failure_name(*failure)
                        })
                },
                |_| "captured",
            )
            .into(),
        sha256: file.map(|file| sha256(&file.content)),
        bytes: file.map(|file| file.content.len()),
        tokens: file.map(|file| counter.count(&file.content)),
        roles: evidence.roles.into_iter().collect(),
        distance: evidence.distance,
        via: evidence.via,
        test_evidence: tests,
        selection: "not-requested".into(),
        source: None,
    }
}

fn priority(file: &ReviewContextFile) -> u8 {
    if file.roles.iter().any(|role| role == "changed") {
        0
    } else if !file.test_evidence.is_empty() {
        1
    } else if file.distance == 1 {
        2
    } else {
        3
    }
}
