use super::{
    BTreeMap, BTreeSet, DepGraph, GraphDirection, GraphEdge, GraphFile, GraphNode, GraphQuery,
    GraphSignals, GraphSymbol, GraphSymbolEdge, GraphSymbolReach, HashMap, HashSet, ImpactAnalysis,
    Path, PathBuf, Topology, VecDeque, detect, normalize_path,
};

struct ProjectionSelection {
    focus: Vec<String>,
    unmatched_focus: Vec<String>,
    distances: Vec<Option<usize>>,
    selected: HashSet<usize>,
    selected_paths: HashSet<String>,
}

struct SymbolProjection {
    symbols: Vec<GraphSymbol>,
    edges: Vec<GraphSymbolEdge>,
    reach_by_path: HashMap<String, GraphSymbolReach>,
}

#[derive(Clone, Copy)]
enum FanMetric {
    In,
    Out,
}

impl FanMetric {
    fn file_value(self, file: &GraphFile) -> usize {
        match self {
            Self::In => file.fan_in,
            Self::Out => file.fan_out,
        }
    }

    fn node_value(self, node: &GraphNode) -> usize {
        match self {
            Self::In => node.fan_in,
            Self::Out => node.fan_out,
        }
    }
}

fn select_projection(topology: &Topology, query: &GraphQuery<'_>) -> ProjectionSelection {
    let mut focus = query
        .focus
        .iter()
        .map(|path| normalize_graph_focus(path, query.root))
        .collect::<Vec<_>>();
    focus.sort();
    focus.dedup();

    let mut distances = vec![None; topology.graph_files.len()];
    let mut unmatched_focus = Vec::new();
    if focus.is_empty() {
        distances.fill(Some(0));
    } else {
        fill_focus_distances(
            topology,
            &focus,
            query.direction,
            query.depth,
            &mut distances,
            &mut unmatched_focus,
        );
    }
    let selected = distances
        .iter()
        .enumerate()
        .filter_map(|(index, distance)| distance.map(|_| index))
        .collect::<HashSet<_>>();
    let selected_paths = selected
        .iter()
        .map(|&index| topology.graph_files[index].clone())
        .collect();
    ProjectionSelection {
        focus,
        unmatched_focus,
        distances,
        selected,
        selected_paths,
    }
}

fn fill_focus_distances(
    topology: &Topology,
    focus: &[String],
    direction: GraphDirection,
    depth: usize,
    distances: &mut [Option<usize>],
    unmatched_focus: &mut Vec<String>,
) {
    let mut queue = VecDeque::new();
    for focus_path in focus {
        let mut matched = false;
        for (index, path) in topology.graph_files.iter().enumerate() {
            if graph_focus_matches(path, focus_path) {
                matched = true;
                if distances[index].is_none() {
                    distances[index] = Some(0);
                    queue.push_back(index);
                }
            }
        }
        if !matched {
            unmatched_focus.push(focus_path.clone());
        }
    }

    let mut dependencies = vec![Vec::new(); topology.graph_files.len()];
    let mut dependents = vec![Vec::new(); topology.graph_files.len()];
    for &(source, target) in &topology.edges {
        dependencies[source].push(target);
        dependents[target].push(source);
    }
    while let Some(index) = queue.pop_front() {
        let distance = distances[index].unwrap_or(0);
        if distance >= depth {
            continue;
        }
        let neighbors: Vec<usize> = match direction {
            GraphDirection::Dependencies => dependencies[index].clone(),
            GraphDirection::Dependents => dependents[index].clone(),
            GraphDirection::Both => dependencies[index]
                .iter()
                .chain(&dependents[index])
                .copied()
                .collect(),
        };
        for neighbor in neighbors {
            if distances[neighbor].is_none() {
                distances[neighbor] = Some(distance.saturating_add(1));
                queue.push_back(neighbor);
            }
        }
    }
}

fn project_symbols(topology: &Topology, selected_paths: &HashSet<String>) -> SymbolProjection {
    let mut symbols = topology
        .symbols
        .iter()
        .filter(|symbol| selected_paths.contains(&symbol.path))
        .cloned()
        .collect::<Vec<_>>();
    let selected_symbol_ids = symbols
        .iter()
        .map(|symbol| symbol.id.as_str())
        .collect::<HashSet<_>>();
    let edges = topology
        .symbol_edges
        .iter()
        .filter(|edge| {
            selected_symbol_ids.contains(edge.source.as_str())
                && selected_symbol_ids.contains(edge.target.as_str())
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut fan_in = HashMap::<&str, usize>::new();
    let mut fan_out = HashMap::<&str, usize>::new();
    for edge in &edges {
        *fan_out.entry(edge.source.as_str()).or_default() += 1;
        *fan_in.entry(edge.target.as_str()).or_default() += 1;
    }
    for symbol in &mut symbols {
        symbol.fan_in = fan_in.get(symbol.id.as_str()).copied().unwrap_or_default();
        symbol.fan_out = fan_out.get(symbol.id.as_str()).copied().unwrap_or_default();
    }
    let reach_by_path = symbol_reach_by_path(&symbols, &edges);
    SymbolProjection {
        symbols,
        edges,
        reach_by_path,
    }
}

fn project_files(
    topology: &Topology,
    signals: &GraphSignals,
    selection: &ProjectionSelection,
    symbol_reach: &HashMap<String, GraphSymbolReach>,
) -> Vec<GraphFile> {
    topology
        .graph_files
        .iter()
        .enumerate()
        .filter(|(index, _)| selection.selected.contains(index))
        .map(|(index, path)| {
            let signal = signals.files.get(path).cloned().unwrap_or_default();
            let dependencies = signal
                .dependencies
                .into_iter()
                .filter(|path| selection.selected_paths.contains(path))
                .collect::<Vec<_>>();
            let dependents = signal
                .dependents
                .into_iter()
                .filter(|path| selection.selected_paths.contains(path))
                .collect::<Vec<_>>();
            GraphFile {
                path: path.clone(),
                language: detect(Path::new(path))
                    .map(|language| language.name.to_string())
                    .unwrap_or_default(),
                fan_in: dependents.len(),
                fan_out: dependencies.len(),
                dependencies,
                dependents,
                focus_distance: (!selection.focus.is_empty())
                    .then_some(selection.distances[index].unwrap_or(0)),
                symbol_reach: symbol_reach.get(path).cloned(),
            }
        })
        .collect()
}

fn project_edges(topology: &Topology, selected: &HashSet<usize>) -> Vec<GraphEdge> {
    topology
        .edges
        .iter()
        .filter(|(source, target)| selected.contains(source) && selected.contains(target))
        .map(|&(source, target)| GraphEdge {
            source: topology.graph_files[source].clone(),
            target: topology.graph_files[target].clone(),
            resolver: topology
                .edge_resolvers
                .get(&(source, target))
                .cloned()
                .unwrap_or_else(|| "heuristic".to_string()),
        })
        .collect()
}

fn top_nodes(files: &[GraphFile], metric: FanMetric) -> Vec<GraphNode> {
    let mut nodes = files
        .iter()
        .filter(|file| metric.file_value(file) > 0)
        .map(|file| GraphNode {
            path: file.path.clone(),
            fan_in: file.fan_in,
            fan_out: file.fan_out,
        })
        .collect::<Vec<_>>();
    nodes.sort_by(|left, right| {
        metric
            .node_value(right)
            .cmp(&metric.node_value(left))
            .then_with(|| left.path.cmp(&right.path))
    });
    nodes.truncate(10);
    nodes
}

pub(super) fn project_graph(
    topology: &Topology,
    signals: &GraphSignals,
    cycles: Vec<Vec<String>>,
    orphans: Vec<String>,
    query: GraphQuery<'_>,
) -> DepGraph {
    let selection = select_projection(topology, &query);

    let symbol_projection = project_symbols(topology, &selection.selected_paths);

    let files = project_files(
        topology,
        signals,
        &selection,
        &symbol_projection.reach_by_path,
    );
    let edge_list = project_edges(topology, &selection.selected);

    let top_depended = top_nodes(&files, FanMetric::In);
    let most_dependent = top_nodes(&files, FanMetric::Out);

    let languages = files
        .iter()
        .map(|file| file.language.clone())
        .filter(|language| !language.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let cycles = cycles
        .into_iter()
        .filter(|cycle| {
            cycle
                .iter()
                .all(|path| selection.selected_paths.contains(path))
        })
        .collect();
    let orphans = orphans
        .into_iter()
        .filter(|path| selection.selected_paths.contains(path))
        .collect();
    let unresolved_imports = selection
        .selected
        .iter()
        .map(|&index| topology.unresolved_by_node[index])
        .sum();
    let parse_errors = selection
        .selected
        .iter()
        .map(|&index| topology.parse_errors_by_node[index])
        .sum();
    let unresolved_symbol_relations =
        if selection.selected_paths.len() == topology.graph_files.len() {
            topology.unresolved_symbol_relations
        } else {
            selection
                .selected_paths
                .iter()
                .map(|path| {
                    topology
                        .unresolved_symbol_relations_by_path
                        .get(path)
                        .copied()
                        .unwrap_or_default()
                })
                .sum()
        };
    let focus = selection.focus;
    let unmatched_focus = selection.unmatched_focus;

    DepGraph {
        languages,
        nodes: files.len(),
        edges: edge_list.len(),
        files,
        edge_list,
        symbols: symbol_projection.symbols,
        symbol_edges: symbol_projection.edges,
        unresolved_symbol_relations,
        focus,
        unmatched_focus,
        direction: if query.focus.is_empty() {
            "all".to_string()
        } else {
            query.direction.as_str().to_string()
        },
        depth: (!query.focus.is_empty()).then_some(query.depth),
        cycles,
        orphans,
        top_depended,
        most_dependent,
        unresolved_imports,
        parse_errors,
        config_errors: topology.config_errors,
        config_files: topology.config_files.clone(),
    }
}

pub(super) fn symbol_reach_by_path(
    symbols: &[GraphSymbol],
    edges: &[GraphSymbolEdge],
) -> HashMap<String, GraphSymbolReach> {
    let mut incoming = HashMap::<&str, BTreeMap<&str, usize>>::new();
    let mut outgoing = HashMap::<&str, BTreeMap<&str, usize>>::new();
    for edge in edges {
        *incoming
            .entry(edge.target.as_str())
            .or_default()
            .entry(edge.relation.as_str())
            .or_default() += 1;
        *outgoing
            .entry(edge.source.as_str())
            .or_default()
            .entry(edge.relation.as_str())
            .or_default() += 1;
    }

    let mut by_path = HashMap::<String, GraphSymbolReach>::new();
    for symbol in symbols {
        let relation = dominant_relation(
            incoming
                .get(symbol.id.as_str())
                .or_else(|| outgoing.get(symbol.id.as_str())),
        );
        let candidate = GraphSymbolReach {
            symbol_id: symbol.id.clone(),
            name: symbol.name.clone(),
            kind: symbol.kind.clone(),
            fan_in: symbol.fan_in,
            fan_out: symbol.fan_out,
            relation,
        };
        let replace = by_path.get(&symbol.path).is_none_or(|current| {
            candidate
                .fan_in
                .cmp(&current.fan_in)
                .then_with(|| candidate.fan_out.cmp(&current.fan_out))
                .then_with(|| current.name.cmp(&candidate.name))
                .is_gt()
        });
        if replace {
            by_path.insert(symbol.path.clone(), candidate);
        }
    }
    by_path
}

pub(super) fn dominant_relation(relations: Option<&BTreeMap<&str, usize>>) -> String {
    relations
        .and_then(|relations| {
            relations
                .iter()
                .max_by(|left, right| left.1.cmp(right.1).then_with(|| right.0.cmp(left.0)))
        })
        .map(|(relation, _)| (*relation).to_string())
        .unwrap_or_default()
}

pub(super) fn normalize_graph_focus(path: &Path, root: &Path) -> String {
    let relative = if path.is_absolute() {
        path.strip_prefix(root).unwrap_or(path)
    } else {
        path
    };
    normalize_path(&relative.to_string_lossy().replace('\\', "/"))
        .trim_end_matches('/')
        .to_string()
}

pub(super) fn graph_focus_matches(path: &str, focus: &str) -> bool {
    focus.is_empty()
        || path == focus
        || path
            .strip_prefix(focus)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

pub(super) fn impact_from_topology<S>(
    topology: &Topology,
    changed: &HashSet<PathBuf, S>,
) -> ImpactAnalysis
where
    S: std::hash::BuildHasher,
{
    let mut changed_files: Vec<String> = changed
        .iter()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .collect();
    changed_files.sort();

    let node_index: HashMap<&str, usize> = topology
        .graph_files
        .iter()
        .enumerate()
        .map(|(index, path)| (path.as_str(), index))
        .collect();
    let changed_indices: HashSet<usize> = changed_files
        .iter()
        .filter_map(|path| node_index.get(path.as_str()).copied())
        .collect();
    let mut graph_changed_files: Vec<String> = changed_indices
        .iter()
        .map(|&index| topology.graph_files[index].clone())
        .collect();
    graph_changed_files.sort();

    let mut reverse = vec![Vec::new(); topology.graph_files.len()];
    for &(importer, imported) in &topology.edges {
        if importer < reverse.len() && imported < reverse.len() {
            reverse[imported].push(importer);
        }
    }

    let mut distance = vec![None; topology.graph_files.len()];
    let mut queue = VecDeque::new();
    for &index in &changed_indices {
        distance[index] = Some(0usize);
        queue.push_back(index);
    }
    while let Some(imported) = queue.pop_front() {
        let next_distance = distance[imported].unwrap_or(0) + 1;
        for &dependent in &reverse[imported] {
            if distance[dependent].is_none() {
                distance[dependent] = Some(next_distance);
                queue.push_back(dependent);
            }
        }
    }

    let mut direct_dependents = Vec::new();
    let mut transitive_dependents = Vec::new();
    for (index, path) in topology.graph_files.iter().enumerate() {
        if changed_indices.contains(&index) {
            continue;
        }
        match distance[index] {
            Some(1) => direct_dependents.push(path.clone()),
            Some(_) => transitive_dependents.push(path.clone()),
            None => {}
        }
    }

    let confidence = if graph_changed_files.is_empty() {
        "none"
    } else if graph_changed_files.len() == changed_files.len()
        && topology.unresolved_imports == 0
        && topology.unreadable_nodes.is_empty()
        && topology.parse_errors == 0
        && topology.config_errors == 0
    {
        "high"
    } else {
        "partial"
    };

    ImpactAnalysis {
        changed_files,
        graph_changed_files,
        direct_dependents,
        transitive_dependents,
        unresolved_imports: topology.unresolved_imports,
        parse_errors: topology.parse_errors,
        config_errors: topology.config_errors,
        confidence: confidence.to_string(),
    }
}
