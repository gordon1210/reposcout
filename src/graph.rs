//! Structural graph for every first-class language.
//!
//! Language scope: JavaScript (.js, .jsx, .mjs, .cjs), TypeScript (.ts, .tsx),
//! Python (.py, .pyi, .pyw), PHP (including common framework extensions), Rust,
//! and Go. File-oriented languages resolve directly to source files. Rust module
//! paths resolve through Cargo package/module roots; Go package imports resolve
//! to a deterministic representative file and retain resolver provenance so the
//! file-level approximation remains visible to callers.
//!
//! This module is self-contained and used by opt-in graph, impact, explain, and
//! context features. It consumes cached per-file source facts when a scan has
//! already parsed the source. Any residual filesystem access is budgeted,
//! symlink-safe, and optional for sources when facts are supplied.

mod symbols;

use crate::cli::GraphDirection;
use crate::fs_budget::{self, ReadBudget, ReadOutcome};
use crate::lang::{FirstClass, detect};
use crate::metrics::testcov;
use crate::model::{
    DepGraph, FileGraphContext, GraphEdge, GraphFile, GraphNode, GraphSymbol, GraphSymbolEdge,
    GraphSymbolReach, ImpactAnalysis,
};
use crate::parse;
use crate::php::{self, StaticInclude};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use tree_sitter::Node;

/// Resource and trust policy for graph construction.
#[derive(Debug, Clone)]
pub struct GraphReadLimits {
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
    pub max_files: usize,
    /// When true, source files are never re-read from the filesystem; missing
    /// facts mark a node unreadable instead.
    pub facts_only_sources: bool,
    /// Cooperative deadline shared with repository config reads.
    pub deadline: Option<std::time::Instant>,
}

impl Default for GraphReadLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 32 * 1024 * 1024,
            max_total_bytes: 512 * 1024 * 1024,
            max_files: 100_000,
            facts_only_sources: false,
            deadline: None,
        }
    }
}

impl GraphReadLimits {
    pub fn from_config(cfg: &crate::config::Config) -> Self {
        Self {
            max_file_bytes: cfg.max_file_bytes,
            max_total_bytes: cfg.max_total_bytes,
            max_files: cfg.max_files,
            facts_only_sources: false,
            deadline: None,
        }
    }

    fn budget(&self) -> ReadBudget {
        ReadBudget::new(
            self.max_file_bytes,
            self.max_total_bytes,
            self.max_files,
            self.deadline,
        )
    }
}

/// Revision-scoped inputs for graph construction.
#[derive(Debug, Clone, Default)]
pub struct GraphInputs {
    pub source_facts: BTreeMap<PathBuf, SourceFacts>,
    /// Relative repository paths to immutable resolver config contents.
    pub resolver_configs: BTreeMap<String, String>,
}

struct Topology {
    graph_files: Vec<String>,
    edges: Vec<(usize, usize)>,
    unresolved_imports: usize,
    unresolved_by_node: Vec<usize>,
    parse_errors_by_node: Vec<usize>,
    unreadable_nodes: HashSet<usize>,
    parse_errors: usize,
    edge_resolvers: BTreeMap<(usize, usize), String>,
    config_errors: usize,
    config_errors_by_path: BTreeMap<String, usize>,
    config_files: Vec<String>,
    symbols: Vec<GraphSymbol>,
    symbol_edges: Vec<GraphSymbolEdge>,
    unresolved_symbol_relations: usize,
    unresolved_symbol_relations_by_path: HashMap<String, usize>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct GraphFileSignal {
    pub fan_in: usize,
    pub fan_out: usize,
    pub dependencies: Vec<String>,
    pub dependents: Vec<String>,
    pub dependency_resolvers: BTreeMap<String, String>,
    pub dependent_resolvers: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct GraphSignals {
    pub languages: Vec<String>,
    pub files: HashMap<String, GraphFileSignal>,
    pub unresolved_imports: usize,
    pub parse_errors: usize,
    pub config_errors: usize,
}

pub(crate) struct GraphAnalysis {
    pub report: DepGraph,
    pub signals: GraphSignals,
    topology: Topology,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct GraphDiagnosticFact {
    pub path: String,
    pub unreadable: bool,
    pub parse_errors: usize,
    pub unresolved_imports: usize,
    pub config_errors: usize,
}

struct GraphQuery<'a> {
    root: &'a Path,
    focus: &'a [PathBuf],
    direction: GraphDirection,
    depth: usize,
}

/// Build structural topology from the already-scanned file list.
pub fn build(files: &[crate::model::FileReport], root: &Path) -> DepGraph {
    build_with_limits(files, root, GraphReadLimits::default(), None)
}

/// Build structural topology with explicit read limits and optional source facts.
pub fn build_with_limits(
    files: &[crate::model::FileReport],
    root: &Path,
    limits: GraphReadLimits,
    facts: Option<&BTreeMap<PathBuf, SourceFacts>>,
) -> DepGraph {
    analyze_with_limits(
        files,
        root,
        limits,
        facts,
        None,
        &[],
        GraphDirection::Both,
        1,
    )
    .report
}

/// Build topology from fully revisioned graph inputs (sources + resolver configs).
pub fn build_with_inputs(
    files: &[crate::model::FileReport],
    root: &Path,
    limits: GraphReadLimits,
    inputs: &GraphInputs,
) -> DepGraph {
    analyze_with_limits(
        files,
        root,
        GraphReadLimits {
            facts_only_sources: true,
            ..limits
        },
        Some(&inputs.source_facts),
        Some(&inputs.resolver_configs),
        &[],
        GraphDirection::Both,
        1,
    )
    .report
}

/// Collect bounded resolver configuration files for the given first-class paths.
///
/// For TypeScript/JavaScript, relative `extends` and project `references` are
/// followed recursively so snapshot-mode graph builds see the same configs as a
/// live walk under the same byte budget.
pub fn collect_resolver_configs(
    root: &Path,
    files: &[PathBuf],
    limits: &GraphReadLimits,
) -> BTreeMap<String, String> {
    let graph_files = files
        .iter()
        .filter_map(|path| {
            detect(path).and_then(|info| {
                info.first_class
                    .map(|_| path.to_string_lossy().replace('\\', "/"))
            })
        })
        .collect::<Vec<_>>();
    let mut budget = limits.budget();
    let mut configs = BTreeMap::new();
    let mut pending = candidate_resolver_config_paths(root, &graph_files)
        .into_iter()
        .collect::<VecDeque<_>>();
    let mut seen = HashSet::new();
    while let Some(relative) = pending.pop_front() {
        if !seen.insert(relative.clone()) {
            continue;
        }
        let Some(content) = read_repo_text(root, &relative, &mut budget) else {
            continue;
        };
        // Follow local extends/references for TS/JS configs and any JSON fragment
        // pulled in through those links (e.g. configs/base.json).
        if looks_like_ts_config(&relative, &content) {
            for related in ts_config_related_paths(root, &relative, &content) {
                if !seen.contains(&related) {
                    pending.push_back(related);
                }
            }
        }
        configs.insert(relative, content);
    }
    configs
}

fn looks_like_ts_config(relative: &str, content: &str) -> bool {
    let name = Path::new(relative)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(relative);
    if name == "package.json" || name == "composer.json" {
        return false;
    }
    name == "tsconfig.json"
        || name == "jsconfig.json"
        || (name.starts_with("tsconfig.") && name.ends_with(".json"))
        || (name.starts_with("jsconfig.") && name.ends_with(".json"))
        || (relative.ends_with(".json")
            && (content.contains("\"compilerOptions\"")
                || content.contains("\"extends\"")
                || content.contains("\"references\"")))
}

/// Local relative targets of `extends` / `references` for a TS/JS config.
fn ts_config_related_paths(root: &Path, relative: &str, content: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<Value>(&sanitize_jsonc(content)) else {
        return Vec::new();
    };
    let directory = path_parent(relative);
    let mut related = Vec::new();
    match value.get("extends") {
        Some(Value::String(extended)) => {
            if let Some(path) = resolve_related_config_path(root, &directory, extended) {
                related.push(path);
            }
        }
        Some(Value::Array(items)) => {
            for item in items {
                if let Some(extended) = item.as_str()
                    && let Some(path) = resolve_related_config_path(root, &directory, extended)
                {
                    related.push(path);
                }
            }
        }
        _ => {}
    }
    if let Some(references) = value.get("references").and_then(Value::as_array) {
        for reference in references {
            if let Some(path) = reference.get("path").and_then(Value::as_str)
                && let Some(resolved) = resolve_related_config_path(root, &directory, path)
            {
                related.push(resolved);
            }
        }
    }
    related.sort();
    related.dedup();
    related
}

fn resolve_related_config_path(root: &Path, directory: &str, reference: &str) -> Option<String> {
    if !reference.starts_with('.') {
        return None;
    }
    let mut candidate = join_graph_path(directory, reference);
    let absolute = root.join(&candidate);
    let metadata = std::fs::symlink_metadata(&absolute).ok();
    if metadata
        .as_ref()
        .is_some_and(|meta| meta.is_dir() && !meta.file_type().is_symlink())
    {
        candidate = join_graph_path(&candidate, "tsconfig.json");
    } else if !metadata
        .as_ref()
        .is_some_and(|meta| meta.is_file() && !meta.file_type().is_symlink())
        && Path::new(&candidate).extension().is_none()
    {
        candidate.push_str(".json");
    }
    repo_is_regular_file(root, &candidate).then_some(candidate)
}

/// Build a complete topology once, then project a deterministic bounded graph
/// around optional file or directory focus paths.
#[cfg(test)]
pub(crate) fn analyze_with_query(
    files: &[crate::model::FileReport],
    root: &Path,
    focus: &[PathBuf],
    direction: GraphDirection,
    depth: usize,
) -> GraphAnalysis {
    analyze_with_limits(
        files,
        root,
        GraphReadLimits::default(),
        None,
        None,
        focus,
        direction,
        depth,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn analyze_with_query_facts(
    files: &[crate::model::FileReport],
    root: &Path,
    facts: &BTreeMap<PathBuf, SourceFacts>,
    resolver_configs: Option<&BTreeMap<String, String>>,
    limits: GraphReadLimits,
    focus: &[PathBuf],
    direction: GraphDirection,
    depth: usize,
) -> GraphAnalysis {
    analyze_with_limits(
        files,
        root,
        GraphReadLimits {
            facts_only_sources: true,
            ..limits
        },
        Some(facts),
        resolver_configs,
        focus,
        direction,
        depth,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn analyze_with_limits(
    files: &[crate::model::FileReport],
    root: &Path,
    limits: GraphReadLimits,
    facts: Option<&BTreeMap<PathBuf, SourceFacts>>,
    resolver_configs: Option<&BTreeMap<String, String>>,
    focus: &[PathBuf],
    direction: GraphDirection,
    depth: usize,
) -> GraphAnalysis {
    let paths = files
        .iter()
        .map(|file| file.path.clone())
        .collect::<Vec<_>>();
    build_from_paths_with_query(
        &paths,
        root,
        &HashSet::new(),
        facts,
        resolver_configs,
        limits,
        focus,
        direction,
        depth,
    )
}

/// Build reusable full-tree signals from discovered paths, including changed
/// paths that no longer exist in the current worktree. Change-aware context
/// planning and impact share this topology rather than implementing another
/// import walk.
#[cfg(test)]
pub(crate) fn analyze_paths(
    paths: &[PathBuf],
    root: &Path,
    virtual_paths: &HashSet<String>,
) -> GraphAnalysis {
    build_from_paths_with_query(
        paths,
        root,
        virtual_paths,
        None,
        None,
        GraphReadLimits::default(),
        &[],
        GraphDirection::Both,
        1,
    )
}

pub(crate) fn analyze_paths_with_facts(
    paths: &[PathBuf],
    root: &Path,
    virtual_paths: &HashSet<String>,
    facts: &BTreeMap<PathBuf, SourceFacts>,
    resolver_configs: Option<&BTreeMap<String, String>>,
    limits: GraphReadLimits,
) -> GraphAnalysis {
    build_from_paths_with_query(
        paths,
        root,
        virtual_paths,
        Some(facts),
        resolver_configs,
        GraphReadLimits {
            facts_only_sources: true,
            ..limits
        },
        &[],
        GraphDirection::Both,
        1,
    )
}

pub(crate) fn impact_from_analysis(
    analysis: &GraphAnalysis,
    changed: &HashSet<PathBuf>,
) -> ImpactAnalysis {
    impact_from_topology(&analysis.topology, changed)
}

pub(crate) fn diagnostic_facts(analysis: &GraphAnalysis) -> Vec<GraphDiagnosticFact> {
    let topology = &analysis.topology;
    let mut facts = BTreeMap::new();
    for (index, path) in topology.graph_files.iter().enumerate() {
        let unreadable = topology.unreadable_nodes.contains(&index);
        let parse_errors = topology.parse_errors_by_node[index];
        let unresolved_imports = topology.unresolved_by_node[index];
        if unreadable || parse_errors > 0 || unresolved_imports > 0 {
            facts.insert(
                path.clone(),
                GraphDiagnosticFact {
                    path: path.clone(),
                    unreadable,
                    parse_errors,
                    unresolved_imports,
                    config_errors: 0,
                },
            );
        }
    }
    for (path, config_errors) in &topology.config_errors_by_path {
        facts
            .entry(path.clone())
            .or_insert_with(|| GraphDiagnosticFact {
                path: path.clone(),
                ..GraphDiagnosticFact::default()
            })
            .config_errors = *config_errors;
    }
    facts.into_values().collect()
}

/// Return direct graph context for one already-scanned file.
pub fn explain(files: &[crate::model::FileReport], root: &Path, path: &Path) -> FileGraphContext {
    let paths: Vec<PathBuf> = files.iter().map(|file| file.path.clone()).collect();
    let built = build_from_paths(&paths, root, &HashSet::new());
    explain_from_analysis(&built, path)
}

pub(crate) fn explain_with_facts(
    files: &[crate::model::FileReport],
    root: &Path,
    path: &Path,
    facts: &BTreeMap<PathBuf, SourceFacts>,
    resolver_configs: Option<&BTreeMap<String, String>>,
    limits: GraphReadLimits,
) -> FileGraphContext {
    let paths = files
        .iter()
        .map(|file| file.path.clone())
        .collect::<Vec<_>>();
    let built = build_from_paths_with_query(
        &paths,
        root,
        &HashSet::new(),
        Some(facts),
        resolver_configs,
        GraphReadLimits {
            facts_only_sources: true,
            ..limits
        },
        &[],
        GraphDirection::Both,
        1,
    );
    explain_from_analysis(&built, path)
}

fn explain_from_analysis(built: &GraphAnalysis, path: &Path) -> FileGraphContext {
    let target = path.to_string_lossy().replace('\\', "/");
    let Some(index) = built
        .topology
        .graph_files
        .iter()
        .position(|candidate| candidate == &target)
    else {
        return FileGraphContext::default();
    };

    let mut dependencies = built
        .topology
        .edges
        .iter()
        .filter(|&&(importer, _)| importer == index)
        .map(|&(_, imported)| built.topology.graph_files[imported].clone())
        .collect::<Vec<_>>();
    let mut dependents = built
        .topology
        .edges
        .iter()
        .filter(|&&(_, imported)| imported == index)
        .map(|&(importer, _)| built.topology.graph_files[importer].clone())
        .collect::<Vec<_>>();
    dependencies.sort();
    dependents.sort();
    FileGraphContext {
        supported: true,
        fan_in: dependents.len(),
        fan_out: dependencies.len(),
        dependencies,
        dependents,
        cycles: built
            .report
            .cycles
            .iter()
            .filter(|cycle| cycle.contains(&target))
            .cloned()
            .collect(),
        unresolved_imports: built.topology.unresolved_by_node[index],
    }
}

/// Build full topology for a diff-scoped scan, then report which unchanged
/// graph files directly or transitively depend on its changed files.
pub fn impact(paths: &[PathBuf], root: &Path, changed: &HashSet<PathBuf>) -> ImpactAnalysis {
    let existing_paths: HashSet<String> = paths
        .iter()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .collect();
    let virtual_paths: HashSet<String> = changed
        .iter()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .filter(|path| !existing_paths.contains(path))
        .collect();
    let mut topology_paths = paths.to_vec();
    topology_paths.extend(
        virtual_paths
            .iter()
            .map(|path| PathBuf::from(path.as_str())),
    );

    let built = build_from_paths(&topology_paths, root, &virtual_paths);
    impact_from_topology(&built.topology, changed)
}

fn build_from_paths(
    paths: &[PathBuf],
    root: &Path,
    virtual_paths: &HashSet<String>,
) -> GraphAnalysis {
    build_from_paths_with_query(
        paths,
        root,
        virtual_paths,
        None,
        None,
        GraphReadLimits::default(),
        &[],
        GraphDirection::Both,
        1,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_from_paths_with_query(
    paths: &[PathBuf],
    root: &Path,
    virtual_paths: &HashSet<String>,
    source_facts: Option<&BTreeMap<PathBuf, SourceFacts>>,
    resolver_configs: Option<&BTreeMap<String, String>>,
    limits: GraphReadLimits,
    focus: &[PathBuf],
    direction: GraphDirection,
    depth: usize,
) -> GraphAnalysis {
    let mut budget = limits.budget();
    // Step 1: select every first-class file.
    let mut graph_files: Vec<String> = Vec::new();
    let mut lang_set: HashSet<String> = HashSet::new();

    for path in paths {
        if let Some(info) = detect(path)
            && info.first_class.is_some()
        {
            let path_str = path.to_string_lossy().replace('\\', "/");
            graph_files.push(path_str);
            lang_set.insert(info.name.to_string());
        }
    }

    graph_files.sort();
    graph_files.dedup();
    let node_set: HashSet<String> = graph_files.iter().cloned().collect();
    let node_index: HashMap<String, usize> = graph_files
        .iter()
        .enumerate()
        .map(|(i, p)| (p.clone(), i))
        .collect();
    let n = graph_files.len();
    let mut access = ConfigAccess {
        root,
        budget: &mut budget,
        snapshot: resolver_configs,
    };
    let js_resolver = JsResolver::discover(&graph_files, &mut access);
    let python_resolver = PythonResolver::discover(&graph_files);
    let php_resolver = PhpResolver::discover(&graph_files, &mut access);
    let rust_resolver = RustResolver::discover(&graph_files, &mut access);
    let go_resolver = GoResolver::discover(&graph_files, &mut access);

    // Step 2: extract imports and build edges (no self-edges in edge_set).
    let mut edge_set: HashSet<(usize, usize)> = HashSet::new();
    let mut self_cycle_paths: Vec<String> = Vec::new();
    let mut unresolved_imports: usize = 0;
    let mut unresolved_by_node = vec![0usize; n];
    let mut unreadable_nodes = HashSet::new();
    let mut parse_errors = 0usize;
    let mut parse_errors_by_node = vec![0usize; n];
    let mut edge_resolvers = BTreeMap::new();
    let mut symbol_collector = symbols::Collector::default();

    for (i, rel_path) in graph_files.iter().enumerate() {
        let Some(fc) = detect(Path::new(rel_path)).and_then(|info| info.first_class) else {
            continue;
        };
        let extraction = if let Some(facts) = source_facts
            .and_then(|facts| facts.get(Path::new(rel_path)))
            .cloned()
        {
            facts
        } else if limits.facts_only_sources {
            if !virtual_paths.contains(rel_path) {
                unreadable_nodes.insert(i);
            }
            continue;
        } else {
            let abs = if root.is_file() {
                root.to_path_buf()
            } else {
                root.join(rel_path.as_str())
            };
            let content = match fs_budget::read_text(&abs, &mut budget) {
                ReadOutcome::Content(content) => content,
                _ => {
                    if !virtual_paths.contains(rel_path) {
                        unreadable_nodes.insert(i);
                    }
                    continue;
                }
            };
            extract_source_facts(fc, rel_path, &content)
        };
        symbol_collector.add_facts(extraction.symbols);
        parse_errors_by_node[i] = extraction.parse_errors;
        parse_errors = parse_errors.saturating_add(extraction.parse_errors);

        for spec in extraction.specifiers {
            let resolution = match spec {
                ImportSpecifier::Module(spec) if fc == FirstClass::Python => {
                    python_resolver.resolve(rel_path, &spec, &node_set)
                }
                ImportSpecifier::Module(spec) => js_resolver.resolve(rel_path, &spec, &node_set),
                ImportSpecifier::PhpNamespace(symbol) => {
                    php_resolver.resolve_namespace(rel_path, &symbol, &node_set)
                }
                ImportSpecifier::PhpInclude(include) => {
                    php_resolver.resolve_include(rel_path, &include, &node_set)
                }
                ImportSpecifier::Rust(rust_import) => {
                    rust_resolver.resolve(rel_path, &rust_import, &node_set)
                }
                ImportSpecifier::GoPackage(package) => go_resolver.resolve(rel_path, &package),
            };

            match resolution {
                ImportResolution::Resolved { target, resolver } => {
                    if target == *rel_path {
                        if !self_cycle_paths.contains(rel_path) {
                            self_cycle_paths.push(rel_path.clone());
                        }
                    } else if let Some(&j) = node_index.get(&target) {
                        edge_set.insert((i, j));
                        edge_resolvers
                            .entry((i, j))
                            .or_insert_with(|| resolver.to_string());
                    }
                }
                ImportResolution::Unresolved => {
                    unresolved_imports += 1;
                    unresolved_by_node[i] += 1;
                }
                ImportResolution::External => {}
            }
        }
    }

    let symbol_topology = symbol_collector.finish();
    let mut topology_edges: Vec<(usize, usize)> = edge_set.iter().copied().collect();
    topology_edges.sort_unstable();
    let topology = Topology {
        graph_files: graph_files.clone(),
        edges: topology_edges,
        unresolved_imports,
        unresolved_by_node,
        parse_errors_by_node,
        unreadable_nodes,
        parse_errors,
        edge_resolvers,
        config_errors: js_resolver
            .config_errors
            .saturating_add(php_resolver.config_errors)
            .saturating_add(rust_resolver.config_errors)
            .saturating_add(go_resolver.config_errors),
        config_errors_by_path: combined_config_errors([
            &js_resolver.config_errors_by_path,
            &php_resolver.config_errors_by_path,
            &rust_resolver.config_errors_by_path,
            &go_resolver.config_errors_by_path,
        ]),
        config_files: combined_config_files([
            js_resolver.config_files.as_slice(),
            php_resolver.config_files.as_slice(),
            rust_resolver.config_files.as_slice(),
            go_resolver.config_files.as_slice(),
        ]),
        symbols: symbol_topology.symbols,
        symbol_edges: symbol_topology.edges,
        unresolved_symbol_relations: symbol_topology.unresolved_relations,
        unresolved_symbol_relations_by_path: symbol_topology.unresolved_by_path,
    };

    // Step 3: fan_in / fan_out per node.
    let mut fan_out = vec![0usize; n];
    let mut fan_in = vec![0usize; n];
    for &(u, v) in &edge_set {
        fan_out[u] += 1;
        fan_in[v] += 1;
    }

    // Step 4: cycles via Kosaraju SCC.
    let edge_list: Vec<(usize, usize)> = edge_set.iter().copied().collect();
    let sccs = strongly_connected(&graph_files, &edge_list);
    let mut cycles: Vec<Vec<String>> = Vec::new();
    for scc in sccs {
        if scc.len() >= 2 {
            let mut paths: Vec<String> = scc.iter().map(|&i| graph_files[i].clone()).collect();
            paths.sort();
            cycles.push(paths);
        }
    }
    // Self-cycles.
    self_cycle_paths.sort();
    for path in self_cycle_paths {
        cycles.push(vec![path]);
    }
    cycles.sort();

    // Step 5: orphans (fan_in == 0, not entrypoint, not test file).
    let mut orphans: Vec<String> = Vec::new();
    for (i, path) in graph_files.iter().enumerate() {
        let is_non_representative_go_file =
            detect(Path::new(path)).and_then(|info| info.first_class) == Some(FirstClass::Go)
                && go_resolver
                    .packages
                    .get(&path_parent(path))
                    .is_some_and(|representative| representative != path);
        if fan_in[i] == 0
            && !is_entrypoint(path)
            && !testcov::is_test_file(path)
            && !is_non_representative_go_file
        {
            orphans.push(path.clone());
        }
    }
    orphans.sort();

    // Step 6: languages present in the graph.
    let mut languages: Vec<String> = lang_set.into_iter().collect();
    languages.sort();

    let mut dependencies = vec![Vec::new(); n];
    let mut dependents = vec![Vec::new(); n];
    let mut dependency_resolvers = vec![BTreeMap::new(); n];
    let mut dependent_resolvers = vec![BTreeMap::new(); n];
    for &(importer, imported) in &topology.edges {
        dependencies[importer].push(graph_files[imported].clone());
        dependents[imported].push(graph_files[importer].clone());
        let resolver = topology
            .edge_resolvers
            .get(&(importer, imported))
            .cloned()
            .unwrap_or_else(|| "heuristic".to_string());
        dependency_resolvers[importer].insert(graph_files[imported].clone(), resolver.clone());
        dependent_resolvers[imported].insert(graph_files[importer].clone(), resolver);
    }
    let files = graph_files
        .iter()
        .enumerate()
        .map(|(index, path)| {
            dependencies[index].sort();
            dependents[index].sort();
            (
                path.clone(),
                GraphFileSignal {
                    fan_in: fan_in[index],
                    fan_out: fan_out[index],
                    dependencies: std::mem::take(&mut dependencies[index]),
                    dependents: std::mem::take(&mut dependents[index]),
                    dependency_resolvers: std::mem::take(&mut dependency_resolvers[index]),
                    dependent_resolvers: std::mem::take(&mut dependent_resolvers[index]),
                },
            )
        })
        .collect();
    let signals = GraphSignals {
        languages: languages.clone(),
        files,
        unresolved_imports,
        parse_errors,
        config_errors: js_resolver
            .config_errors
            .saturating_add(php_resolver.config_errors)
            .saturating_add(rust_resolver.config_errors)
            .saturating_add(go_resolver.config_errors),
    };

    let report = project_graph(
        &topology,
        &signals,
        cycles,
        orphans,
        GraphQuery {
            root,
            focus,
            direction,
            depth,
        },
    );

    GraphAnalysis {
        topology,
        signals,
        report,
    }
}

fn project_graph(
    topology: &Topology,
    signals: &GraphSignals,
    cycles: Vec<Vec<String>>,
    orphans: Vec<String>,
    query: GraphQuery<'_>,
) -> DepGraph {
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
        let mut queue = VecDeque::new();
        for focus_path in &focus {
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
            if distance >= query.depth {
                continue;
            }
            let neighbors: &[usize] = match query.direction {
                GraphDirection::Dependencies => &dependencies[index],
                GraphDirection::Dependents => &dependents[index],
                GraphDirection::Both => &[],
            };
            if query.direction == GraphDirection::Both {
                for neighbor in dependencies[index]
                    .iter()
                    .chain(dependents[index].iter())
                    .copied()
                {
                    if distances[neighbor].is_none() {
                        distances[neighbor] = Some(distance + 1);
                        queue.push_back(neighbor);
                    }
                }
            } else {
                for &neighbor in neighbors {
                    if distances[neighbor].is_none() {
                        distances[neighbor] = Some(distance + 1);
                        queue.push_back(neighbor);
                    }
                }
            }
        }
    }

    let selected = distances
        .iter()
        .enumerate()
        .filter_map(|(index, distance)| distance.map(|_| index))
        .collect::<HashSet<_>>();
    let selected_paths = selected
        .iter()
        .map(|&index| topology.graph_files[index].as_str())
        .collect::<HashSet<_>>();

    let mut symbols = topology
        .symbols
        .iter()
        .filter(|symbol| selected_paths.contains(symbol.path.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let selected_symbol_ids = symbols
        .iter()
        .map(|symbol| symbol.id.as_str())
        .collect::<HashSet<_>>();
    let symbol_edges = topology
        .symbol_edges
        .iter()
        .filter(|edge| {
            selected_symbol_ids.contains(edge.source.as_str())
                && selected_symbol_ids.contains(edge.target.as_str())
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut symbol_fan_in = HashMap::<&str, usize>::new();
    let mut symbol_fan_out = HashMap::<&str, usize>::new();
    for edge in &symbol_edges {
        *symbol_fan_out.entry(edge.source.as_str()).or_default() += 1;
        *symbol_fan_in.entry(edge.target.as_str()).or_default() += 1;
    }
    for symbol in &mut symbols {
        symbol.fan_in = symbol_fan_in
            .get(symbol.id.as_str())
            .copied()
            .unwrap_or_default();
        symbol.fan_out = symbol_fan_out
            .get(symbol.id.as_str())
            .copied()
            .unwrap_or_default();
    }
    let symbol_reach = symbol_reach_by_path(&symbols, &symbol_edges);

    let files = topology
        .graph_files
        .iter()
        .enumerate()
        .filter(|(index, _)| selected.contains(index))
        .map(|(index, path)| {
            let signal = signals.files.get(path).cloned().unwrap_or_default();
            let dependencies = signal
                .dependencies
                .into_iter()
                .filter(|path| selected_paths.contains(path.as_str()))
                .collect::<Vec<_>>();
            let dependents = signal
                .dependents
                .into_iter()
                .filter(|path| selected_paths.contains(path.as_str()))
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
                focus_distance: (!focus.is_empty()).then_some(distances[index].unwrap_or(0)),
                symbol_reach: symbol_reach.get(path).cloned(),
            }
        })
        .collect::<Vec<_>>();

    let edge_list = topology
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
        .collect::<Vec<_>>();

    let mut top_depended = files
        .iter()
        .filter(|file| file.fan_in > 0)
        .map(|file| GraphNode {
            path: file.path.clone(),
            fan_in: file.fan_in,
            fan_out: file.fan_out,
        })
        .collect::<Vec<_>>();
    top_depended.sort_by(|a, b| b.fan_in.cmp(&a.fan_in).then_with(|| a.path.cmp(&b.path)));
    top_depended.truncate(10);

    let mut most_dependent = files
        .iter()
        .filter(|file| file.fan_out > 0)
        .map(|file| GraphNode {
            path: file.path.clone(),
            fan_in: file.fan_in,
            fan_out: file.fan_out,
        })
        .collect::<Vec<_>>();
    most_dependent.sort_by(|a, b| b.fan_out.cmp(&a.fan_out).then_with(|| a.path.cmp(&b.path)));
    most_dependent.truncate(10);

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
                .all(|path| selected_paths.contains(path.as_str()))
        })
        .collect();
    let orphans = orphans
        .into_iter()
        .filter(|path| selected_paths.contains(path.as_str()))
        .collect();
    let unresolved_imports = selected
        .iter()
        .map(|&index| topology.unresolved_by_node[index])
        .sum();
    let parse_errors = selected
        .iter()
        .map(|&index| topology.parse_errors_by_node[index])
        .sum();
    let unresolved_symbol_relations = if selected_paths.len() == topology.graph_files.len() {
        topology.unresolved_symbol_relations
    } else {
        selected_paths
            .iter()
            .map(|path| {
                topology
                    .unresolved_symbol_relations_by_path
                    .get(*path)
                    .copied()
                    .unwrap_or_default()
            })
            .sum()
    };

    DepGraph {
        languages,
        nodes: files.len(),
        edges: edge_list.len(),
        files,
        edge_list,
        symbols,
        symbol_edges,
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

fn symbol_reach_by_path(
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

fn dominant_relation(relations: Option<&BTreeMap<&str, usize>>) -> String {
    relations
        .and_then(|relations| {
            relations
                .iter()
                .max_by(|left, right| left.1.cmp(right.1).then_with(|| right.0.cmp(left.0)))
        })
        .map(|(relation, _)| (*relation).to_string())
        .unwrap_or_default()
}

fn normalize_graph_focus(path: &Path, root: &Path) -> String {
    let relative = if path.is_absolute() {
        path.strip_prefix(root).unwrap_or(path)
    } else {
        path
    };
    normalize_path(&relative.to_string_lossy().replace('\\', "/"))
        .trim_end_matches('/')
        .to_string()
}

fn graph_focus_matches(path: &str, focus: &str) -> bool {
    focus.is_empty()
        || path == focus
        || path
            .strip_prefix(focus)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn impact_from_topology(topology: &Topology, changed: &HashSet<PathBuf>) -> ImpactAnalysis {
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

// ---------------------------------------------------------------------------
// Import specifier extractors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SourceFacts {
    specifiers: Vec<ImportSpecifier>,
    parse_errors: usize,
    symbols: symbols::SourceFacts,
}

impl SourceFacts {
    pub(crate) fn parse_error() -> Self {
        Self {
            parse_errors: 1,
            ..Self::default()
        }
    }
}

#[derive(Default)]
struct SpecifierExtraction {
    specifiers: Vec<ImportSpecifier>,
    parse_errors: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum ImportSpecifier {
    Module(String),
    PhpNamespace(String),
    PhpInclude(StaticInclude),
    Rust(RustImport),
    GoPackage(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum RustImport {
    Module {
        name: String,
        path: Option<String>,
        inline_modules: Vec<String>,
    },
    Use {
        path: String,
        inline_modules: Vec<String>,
    },
}

#[cfg(test)]
fn extract_specifiers(fc: FirstClass, content: &str) -> SpecifierExtraction {
    let Some(tree) = parse::parse(fc, content) else {
        return SpecifierExtraction {
            parse_errors: 1,
            ..SpecifierExtraction::default()
        };
    };
    extract_specifiers_from_root(fc, content, tree.root_node())
}

pub(crate) fn extract_source_facts(fc: FirstClass, path: &str, content: &str) -> SourceFacts {
    let Some(tree) = parse::parse(fc, content) else {
        return SourceFacts::parse_error();
    };
    extract_source_facts_from_tree(fc, path, content, tree.root_node())
}

pub(crate) fn extract_source_facts_from_tree(
    fc: FirstClass,
    path: &str,
    content: &str,
    root: Node<'_>,
) -> SourceFacts {
    let extraction = extract_specifiers_from_root(fc, content, root);
    SourceFacts {
        specifiers: extraction.specifiers,
        parse_errors: extraction.parse_errors,
        symbols: symbols::Collector::source_facts(fc, path, content, root),
    }
}

fn extract_specifiers_from_root(
    fc: FirstClass,
    content: &str,
    root: Node<'_>,
) -> SpecifierExtraction {
    let mut extraction = SpecifierExtraction {
        parse_errors: count_parse_errors(root),
        ..SpecifierExtraction::default()
    };
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        match fc {
            FirstClass::JavaScript | FirstClass::TypeScript | FirstClass::Tsx => {
                extract_js_node(node, content, &mut extraction.specifiers);
            }
            FirstClass::Python => extract_python_node(node, content, &mut extraction.specifiers),
            FirstClass::Php => extract_php_node(node, content, &mut extraction.specifiers),
            FirstClass::Rust => extract_rust_node(node, content, &mut extraction.specifiers),
            FirstClass::Go => extract_go_node(node, content, &mut extraction.specifiers),
        }
        for index in (0..node.named_child_count()).rev() {
            if let Some(child) = node.named_child(index as u32) {
                stack.push(child);
            }
        }
    }
    extraction
}

#[cfg(test)]
pub(crate) fn extract_js_specifiers(content: &str) -> Vec<String> {
    module_specifiers(extract_specifiers(FirstClass::JavaScript, content))
}

#[cfg(test)]
pub(crate) fn extract_py_specifiers(content: &str) -> Vec<String> {
    module_specifiers(extract_specifiers(FirstClass::Python, content))
}

#[cfg(test)]
fn module_specifiers(extraction: SpecifierExtraction) -> Vec<String> {
    extraction
        .specifiers
        .into_iter()
        .filter_map(|specifier| match specifier {
            ImportSpecifier::Module(value) => Some(value),
            ImportSpecifier::PhpNamespace(_)
            | ImportSpecifier::PhpInclude(_)
            | ImportSpecifier::Rust(_)
            | ImportSpecifier::GoPackage(_) => None,
        })
        .collect()
}

fn count_parse_errors(root: Node<'_>) -> usize {
    let mut errors = 0usize;
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.is_error() || node.is_missing() {
            errors = errors.saturating_add(1);
        }
        for index in (0..node.child_count()).rev() {
            if let Some(child) = node.child(index as u32) {
                stack.push(child);
            }
        }
    }
    errors
}

fn extract_js_node(node: Node<'_>, content: &str, specs: &mut Vec<ImportSpecifier>) {
    match node.kind() {
        "import_statement" | "export_statement" => {
            if let Some(source) = node.child_by_field_name("source")
                && let Some(specifier) = string_literal(source, content)
            {
                specs.push(ImportSpecifier::Module(specifier));
            }
        }
        "call_expression" => {
            let Some(function) = node.child_by_field_name("function") else {
                return;
            };
            let is_supported = match function.kind() {
                "import" => true,
                "identifier" => function
                    .utf8_text(content.as_bytes())
                    .is_ok_and(|name| name == "require"),
                _ => false,
            };
            if !is_supported {
                return;
            }
            if let Some(arguments) = node.child_by_field_name("arguments") {
                let mut cursor = arguments.walk();
                if let Some(specifier) = arguments
                    .named_children(&mut cursor)
                    .next()
                    .and_then(|argument| string_literal(argument, content))
                {
                    specs.push(ImportSpecifier::Module(specifier));
                }
            }
        }
        _ => {}
    }
}

fn extract_python_node(node: Node<'_>, content: &str, specs: &mut Vec<ImportSpecifier>) {
    match node.kind() {
        "import_statement" => {
            let mut cursor = node.walk();
            for imported in node.named_children(&mut cursor) {
                if let Some(module) = python_import_name(imported, content) {
                    specs.push(ImportSpecifier::Module(module));
                }
            }
        }
        "import_from_statement" => {
            let Some(module_node) = node.child_by_field_name("module_name") else {
                return;
            };
            let Ok(module) = module_node.utf8_text(content.as_bytes()) else {
                return;
            };
            if !module.chars().all(|ch| ch == '.') {
                specs.push(ImportSpecifier::Module(module.to_string()));
                return;
            }

            let mut found_name = false;
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if child.id() == module_node.id() {
                    continue;
                }
                if child.kind() == "wildcard_import" {
                    specs.push(ImportSpecifier::Module(module.to_string()));
                    found_name = true;
                } else if let Some(name) = python_import_name(child, content) {
                    specs.push(ImportSpecifier::Module(format!("{module}{name}")));
                    found_name = true;
                }
            }
            if !found_name {
                specs.push(ImportSpecifier::Module(module.to_string()));
            }
        }
        _ => {}
    }
}

fn extract_php_node(node: Node<'_>, content: &str, specs: &mut Vec<ImportSpecifier>) {
    if node.kind() == "namespace_use_declaration" {
        specs.extend(
            php::use_namespaces(node, content)
                .into_iter()
                .map(ImportSpecifier::PhpNamespace),
        );
    } else if let Some(include) = php::static_include(node, content) {
        specs.push(ImportSpecifier::PhpInclude(include));
    }
}

fn extract_rust_node(node: Node<'_>, content: &str, specs: &mut Vec<ImportSpecifier>) {
    match node.kind() {
        "mod_item" if node.child_by_field_name("body").is_none() => {
            let Some(name) = node
                .child_by_field_name("name")
                .and_then(|name| name.utf8_text(content.as_bytes()).ok())
            else {
                return;
            };
            specs.push(ImportSpecifier::Rust(RustImport::Module {
                name: name.to_string(),
                path: rust_path_attribute(node, content),
                inline_modules: enclosing_inline_rust_modules(node, content),
            }));
        }
        "use_declaration" => {
            let Some(argument) = node.child_by_field_name("argument") else {
                return;
            };
            let mut paths = Vec::new();
            expand_rust_use(argument, content, "", &mut paths);
            paths.sort();
            paths.dedup();
            let inline_modules = enclosing_inline_rust_modules(node, content);
            specs.extend(paths.into_iter().map(|path| {
                ImportSpecifier::Rust(RustImport::Use {
                    path,
                    inline_modules: inline_modules.clone(),
                })
            }));
        }
        _ => {}
    }
}

fn expand_rust_use(node: Node<'_>, content: &str, prefix: &str, paths: &mut Vec<String>) {
    match node.kind() {
        "scoped_use_list" => {
            let next_prefix = node
                .child_by_field_name("path")
                .and_then(|path| rust_node_text(path, content))
                .map(|path| join_rust_path(prefix, &path))
                .unwrap_or_else(|| prefix.to_string());
            if let Some(list) = node.child_by_field_name("list") {
                expand_rust_use(list, content, &next_prefix, paths);
            }
        }
        "use_list" => {
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                expand_rust_use(child, content, prefix, paths);
            }
        }
        "use_as_clause" => {
            if let Some(path) = node.child_by_field_name("path") {
                expand_rust_use(path, content, prefix, paths);
            }
        }
        "use_wildcard" => {
            if let Some(path) = node.child_by_field_name("path")
                && let Some(path) = rust_node_text(path, content)
            {
                paths.push(join_rust_path(prefix, &path));
            }
        }
        "identifier" | "scoped_identifier" | "crate" | "self" | "super" => {
            if let Some(path) = rust_node_text(node, content) {
                paths.push(join_rust_path(prefix, &path));
            }
        }
        _ => {}
    }
}

fn rust_node_text(node: Node<'_>, content: &str) -> Option<String> {
    let text = node.utf8_text(content.as_bytes()).ok()?.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn join_rust_path(prefix: &str, suffix: &str) -> String {
    if prefix.is_empty() {
        suffix.to_string()
    } else if suffix == "self" {
        prefix.to_string()
    } else {
        format!("{prefix}::{suffix}")
    }
}

fn enclosing_inline_rust_modules(node: Node<'_>, content: &str) -> Vec<String> {
    let mut modules = Vec::new();
    let mut ancestor = node.parent();
    while let Some(parent) = ancestor {
        if parent.kind() == "mod_item"
            && parent.child_by_field_name("body").is_some()
            && let Some(name) = parent
                .child_by_field_name("name")
                .and_then(|name| name.utf8_text(content.as_bytes()).ok())
        {
            modules.push(name.to_string());
        }
        ancestor = parent.parent();
    }
    modules.reverse();
    modules
}

fn rust_path_attribute(node: Node<'_>, content: &str) -> Option<String> {
    let mut sibling = node.prev_named_sibling();
    while let Some(attribute) = sibling {
        if attribute.kind() != "attribute_item" {
            break;
        }
        let text = attribute.utf8_text(content.as_bytes()).ok()?.trim();
        if text.starts_with("#[path") {
            let value = text.split_once('=')?.1.trim();
            let value = value.strip_suffix(']')?.trim();
            return strip_static_quotes(value).map(str::to_string);
        }
        sibling = attribute.prev_named_sibling();
    }
    None
}

fn extract_go_node(node: Node<'_>, content: &str, specs: &mut Vec<ImportSpecifier>) {
    if node.kind() != "import_spec" {
        return;
    }
    let Some(path) = node.child_by_field_name("path") else {
        return;
    };
    let Some(path) = path
        .utf8_text(content.as_bytes())
        .ok()
        .map(str::trim)
        .and_then(strip_static_quotes)
    else {
        return;
    };
    specs.push(ImportSpecifier::GoPackage(path.to_string()));
}

fn strip_static_quotes(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    if bytes.len() < 2 {
        return None;
    }
    let quote = bytes[0];
    (matches!(quote, b'\'' | b'"' | b'`') && bytes.last().copied() == Some(quote))
        .then(|| &text[1..text.len() - 1])
}

fn python_import_name(node: Node<'_>, content: &str) -> Option<String> {
    let node = if node.kind() == "aliased_import" {
        node.child_by_field_name("name")?
    } else {
        node
    };
    matches!(node.kind(), "dotted_name" | "identifier")
        .then(|| node.utf8_text(content.as_bytes()).ok().map(str::to_string))
        .flatten()
}

fn string_literal(node: Node<'_>, content: &str) -> Option<String> {
    if node.kind() != "string" {
        return None;
    }
    let text = node.utf8_text(content.as_bytes()).ok()?;
    let quote = text.as_bytes().first().copied()?;
    if !matches!(quote, b'\'' | b'"' | b'`') || text.as_bytes().last().copied() != Some(quote) {
        return None;
    }
    Some(text[1..text.len().saturating_sub(1)].to_string())
}

// ---------------------------------------------------------------------------
// Path resolvers
// ---------------------------------------------------------------------------

enum ImportResolution {
    Resolved {
        target: String,
        resolver: &'static str,
    },
    Unresolved,
    External,
}

#[derive(Default)]
struct PythonResolver {
    /// Deterministic import roots inferred from conventional `src/` layouts.
    /// The repository root is represented by the empty string and tried too.
    roots: Vec<String>,
}

impl PythonResolver {
    fn discover(graph_files: &[String]) -> Self {
        let mut roots = BTreeSet::from([String::new()]);
        for path in graph_files {
            let parts = path.split('/').collect::<Vec<_>>();
            for (index, part) in parts.iter().enumerate() {
                if *part == "src" {
                    roots.insert(parts[..=index].join("/"));
                }
            }
        }
        Self {
            roots: roots.into_iter().collect(),
        }
    }

    fn resolve(&self, importer_rel: &str, spec: &str, nodes: &HashSet<String>) -> ImportResolution {
        if spec.starts_with('.') {
            return resolve_py(importer_rel, spec, nodes).map_or(
                ImportResolution::Unresolved,
                |target| ImportResolution::Resolved {
                    target,
                    resolver: "python-relative",
                },
            );
        }

        let module_path = spec.replace('.', "/");
        let mut candidates = Vec::new();
        for root in &self.roots {
            let base = join_graph_path(root, &module_path);
            for target in [format!("{base}.py"), format!("{base}/__init__.py")] {
                if nodes.contains(&target) {
                    candidates.push((root.as_str(), target));
                }
            }
        }
        candidates.sort_by(|(left_root, left_path), (right_root, right_path)| {
            python_root_rank(importer_rel, right_root)
                .cmp(&python_root_rank(importer_rel, left_root))
                .then_with(|| left_path.cmp(right_path))
        });
        candidates.dedup_by(|left, right| left.1 == right.1);
        let Some((root, target)) = candidates.first() else {
            return ImportResolution::External;
        };
        let top_rank = python_root_rank(importer_rel, root);
        if candidates
            .iter()
            .skip(1)
            .any(|(candidate_root, _)| python_root_rank(importer_rel, candidate_root) == top_rank)
        {
            return ImportResolution::Unresolved;
        }
        ImportResolution::Resolved {
            target: target.clone(),
            resolver: if root.is_empty() {
                "python-absolute"
            } else {
                "python-src-root"
            },
        }
    }
}

#[derive(Default)]
struct RustResolver {
    files: HashMap<String, RustFileModule>,
    modules: BTreeMap<(String, String), Vec<String>>,
    crates: BTreeMap<String, RustCrateTarget>,
    ambiguous_crates: HashSet<String>,
    config_files: Vec<String>,
    config_errors: usize,
    config_errors_by_path: BTreeMap<String, usize>,
}

#[derive(Clone)]
struct RustFileModule {
    source_root: String,
    module: Vec<String>,
}

#[derive(Clone)]
struct RustCrateTarget {
    source_root: String,
    root_file: String,
}

#[derive(Clone)]
struct RustPackage {
    config_path: String,
    directory: String,
    names: Vec<String>,
    lib_path: String,
}

impl RustResolver {
    fn discover(graph_files: &[String], access: &mut ConfigAccess<'_>) -> Self {
        if access.root.is_file() {
            return Self::from_files(graph_files, Vec::new());
        }

        let rust_files = graph_files
            .iter()
            .filter(|path| {
                detect(Path::new(path)).and_then(|info| info.first_class) == Some(FirstClass::Rust)
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut candidates = BTreeSet::new();
        for path in &rust_files {
            let mut directory = path_parent(path);
            loop {
                let relative = join_graph_path(&directory, "Cargo.toml");
                if access.exists(&relative) {
                    candidates.insert(relative);
                }
                if directory.is_empty() {
                    break;
                }
                directory = path_parent(&directory);
            }
        }

        let mut packages = Vec::new();
        let mut config_files = Vec::new();
        let mut config_errors = 0usize;
        let mut config_errors_by_path = BTreeMap::new();
        for relative in candidates {
            match read_cargo_package(access, &relative) {
                Some(Ok(package)) => {
                    config_files.push(relative.clone());
                    if let Some(package) = package {
                        packages.push(package);
                    }
                }
                Some(Err(())) => {
                    config_errors = config_errors.saturating_add(1);
                    *config_errors_by_path.entry(relative).or_insert(0) += 1;
                }
                None => {}
            }
        }
        packages.sort_by(|left, right| {
            rust_scope_rank(&right.directory)
                .cmp(&rust_scope_rank(&left.directory))
                .then_with(|| left.directory.cmp(&right.directory))
        });

        let mut resolver = Self::from_files(&rust_files, packages.clone());
        resolver.config_files = config_files;
        resolver.config_files.sort();
        resolver.config_files.dedup();
        resolver.config_errors = resolver.config_errors.saturating_add(config_errors);
        resolver.config_errors_by_path = config_errors_by_path;

        let node_set = rust_files.iter().cloned().collect::<HashSet<_>>();
        for package in packages {
            if !node_set.contains(&package.lib_path) {
                continue;
            }
            let source_root = path_parent(&package.lib_path);
            let target = RustCrateTarget {
                source_root,
                root_file: package.lib_path,
            };
            for name in package.names {
                if resolver.ambiguous_crates.contains(&name) {
                    continue;
                }
                if resolver
                    .crates
                    .insert(name.clone(), target.clone())
                    .is_some()
                {
                    resolver.crates.remove(&name);
                    resolver.ambiguous_crates.insert(name);
                    resolver.config_errors = resolver.config_errors.saturating_add(1);
                    *resolver
                        .config_errors_by_path
                        .entry(package.config_path.clone())
                        .or_insert(0) += 1;
                }
            }
        }
        resolver
    }

    fn from_files(graph_files: &[String], packages: Vec<RustPackage>) -> Self {
        let mut resolver = Self::default();
        for path in graph_files {
            if detect(Path::new(path)).and_then(|info| info.first_class) != Some(FirstClass::Rust) {
                continue;
            }
            let file = rust_file_module(path, &packages);
            resolver
                .modules
                .entry((file.source_root.clone(), file.module.join("::")))
                .or_default()
                .push(path.clone());
            resolver.files.insert(path.clone(), file);
        }
        for paths in resolver.modules.values_mut() {
            paths.sort_by(|left, right| {
                rust_module_file_rank(left)
                    .cmp(&rust_module_file_rank(right))
                    .then_with(|| left.cmp(right))
            });
            paths.dedup();
        }
        resolver
    }

    fn resolve(
        &self,
        importer_rel: &str,
        import: &RustImport,
        nodes: &HashSet<String>,
    ) -> ImportResolution {
        let Some(file) = self.files.get(importer_rel) else {
            return ImportResolution::Unresolved;
        };
        match import {
            RustImport::Module {
                name,
                path,
                inline_modules,
            } => {
                if let Some(path) = path {
                    let target = normalize_path(&join_graph_path(&path_parent(importer_rel), path));
                    return if nodes.contains(&target) {
                        ImportResolution::Resolved {
                            target,
                            resolver: "rust-path",
                        }
                    } else {
                        ImportResolution::Unresolved
                    };
                }
                let mut module = file.module.clone();
                module.extend(inline_modules.iter().cloned());
                module.push(name.clone());
                self.resolve_exact_module(&file.source_root, &module, importer_rel)
                    .map_or(ImportResolution::Unresolved, |target| {
                        ImportResolution::Resolved {
                            target,
                            resolver: "rust-mod",
                        }
                    })
            }
            RustImport::Use {
                path,
                inline_modules,
            } => self.resolve_use(importer_rel, file, path, inline_modules),
        }
    }

    fn resolve_use(
        &self,
        importer_rel: &str,
        file: &RustFileModule,
        path: &str,
        inline_modules: &[String],
    ) -> ImportResolution {
        let mut segments = path
            .trim_start_matches("::")
            .split("::")
            .filter(|segment| !segment.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>();
        if segments.is_empty() {
            return ImportResolution::External;
        }

        let mut source_root = file.source_root.clone();
        let mut module = file.module.clone();
        module.extend(inline_modules.iter().cloned());
        let resolver = match segments.first().map(String::as_str) {
            Some("crate") => {
                segments.remove(0);
                module.clear();
                "rust-use"
            }
            Some("super") => {
                while segments.first().is_some_and(|segment| segment == "super") {
                    segments.remove(0);
                    module.pop();
                }
                "rust-use"
            }
            Some("self") => {
                segments.remove(0);
                "rust-use"
            }
            Some(name) if self.ambiguous_crates.contains(name) => {
                return ImportResolution::Unresolved;
            }
            Some(name) if self.crates.contains_key(name) => {
                let target = self.crates.get(name).expect("checked crate target");
                segments.remove(0);
                source_root.clone_from(&target.source_root);
                module.clear();
                if segments.is_empty() && target.root_file != importer_rel {
                    return ImportResolution::Resolved {
                        target: target.root_file.clone(),
                        resolver: "rust-workspace",
                    };
                }
                "rust-workspace"
            }
            _ => {
                module.clear();
                "rust-use"
            }
        };
        module.extend(segments);

        match self.resolve_module_prefix(&source_root, &module, importer_rel) {
            Some(target) => ImportResolution::Resolved { target, resolver },
            None if resolver == "rust-workspace"
                || path.starts_with("crate::")
                || path.starts_with("self::")
                || path.starts_with("super::") =>
            {
                ImportResolution::Unresolved
            }
            None => ImportResolution::External,
        }
    }

    fn resolve_exact_module(
        &self,
        source_root: &str,
        module: &[String],
        importer_rel: &str,
    ) -> Option<String> {
        self.modules
            .get(&(source_root.to_string(), module.join("::")))?
            .iter()
            .find(|target| target.as_str() != importer_rel)
            .cloned()
    }

    fn resolve_module_prefix(
        &self,
        source_root: &str,
        module: &[String],
        importer_rel: &str,
    ) -> Option<String> {
        for length in (1..=module.len()).rev() {
            let key = (source_root.to_string(), module[..length].join("::"));
            if let Some(target) = self.modules.get(&key).and_then(|targets| {
                targets
                    .iter()
                    .find(|target| target.as_str() != importer_rel)
                    .cloned()
            }) {
                return Some(target);
            }
        }
        None
    }
}

struct ConfigAccess<'a> {
    root: &'a Path,
    budget: &'a mut ReadBudget,
    snapshot: Option<&'a BTreeMap<String, String>>,
}

impl ConfigAccess<'_> {
    fn exists(&self, relative: &str) -> bool {
        if let Some(snapshot) = self.snapshot {
            return snapshot.contains_key(relative);
        }
        fs_budget::is_regular_file(&self.root.join(relative))
    }

    fn read(&mut self, relative: &str) -> Option<String> {
        if let Some(snapshot) = self.snapshot {
            return snapshot.get(relative).cloned();
        }
        match fs_budget::read_text(&self.root.join(relative), self.budget) {
            ReadOutcome::Content(content) => Some(content),
            _ => None,
        }
    }
}

fn repo_is_regular_file(root: &Path, relative: &str) -> bool {
    fs_budget::is_regular_file(&root.join(relative))
}

fn read_repo_text(root: &Path, relative: &str, budget: &mut ReadBudget) -> Option<String> {
    match fs_budget::read_text(&root.join(relative), budget) {
        ReadOutcome::Content(content) => Some(content),
        _ => None,
    }
}

fn candidate_resolver_config_paths(root: &Path, graph_files: &[String]) -> BTreeSet<String> {
    let mut candidates = BTreeSet::new();
    if root.is_file() {
        return candidates;
    }
    for path in graph_files {
        let fc = detect(Path::new(path)).and_then(|info| info.first_class);
        let mut directory = path_parent(path);
        loop {
            match fc {
                Some(FirstClass::JavaScript | FirstClass::TypeScript | FirstClass::Tsx) => {
                    for name in ["tsconfig.json", "jsconfig.json", "package.json"] {
                        candidates.insert(join_graph_path(&directory, name));
                    }
                }
                Some(FirstClass::Rust) => {
                    candidates.insert(join_graph_path(&directory, "Cargo.toml"));
                }
                Some(FirstClass::Go) => {
                    candidates.insert(join_graph_path(&directory, "go.mod"));
                }
                Some(FirstClass::Php) => {
                    candidates.insert(join_graph_path(&directory, "composer.json"));
                }
                _ => {}
            }
            if directory.is_empty() {
                break;
            }
            directory = path_parent(&directory);
        }
    }
    candidates
        .into_iter()
        .filter(|relative| repo_is_regular_file(root, relative))
        .collect()
}

fn read_cargo_package(
    access: &mut ConfigAccess<'_>,
    relative: &str,
) -> Option<Result<Option<RustPackage>, ()>> {
    let content = access.read(relative)?;
    let value = match toml::from_str::<toml::Value>(&content) {
        Ok(value) => value,
        Err(_) => return Some(Err(())),
    };
    let Some(package_name) = value
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(toml::Value::as_str)
    else {
        return Some(Ok(None));
    };
    let directory = path_parent(relative);
    let lib = value.get("lib");
    let lib_name = lib
        .and_then(|lib| lib.get("name"))
        .and_then(toml::Value::as_str)
        .unwrap_or(package_name);
    let lib_path = lib
        .and_then(|lib| lib.get("path"))
        .and_then(toml::Value::as_str)
        .unwrap_or("src/lib.rs");
    let mut names = vec![rust_crate_name(package_name), rust_crate_name(lib_name)];
    names.sort();
    names.dedup();
    Some(Ok(Some(RustPackage {
        config_path: relative.to_string(),
        directory: directory.clone(),
        names,
        lib_path: join_graph_path(&directory, lib_path),
    })))
}

fn rust_crate_name(name: &str) -> String {
    name.replace('-', "_")
}

fn rust_file_module(path: &str, packages: &[RustPackage]) -> RustFileModule {
    let package = packages
        .iter()
        .find(|package| path_in_scope(path, &package.directory));
    let package_directory = package
        .map(|package| package.directory.as_str())
        .unwrap_or("");
    let relative = strip_graph_prefix(path, package_directory).unwrap_or(path);
    let parts = relative.split('/').collect::<Vec<_>>();

    let (source_relative, treat_top_level_as_root) = if parts.first() == Some(&"src") {
        ("src".to_string(), false)
    } else if matches!(parts.first(), Some(&"tests" | &"examples" | &"benches")) {
        (parts[0].to_string(), true)
    } else if let Some(index) = parts.iter().position(|part| *part == "src") {
        (parts[..=index].join("/"), false)
    } else {
        (String::new(), false)
    };
    let source_root = join_graph_path(package_directory, &source_relative);
    let relative_to_root = strip_graph_prefix(path, &source_root).unwrap_or(path);
    let mut module_parts = relative_to_root
        .split('/')
        .map(str::to_string)
        .collect::<Vec<_>>();
    let file_name = module_parts.pop().unwrap_or_default();
    let stem = Path::new(&file_name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    if stem == "mod" {
        // The directory already names this module.
    } else if (matches!(stem, "lib" | "main") || treat_top_level_as_root) && module_parts.is_empty()
    {
        module_parts.clear();
    } else if !stem.is_empty() {
        module_parts.push(stem.to_string());
    }
    RustFileModule {
        source_root,
        module: module_parts,
    }
}

fn rust_scope_rank(directory: &str) -> usize {
    directory.split('/').filter(|part| !part.is_empty()).count()
}

fn rust_module_file_rank(path: &str) -> usize {
    match Path::new(path).file_name().and_then(|name| name.to_str()) {
        Some("lib.rs") => 0,
        Some("main.rs") => 1,
        Some("mod.rs") => 2,
        _ => 3,
    }
}

#[derive(Default)]
struct GoResolver {
    modules: Vec<GoModule>,
    packages: BTreeMap<String, String>,
    config_files: Vec<String>,
    config_errors: usize,
    config_errors_by_path: BTreeMap<String, usize>,
}

struct GoModule {
    prefix: String,
    directory: String,
}

impl GoResolver {
    fn discover(graph_files: &[String], access: &mut ConfigAccess<'_>) -> Self {
        let go_files = graph_files
            .iter()
            .filter(|path| {
                detect(Path::new(path)).and_then(|info| info.first_class) == Some(FirstClass::Go)
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut package_files = BTreeMap::<String, Vec<String>>::new();
        for path in &go_files {
            package_files
                .entry(path_parent(path))
                .or_default()
                .push(path.clone());
        }
        let packages = package_files
            .into_iter()
            .map(|(directory, mut paths)| {
                paths.sort_by(|left, right| {
                    go_representative_rank(left, &directory)
                        .cmp(&go_representative_rank(right, &directory))
                        .then_with(|| left.cmp(right))
                });
                (directory, paths.remove(0))
            })
            .collect();
        if access.root.is_file() {
            return Self {
                packages,
                ..Self::default()
            };
        }

        let mut candidates = BTreeSet::new();
        for path in &go_files {
            let mut directory = path_parent(path);
            loop {
                let relative = join_graph_path(&directory, "go.mod");
                if access.exists(&relative) {
                    candidates.insert(relative);
                }
                if directory.is_empty() {
                    break;
                }
                directory = path_parent(&directory);
            }
        }

        let mut resolver = Self {
            packages,
            ..Self::default()
        };
        let mut prefixes = HashSet::new();
        for relative in candidates {
            match read_go_module(access, &relative) {
                Some(Ok(prefix)) => {
                    resolver.config_files.push(relative.clone());
                    if prefixes.insert(prefix.clone()) {
                        resolver.modules.push(GoModule {
                            prefix,
                            directory: path_parent(&relative),
                        });
                    } else {
                        resolver.config_errors = resolver.config_errors.saturating_add(1);
                        *resolver
                            .config_errors_by_path
                            .entry(relative.clone())
                            .or_insert(0) += 1;
                    }
                }
                Some(Err(())) => {
                    resolver.config_errors = resolver.config_errors.saturating_add(1);
                    *resolver.config_errors_by_path.entry(relative).or_insert(0) += 1;
                }
                None => {}
            }
        }
        resolver.modules.sort_by(|left, right| {
            right
                .prefix
                .len()
                .cmp(&left.prefix.len())
                .then_with(|| left.prefix.cmp(&right.prefix))
        });
        resolver.config_files.sort();
        resolver.config_files.dedup();
        resolver
    }

    fn resolve(&self, importer_rel: &str, package: &str) -> ImportResolution {
        if package.starts_with("./") || package.starts_with("../") {
            let directory = normalize_path(&join_graph_path(&path_parent(importer_rel), package));
            return self
                .packages
                .get(&directory)
                .map_or(ImportResolution::Unresolved, |target| {
                    ImportResolution::Resolved {
                        target: target.clone(),
                        resolver: "go-relative",
                    }
                });
        }
        let Some(module) = self.modules.iter().find(|module| {
            package == module.prefix
                || package
                    .strip_prefix(&module.prefix)
                    .is_some_and(|suffix| suffix.starts_with('/'))
        }) else {
            return ImportResolution::External;
        };
        let suffix = package
            .strip_prefix(&module.prefix)
            .unwrap_or_default()
            .trim_start_matches('/');
        let directory = join_graph_path(&module.directory, suffix);
        self.packages
            .get(&directory)
            .map_or(ImportResolution::Unresolved, |target| {
                ImportResolution::Resolved {
                    target: target.clone(),
                    resolver: "go-module",
                }
            })
    }
}

fn read_go_module(access: &mut ConfigAccess<'_>, relative: &str) -> Option<Result<String, ()>> {
    let content = access.read(relative)?;
    let module = content.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        (parts.next() == Some("module"))
            .then(|| parts.next())
            .flatten()
    });
    Some(module.map(str::to_string).ok_or(()))
}

fn go_representative_rank(path: &str, directory: &str) -> (bool, bool, bool) {
    let name = Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let directory_name = Path::new(directory)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    (
        name.ends_with("_test.go"),
        name != format!("{directory_name}.go"),
        name != "doc.go",
    )
}

fn path_in_scope(path: &str, directory: &str) -> bool {
    directory.is_empty()
        || path
            .strip_prefix(directory)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn strip_graph_prefix<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    if prefix.is_empty() {
        Some(path)
    } else {
        path.strip_prefix(prefix)?.strip_prefix('/')
    }
}

#[derive(Default)]
struct PhpResolver {
    mappings: Vec<PhpMapping>,
    config_files: Vec<String>,
    config_errors: usize,
    config_errors_by_path: BTreeMap<String, usize>,
}

struct PhpMapping {
    prefix: String,
    directories: Vec<String>,
    config_directory: String,
    kind: PhpMappingKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PhpMappingKind {
    Psr4,
    Psr0,
}

impl PhpResolver {
    fn discover(graph_files: &[String], access: &mut ConfigAccess<'_>) -> Self {
        if access.root.is_file() {
            return Self::default();
        }

        let mut candidates = BTreeSet::new();
        for path in graph_files {
            if detect(Path::new(path)).and_then(|info| info.first_class) != Some(FirstClass::Php) {
                continue;
            }
            let mut directory = path_parent(path);
            loop {
                let relative = join_graph_path(&directory, "composer.json");
                if access.exists(&relative) {
                    candidates.insert(relative);
                }
                if directory.is_empty() {
                    break;
                }
                directory = path_parent(&directory);
            }
        }

        let mut resolver = Self::default();
        for relative in candidates {
            match read_composer_config(access, &relative) {
                Some(Ok(mappings)) if !mappings.is_empty() => {
                    resolver.config_files.push(relative);
                    resolver.mappings.extend(mappings);
                }
                Some(Ok(_)) => {}
                Some(Err(())) => {
                    resolver.config_errors += 1;
                    *resolver.config_errors_by_path.entry(relative).or_insert(0) += 1;
                }
                None => {}
            }
        }
        resolver.mappings.sort_by(|left, right| {
            php_scope_rank(&right.config_directory)
                .cmp(&php_scope_rank(&left.config_directory))
                .then_with(|| right.prefix.len().cmp(&left.prefix.len()))
                .then_with(|| php_mapping_rank(left.kind).cmp(&php_mapping_rank(right.kind)))
                .then_with(|| left.prefix.cmp(&right.prefix))
        });
        resolver.config_files.sort();
        resolver.config_files.dedup();
        resolver
    }

    fn resolve_namespace(
        &self,
        importer_rel: &str,
        symbol: &str,
        nodes: &HashSet<String>,
    ) -> ImportResolution {
        let symbol = symbol.trim().trim_start_matches('\\');
        let best_scope = self
            .mappings
            .iter()
            .filter(|mapping| {
                php_importer_in_scope(importer_rel, &mapping.config_directory)
                    && symbol.starts_with(&mapping.prefix)
            })
            .map(|mapping| php_scope_rank(&mapping.config_directory))
            .max();
        let best_prefix = best_scope.and_then(|scope| {
            self.mappings
                .iter()
                .filter(|mapping| {
                    php_scope_rank(&mapping.config_directory) == scope
                        && php_importer_in_scope(importer_rel, &mapping.config_directory)
                        && symbol.starts_with(&mapping.prefix)
                })
                .map(|mapping| mapping.prefix.len())
                .max()
        });

        if let (Some(scope), Some(prefix_len)) = (best_scope, best_prefix) {
            for mapping in self.mappings.iter().filter(|mapping| {
                php_scope_rank(&mapping.config_directory) == scope
                    && mapping.prefix.len() == prefix_len
                    && php_importer_in_scope(importer_rel, &mapping.config_directory)
                    && symbol.starts_with(&mapping.prefix)
            }) {
                let class_path = match mapping.kind {
                    PhpMappingKind::Psr4 => symbol
                        .strip_prefix(&mapping.prefix)
                        .unwrap_or(symbol)
                        .replace('\\', "/"),
                    PhpMappingKind::Psr0 => symbol.replace(['\\', '_'], "/"),
                };
                for directory in &mapping.directories {
                    let base = join_graph_path(directory, &class_path);
                    if let Some(target) = try_resolve_php(&base, nodes) {
                        return ImportResolution::Resolved {
                            target,
                            resolver: match mapping.kind {
                                PhpMappingKind::Psr4 => "composer-psr-4",
                                PhpMappingKind::Psr0 => "composer-psr-0",
                            },
                        };
                    }
                }
            }
            return ImportResolution::Unresolved;
        }

        let parts = symbol.split('\\').collect::<Vec<_>>();
        let without_vendor = (parts.len() > 1).then(|| parts[1..].join("/"));
        let full = parts.join("/");
        let mut candidates = Vec::new();
        for directory in ["src", "app", "lib"] {
            for class_path in std::iter::once(full.as_str()).chain(without_vendor.as_deref()) {
                if let Some(target) =
                    try_resolve_php(&join_graph_path(directory, class_path), nodes)
                {
                    candidates.push(target);
                }
            }
        }
        candidates.sort();
        candidates.dedup();
        match candidates.as_slice() {
            [target] => ImportResolution::Resolved {
                target: target.clone(),
                resolver: "php-namespace-heuristic",
            },
            [] => ImportResolution::External,
            _ => ImportResolution::Unresolved,
        }
    }

    fn resolve_include(
        &self,
        importer_rel: &str,
        include: &StaticInclude,
        nodes: &HashSet<String>,
    ) -> ImportResolution {
        let include_path = match include {
            StaticInclude::Literal(path) | StaticInclude::DirectoryRelative { path, .. } => path,
        };
        if is_composer_autoloader(include_path) {
            return ImportResolution::External;
        }
        let mut bases = Vec::new();
        match include {
            StaticInclude::Literal(path) => {
                if Path::new(path).is_absolute() {
                    return ImportResolution::External;
                }
                bases.push(join_graph_path(&path_parent(importer_rel), path));
                bases.push(normalize_path(path));
            }
            StaticInclude::DirectoryRelative { parents, path } => {
                let directory = go_up(&path_parent(importer_rel), *parents);
                bases.push(join_graph_path(&directory, path));
            }
        }
        let mut candidates = bases
            .into_iter()
            .filter_map(|base| try_resolve_php(&base, nodes))
            .collect::<Vec<_>>();
        candidates.sort();
        candidates.dedup();
        match candidates.as_slice() {
            [target] => ImportResolution::Resolved {
                target: target.clone(),
                resolver: "php-include",
            },
            _ => ImportResolution::Unresolved,
        }
    }
}

fn is_composer_autoloader(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    let normalized = normalized.trim_start_matches(['.', '/']);
    normalized == "vendor/autoload.php" || normalized.ends_with("/vendor/autoload.php")
}

fn read_composer_config(
    access: &mut ConfigAccess<'_>,
    relative: &str,
) -> Option<Result<Vec<PhpMapping>, ()>> {
    let content = access.read(relative)?;
    let value: Value = match serde_json::from_str(&content) {
        Ok(value) => value,
        Err(_) => return Some(Err(())),
    };
    let config_directory = path_parent(relative);
    let mut mappings = Vec::new();
    for section in ["autoload", "autoload-dev"] {
        let Some(autoload) = value.get(section).and_then(Value::as_object) else {
            continue;
        };
        for (field, kind) in [
            ("psr-4", PhpMappingKind::Psr4),
            ("psr-0", PhpMappingKind::Psr0),
        ] {
            let Some(prefixes) = autoload.get(field).and_then(Value::as_object) else {
                continue;
            };
            for (prefix, directories) in prefixes {
                let directories = composer_paths(directories)
                    .into_iter()
                    .map(|directory| join_graph_path(&config_directory, &directory))
                    .collect::<Vec<_>>();
                if !directories.is_empty() {
                    mappings.push(PhpMapping {
                        prefix: prefix.trim_start_matches('\\').to_string(),
                        directories,
                        config_directory: config_directory.clone(),
                        kind,
                    });
                }
            }
        }
    }
    Some(Ok(mappings))
}

fn composer_paths(value: &Value) -> Vec<String> {
    match value {
        Value::String(path) => vec![path.clone()],
        Value::Array(paths) => paths
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

fn php_importer_in_scope(importer_rel: &str, directory: &str) -> bool {
    directory.is_empty()
        || importer_rel
            .strip_prefix(directory)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn php_scope_rank(directory: &str) -> usize {
    directory.split('/').filter(|part| !part.is_empty()).count()
}

fn php_mapping_rank(kind: PhpMappingKind) -> usize {
    match kind {
        PhpMappingKind::Psr4 => 0,
        PhpMappingKind::Psr0 => 1,
    }
}

fn combined_config_files<'a>(groups: impl IntoIterator<Item = &'a [String]>) -> Vec<String> {
    let mut files = groups
        .into_iter()
        .flat_map(|files| files.iter().cloned())
        .collect::<Vec<_>>();
    files.sort();
    files.dedup();
    files
}

fn combined_config_errors<'a>(
    groups: impl IntoIterator<Item = &'a BTreeMap<String, usize>>,
) -> BTreeMap<String, usize> {
    let mut combined = BTreeMap::new();
    for group in groups {
        for (path, count) in group {
            let entry = combined.entry(path.clone()).or_insert(0usize);
            *entry = entry.saturating_add(*count);
        }
    }
    combined
}

fn python_root_rank(importer_rel: &str, root: &str) -> usize {
    if root.is_empty() {
        return 1;
    }
    let importer_directory = path_parent(importer_rel);
    let shared_prefix = importer_directory
        .split('/')
        .zip(root.split('/'))
        .take_while(|(importer, candidate)| importer == candidate)
        .count();
    let depth = root.split('/').count();
    if importer_rel == root
        || importer_rel
            .strip_prefix(root)
            .is_some_and(|suffix| suffix.starts_with('/'))
    {
        return depth.saturating_add(10_000);
    }
    if shared_prefix == 0 {
        2
    } else {
        shared_prefix.saturating_mul(100).saturating_add(depth)
    }
}

#[derive(Default)]
struct JsResolver {
    configs: BTreeMap<String, TsConfig>,
    packages: BTreeMap<String, PackageConfig>,
    packages_by_directory: BTreeMap<String, PackageConfig>,
    ambiguous_packages: HashSet<String>,
    config_files: Vec<String>,
    config_errors: usize,
    config_errors_by_path: BTreeMap<String, usize>,
}

struct TsConfig {
    directory: String,
    base_url: Option<String>,
    paths: Vec<PathMapping>,
}

struct PathMapping {
    pattern: String,
    targets: Vec<String>,
    base: String,
}

struct ParsedTsConfig {
    config: TsConfig,
    related: Vec<String>,
}

#[derive(Clone)]
struct PackageConfig {
    directory: String,
    name: Option<String>,
    has_exports: bool,
    has_imports: bool,
    exports: Vec<PackageMapping>,
    imports: Vec<PackageMapping>,
    entrypoints: Vec<String>,
}

#[derive(Clone)]
struct PackageMapping {
    pattern: String,
    targets: Vec<String>,
}

impl JsResolver {
    fn discover(graph_files: &[String], access: &mut ConfigAccess<'_>) -> Self {
        if access.root.is_file() {
            return Self::default();
        }
        let mut candidates = BTreeSet::new();
        let mut package_candidates = BTreeSet::new();
        for path in graph_files {
            if !matches!(
                detect(Path::new(path)).and_then(|info| info.first_class),
                Some(FirstClass::JavaScript | FirstClass::TypeScript | FirstClass::Tsx)
            ) {
                continue;
            }
            let mut directory = path_parent(path);
            loop {
                for name in ["tsconfig.json", "jsconfig.json"] {
                    let relative = if directory.is_empty() {
                        name.to_string()
                    } else {
                        format!("{directory}/{name}")
                    };
                    if access.exists(&relative) {
                        candidates.insert(relative);
                    }
                }
                let package = if directory.is_empty() {
                    "package.json".to_string()
                } else {
                    format!("{directory}/package.json")
                };
                if access.exists(&package) {
                    package_candidates.insert(package);
                }
                if directory.is_empty() {
                    break;
                }
                directory = path_parent(&directory);
            }
        }

        let mut resolver = Self::default();
        let mut seen = BTreeSet::new();
        while let Some(relative) = candidates.pop_first() {
            if !seen.insert(relative.clone()) {
                continue;
            }
            match read_ts_config(access, &relative) {
                Some(Ok(parsed)) => {
                    candidates.extend(parsed.related);
                    let config = parsed.config;
                    if config.base_url.is_some() || !config.paths.is_empty() {
                        resolver.config_files.push(relative);
                    }
                    resolver
                        .configs
                        .entry(config.directory.clone())
                        .and_modify(|existing| {
                            if config.base_url.is_some() {
                                existing.base_url.clone_from(&config.base_url);
                            }
                            existing
                                .paths
                                .extend(config.paths.iter().map(|mapping| PathMapping {
                                    pattern: mapping.pattern.clone(),
                                    targets: mapping.targets.clone(),
                                    base: mapping.base.clone(),
                                }));
                            existing.paths.sort_by(|a, b| a.pattern.cmp(&b.pattern));
                            existing
                                .paths
                                .dedup_by(|a, b| a.pattern == b.pattern && a.base == b.base);
                        })
                        .or_insert(config);
                }
                Some(Err(())) => {
                    resolver.config_errors += 1;
                    *resolver
                        .config_errors_by_path
                        .entry(relative.clone())
                        .or_insert(0) += 1;
                }
                None => {}
            }
        }
        for relative in package_candidates {
            match read_package_config(access, &relative) {
                Some(Ok(package)) => {
                    if package.name.is_some() || package.has_exports || package.has_imports {
                        resolver.config_files.push(relative.clone());
                    }
                    resolver
                        .packages_by_directory
                        .insert(package.directory.clone(), package.clone());
                    if let Some(name) = package.name.clone() {
                        if resolver.ambiguous_packages.contains(&name) {
                            continue;
                        }
                        if resolver.packages.insert(name.clone(), package).is_some() {
                            resolver.packages.remove(&name);
                            resolver.ambiguous_packages.insert(name);
                            resolver.config_errors += 1;
                            *resolver
                                .config_errors_by_path
                                .entry(relative.clone())
                                .or_insert(0) += 1;
                        }
                    }
                }
                Some(Err(())) => {
                    resolver.config_errors += 1;
                    *resolver.config_errors_by_path.entry(relative).or_insert(0) += 1;
                }
                None => {}
            }
        }
        resolver.config_files.sort();
        resolver.config_files.dedup();
        resolver
    }

    fn resolve(&self, importer_rel: &str, spec: &str, nodes: &HashSet<String>) -> ImportResolution {
        if spec.starts_with("./") || spec.starts_with("../") {
            let parent = path_parent(importer_rel);
            let joined = if parent.is_empty() {
                spec.to_string()
            } else {
                format!("{parent}/{spec}")
            };
            return try_resolve_js(&normalize_path(&joined), nodes).map_or(
                ImportResolution::Unresolved,
                |target| ImportResolution::Resolved {
                    target,
                    resolver: "relative",
                },
            );
        }

        let mut directory = path_parent(importer_rel);
        loop {
            if let Some(config) = self.configs.get(&directory) {
                let mut matched_mapping = false;
                for mapping in &config.paths {
                    let Some(capture) = match_path_pattern(&mapping.pattern, spec) else {
                        continue;
                    };
                    matched_mapping = true;
                    for target in &mapping.targets {
                        let target = apply_path_capture(target, capture.as_deref());
                        let joined = join_graph_path(&mapping.base, &target);
                        if let Some(target) = try_resolve_js(&joined, nodes) {
                            return ImportResolution::Resolved {
                                target,
                                resolver: "tsconfig-paths",
                            };
                        }
                    }
                }
                if matched_mapping {
                    return ImportResolution::Unresolved;
                }
                if let Some(base_url) = &config.base_url
                    && let Some(target) = try_resolve_js(&join_graph_path(base_url, spec), nodes)
                {
                    return ImportResolution::Resolved {
                        target,
                        resolver: "tsconfig-base-url",
                    };
                }
            }
            if directory.is_empty() {
                break;
            }
            directory = path_parent(&directory);
        }

        if spec.starts_with('#') {
            let mut directory = path_parent(importer_rel);
            loop {
                if let Some(package) = self.packages_by_directory.get(&directory) {
                    return resolve_package_mappings(
                        package,
                        &package.imports,
                        spec,
                        nodes,
                        "package-imports",
                    )
                    .unwrap_or(ImportResolution::Unresolved);
                }
                if directory.is_empty() {
                    break;
                }
                directory = path_parent(&directory);
            }
            return ImportResolution::Unresolved;
        }

        if let Some((package_name, package_subpath)) = split_package_specifier(spec) {
            if self.ambiguous_packages.contains(package_name) {
                return ImportResolution::Unresolved;
            }
            if let Some(package) = self.packages.get(package_name) {
                let requested = package_subpath
                    .map(|subpath| format!("./{subpath}"))
                    .unwrap_or_else(|| ".".to_string());
                if package.has_exports {
                    return resolve_package_mappings(
                        package,
                        &package.exports,
                        &requested,
                        nodes,
                        "package-exports",
                    )
                    .unwrap_or(ImportResolution::Unresolved);
                }
                if let Some(subpath) = package_subpath {
                    return try_resolve_js(&join_graph_path(&package.directory, subpath), nodes)
                        .map_or(ImportResolution::Unresolved, |target| {
                            ImportResolution::Resolved {
                                target,
                                resolver: "package-subpath",
                            }
                        });
                }
                for entrypoint in &package.entrypoints {
                    if let Some(target) =
                        try_resolve_js(&join_graph_path(&package.directory, entrypoint), nodes)
                    {
                        return ImportResolution::Resolved {
                            target,
                            resolver: "package-entrypoint",
                        };
                    }
                }
                for entrypoint in ["src/index", "index"] {
                    if let Some(target) =
                        try_resolve_js(&join_graph_path(&package.directory, entrypoint), nodes)
                    {
                        return ImportResolution::Resolved {
                            target,
                            resolver: "package-index",
                        };
                    }
                }
                return ImportResolution::Unresolved;
            }
        }

        if let Some(stripped) = spec.strip_prefix("@/") {
            for base_root in ["src", "app", ""] {
                let joined = join_graph_path(base_root, stripped);
                if let Some(target) = try_resolve_js(&joined, nodes) {
                    return ImportResolution::Resolved {
                        target,
                        resolver: "heuristic-alias",
                    };
                }
            }
            return ImportResolution::Unresolved;
        }

        ImportResolution::External
    }
}

fn split_package_specifier(spec: &str) -> Option<(&str, Option<&str>)> {
    if spec.starts_with('.') || spec.starts_with('/') || spec.starts_with('#') {
        return None;
    }
    if spec.starts_with('@') {
        let mut separators = spec.match_indices('/');
        separators.next()?;
        let (second, _) = separators.next().unwrap_or((spec.len(), ""));
        let name = &spec[..second];
        let subpath = spec.get(second + usize::from(second < spec.len())..);
        return Some((name, subpath.filter(|value| !value.is_empty())));
    }
    let (name, subpath) = spec
        .split_once('/')
        .map_or((spec, None), |(name, subpath)| (name, Some(subpath)));
    (!name.is_empty()).then_some((name, subpath.filter(|value| !value.is_empty())))
}

fn resolve_package_mappings(
    package: &PackageConfig,
    mappings: &[PackageMapping],
    requested: &str,
    nodes: &HashSet<String>,
    resolver: &'static str,
) -> Option<ImportResolution> {
    for mapping in mappings {
        let Some(capture) = match_path_pattern(&mapping.pattern, requested) else {
            continue;
        };
        for target in &mapping.targets {
            if !target.starts_with("./") {
                return Some(ImportResolution::External);
            }
            let target = apply_path_capture(target, capture.as_deref());
            let joined = join_graph_path(&package.directory, &target);
            if let Some(target) = try_resolve_js(&joined, nodes) {
                return Some(ImportResolution::Resolved { target, resolver });
            }
        }
        // Package maps choose one most-specific key. An explicit `null`, an
        // unsupported condition object, or a missing target under that key
        // blocks broader wildcard mappings instead of falling through.
        return Some(ImportResolution::Unresolved);
    }
    None
}

fn read_ts_config(
    access: &mut ConfigAccess<'_>,
    relative: &str,
) -> Option<Result<ParsedTsConfig, ()>> {
    let content = access.read(relative)?;
    let value: Value = match serde_json::from_str(&sanitize_jsonc(&content)) {
        Ok(value) => value,
        Err(_) => return Some(Err(())),
    };
    let compiler = value
        .get("compilerOptions")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let directory = path_parent(relative);
    let mut related = value
        .get("references")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|reference| reference.get("path").and_then(Value::as_str))
        .filter_map(|reference| resolve_config_reference(access, &directory, reference))
        .collect::<Vec<_>>();
    if let Some(extended) = value.get("extends").and_then(Value::as_str)
        && let Some(extended) = resolve_config_reference(access, &directory, extended)
    {
        related.push(extended);
    }
    related.sort();
    related.dedup();
    let base_url = compiler
        .get("baseUrl")
        .and_then(Value::as_str)
        .map(|base| join_graph_path(&directory, base));
    let mapping_base = base_url.clone().unwrap_or_else(|| directory.clone());
    let mut paths = compiler
        .get("paths")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(pattern, targets)| {
            let targets = targets
                .as_array()?
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>();
            (!targets.is_empty()).then(|| PathMapping {
                pattern: pattern.clone(),
                targets,
                base: mapping_base.clone(),
            })
        })
        .collect::<Vec<_>>();
    paths.sort_by(|a, b| a.pattern.cmp(&b.pattern));
    Some(Ok(ParsedTsConfig {
        config: TsConfig {
            directory,
            base_url,
            paths,
        },
        related,
    }))
}

fn read_package_config(
    access: &mut ConfigAccess<'_>,
    relative: &str,
) -> Option<Result<PackageConfig, ()>> {
    let content = access.read(relative)?;
    let value: Value = match serde_json::from_str(&content) {
        Ok(value) => value,
        Err(_) => return Some(Err(())),
    };
    let directory = path_parent(relative);
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string);
    let has_exports = value.get("exports").is_some();
    let exports = value
        .get("exports")
        .map(package_exports)
        .unwrap_or_default();
    let has_imports = value.get("imports").is_some();
    let imports = value
        .get("imports")
        .and_then(Value::as_object)
        .map(|imports| {
            imports
                .iter()
                .filter(|(pattern, _)| pattern.starts_with('#'))
                .map(|(pattern, value)| PackageMapping {
                    pattern: pattern.clone(),
                    targets: package_targets(value),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let entrypoints = ["source", "module", "main", "types", "typings"]
        .into_iter()
        .filter_map(|field| value.get(field).and_then(Value::as_str))
        .map(str::to_string)
        .collect::<Vec<_>>();
    Some(Ok(PackageConfig {
        directory,
        name,
        has_exports,
        has_imports,
        exports: sorted_package_mappings(exports),
        imports: sorted_package_mappings(imports),
        entrypoints,
    }))
}

fn package_exports(value: &Value) -> Vec<PackageMapping> {
    if let Some(exports) = value.as_object()
        && exports.keys().any(|key| key.starts_with('.'))
    {
        return exports
            .iter()
            .filter(|(pattern, _)| pattern.starts_with('.'))
            .map(|(pattern, value)| PackageMapping {
                pattern: pattern.clone(),
                targets: package_targets(value),
            })
            .collect();
    }
    let targets = package_targets(value);
    vec![PackageMapping {
        pattern: ".".to_string(),
        targets,
    }]
}

fn package_targets(value: &Value) -> Vec<String> {
    match value {
        Value::String(target) => vec![target.clone()],
        Value::Array(values) => values.iter().flat_map(package_targets).collect(),
        Value::Object(conditions) => {
            let mut targets = Vec::new();
            for condition in ["source", "import", "default", "node", "require", "types"] {
                if let Some(value) = conditions.get(condition) {
                    targets.extend(package_targets(value));
                }
            }
            targets
        }
        _ => Vec::new(),
    }
}

fn sorted_package_mappings(mut mappings: Vec<PackageMapping>) -> Vec<PackageMapping> {
    for mapping in &mut mappings {
        let mut seen = HashSet::new();
        mapping.targets.retain(|target| seen.insert(target.clone()));
    }
    mappings.sort_by(|left, right| {
        left.pattern
            .contains('*')
            .cmp(&right.pattern.contains('*'))
            .then_with(|| right.pattern.len().cmp(&left.pattern.len()))
            .then_with(|| left.pattern.cmp(&right.pattern))
    });
    mappings
}

fn resolve_config_reference(
    access: &ConfigAccess<'_>,
    directory: &str,
    reference: &str,
) -> Option<String> {
    if !reference.starts_with('.') {
        return None;
    }
    let mut candidate = join_graph_path(directory, reference);
    if access.snapshot.is_some() {
        // Snapshot mode: only preloaded paths exist. Prefer exact key, then .json.
        if access.exists(&candidate) {
            return Some(candidate);
        }
        if Path::new(&candidate).extension().is_none() {
            candidate.push_str(".json");
            if access.exists(&candidate) {
                return Some(candidate);
            }
            let dir_ts = join_graph_path(candidate.trim_end_matches(".json"), "tsconfig.json");
            if access.exists(&dir_ts) {
                return Some(dir_ts);
            }
        }
        return None;
    }
    let absolute = access.root.join(&candidate);
    let metadata = std::fs::symlink_metadata(&absolute).ok();
    if metadata
        .as_ref()
        .is_some_and(|meta| meta.is_dir() && !meta.file_type().is_symlink())
    {
        candidate = join_graph_path(&candidate, "tsconfig.json");
    } else if !metadata
        .as_ref()
        .is_some_and(|meta| meta.is_file() && !meta.file_type().is_symlink())
        && Path::new(&candidate).extension().is_none()
    {
        candidate.push_str(".json");
    }
    access.exists(&candidate).then_some(candidate)
}

fn sanitize_jsonc(content: &str) -> String {
    let bytes = content.as_bytes();
    let mut stripped = Vec::with_capacity(bytes.len());
    let mut index = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            stripped.push(byte);
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if byte == b'"' {
            in_string = true;
            stripped.push(byte);
            index += 1;
        } else if byte == b'/' && bytes.get(index + 1) == Some(&b'/') {
            stripped.extend_from_slice(b"  ");
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                stripped.push(b' ');
                index += 1;
            }
        } else if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
            stripped.extend_from_slice(b"  ");
            index += 2;
            while index < bytes.len() {
                if bytes[index] == b'*' && bytes.get(index + 1) == Some(&b'/') {
                    stripped.extend_from_slice(b"  ");
                    index += 2;
                    break;
                }
                stripped.push(if bytes[index] == b'\n' { b'\n' } else { b' ' });
                index += 1;
            }
        } else {
            stripped.push(byte);
            index += 1;
        }
    }

    let mut sanitized = Vec::with_capacity(stripped.len());
    let mut in_string = false;
    let mut escaped = false;
    for (index, &byte) in stripped.iter().enumerate() {
        if in_string {
            sanitized.push(byte);
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        if byte == b'"' {
            in_string = true;
            sanitized.push(byte);
            continue;
        }
        if byte == b',' {
            let next = stripped[index + 1..]
                .iter()
                .copied()
                .find(|candidate| !candidate.is_ascii_whitespace());
            if matches!(next, Some(b'}' | b']')) {
                sanitized.push(b' ');
                continue;
            }
        }
        sanitized.push(byte);
    }
    String::from_utf8(sanitized).unwrap_or_default()
}

fn match_path_pattern(pattern: &str, spec: &str) -> Option<Option<String>> {
    let Some(star) = pattern.find('*') else {
        return (pattern == spec).then_some(None);
    };
    let prefix = &pattern[..star];
    let suffix = &pattern[star + 1..];
    (spec.starts_with(prefix)
        && spec.ends_with(suffix)
        && spec.len() >= prefix.len() + suffix.len())
    .then(|| Some(spec[prefix.len()..spec.len() - suffix.len()].to_string()))
}

fn apply_path_capture(target: &str, capture: Option<&str>) -> String {
    match capture {
        Some(capture) => target.replacen('*', capture, 1),
        None => target.to_string(),
    }
}

fn join_graph_path(base: &str, path: &str) -> String {
    if base.is_empty() || base == "." {
        normalize_path(path)
    } else {
        normalize_path(&format!("{base}/{path}"))
    }
}

/// Resolve a JS/TS import specifier to a node path.
///
/// Only handles local specs: `./`, `../`, or `@/` prefixes. All others return
/// `None` (treated as external / npm packages).
#[cfg(test)]
pub(crate) fn resolve_js(
    importer_rel: &str,
    spec: &str,
    nodes: &HashSet<String>,
) -> Option<String> {
    match JsResolver::default().resolve(importer_rel, spec, nodes) {
        ImportResolution::Resolved { target, .. } => Some(target),
        ImportResolution::Unresolved | ImportResolution::External => None,
    }
}

/// Resolve a Python import specifier to a node path.
///
/// Only handles relative imports (specs starting with `.`). All others return
/// `None`.
pub(crate) fn resolve_py(
    importer_rel: &str,
    spec: &str,
    nodes: &HashSet<String>,
) -> Option<String> {
    if !spec.starts_with('.') {
        return None;
    }

    let level = spec.chars().take_while(|&c| c == '.').count();
    let remainder_str = &spec[level..];
    let remainder: Vec<&str> = remainder_str.split('.').filter(|s| !s.is_empty()).collect();

    let parent = path_parent(importer_rel);
    let start_dir = go_up(&parent, level.saturating_sub(1));

    if remainder.is_empty() {
        // `from . import x` — look for __init__.py in the package dir.
        let candidate = if start_dir.is_empty() {
            "__init__.py".to_string()
        } else {
            format!("{start_dir}/__init__.py")
        };
        return nodes.contains(candidate.as_str()).then_some(candidate);
    }

    let base = if start_dir.is_empty() {
        remainder.join("/")
    } else {
        format!("{}/{}", start_dir, remainder.join("/"))
    };

    let c1 = format!("{base}.py");
    if nodes.contains(c1.as_str()) {
        return Some(c1);
    }
    let c2 = format!("{base}/__init__.py");
    if nodes.contains(c2.as_str()) {
        return Some(c2);
    }

    None
}

// ---------------------------------------------------------------------------
// Entrypoint heuristic
// ---------------------------------------------------------------------------

/// Returns `true` if `rel` looks like a well-known entrypoint or config file.
///
/// Filenames checked (case-insensitive stem): `index`, `main`, `app`;
/// exact names: `__init__.py`, `__main__.py`, `setup.py`, `conftest.py`,
/// `bootstrap.php`, `artisan`;
/// suffix patterns: `.config.{js,ts,mjs,cjs}`, `.d.ts`.
pub(crate) fn is_entrypoint(rel: &str) -> bool {
    let filename = rel.rsplit('/').next().unwrap_or(rel);
    let lower = filename.to_ascii_lowercase();

    if matches!(
        lower.as_str(),
        "__init__.py"
            | "__main__.py"
            | "setup.py"
            | "conftest.py"
            | "bootstrap.php"
            | "artisan"
            | "lib.rs"
            | "build.rs"
    ) {
        return true;
    }

    if lower.ends_with(".d.ts") {
        return true;
    }

    for suffix in &[".config.js", ".config.ts", ".config.mjs", ".config.cjs"] {
        if lower.ends_with(suffix) {
            return true;
        }
    }

    // index.*, main.*, app.* — any extension.
    if let Some(dot) = lower.find('.')
        && matches!(&lower[..dot], "index" | "main" | "app")
    {
        return true;
    }

    false
}

// ---------------------------------------------------------------------------
// Strongly-connected components (Kosaraju's algorithm, iterative)
// ---------------------------------------------------------------------------

/// Compute strongly-connected components of the directed graph.
///
/// `nodes` is the ordered node list (used only for its length).
/// `edges` is a list of `(from, to)` index pairs.
/// Returns one `Vec<usize>` per component (indices into `nodes`).
pub(crate) fn strongly_connected(nodes: &[String], edges: &[(usize, usize)]) -> Vec<Vec<usize>> {
    let n = nodes.len();
    if n == 0 {
        return Vec::new();
    }

    let mut adj = vec![vec![]; n];
    let mut radj = vec![vec![]; n];
    for &(u, v) in edges {
        if u < n && v < n {
            adj[u].push(v);
            radj[v].push(u);
        }
    }

    // Phase 1: DFS on the original graph to collect finish order.
    let mut visited = vec![false; n];
    let mut finish_order: Vec<usize> = Vec::with_capacity(n);

    for start in 0..n {
        if visited[start] {
            continue;
        }
        // Iterative DFS: stack stores (node, next_adj_index).
        let mut stack: Vec<(usize, usize)> = vec![(start, 0)];
        visited[start] = true;
        loop {
            match stack.last_mut() {
                None => break,
                Some((node, idx)) => {
                    let node = *node;
                    if *idx < adj[node].len() {
                        let next = adj[node][*idx];
                        *idx += 1;
                        if !visited[next] {
                            visited[next] = true;
                            stack.push((next, 0));
                        }
                    } else {
                        finish_order.push(node);
                        stack.pop();
                    }
                }
            }
        }
    }

    // Phase 2: DFS on the transposed graph in reverse finish order.
    let mut comp_id = vec![usize::MAX; n];
    let mut components: Vec<Vec<usize>> = Vec::new();

    for &start in finish_order.iter().rev() {
        if comp_id[start] != usize::MAX {
            continue;
        }
        let c = components.len();
        components.push(Vec::new());
        let mut stack = vec![start];
        comp_id[start] = c;
        while let Some(node) = stack.pop() {
            components[c].push(node);
            for &next in &radj[node] {
                if comp_id[next] == usize::MAX {
                    comp_id[next] = c;
                    stack.push(next);
                }
            }
        }
    }

    components
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Return the directory portion of a relative path (everything before the last `/`).
fn path_parent(rel: &str) -> String {
    match rel.rfind('/') {
        Some(pos) => rel[..pos].to_string(),
        None => String::new(),
    }
}

/// Walk `levels` directories up from `dir` (string-based, no filesystem I/O).
fn go_up(dir: &str, levels: usize) -> String {
    if levels == 0 {
        return dir.to_string();
    }
    let parts: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() <= levels {
        return String::new();
    }
    parts[..parts.len() - levels].join("/")
}

/// Normalise a `/`-joined path by resolving `.` and `..` segments.
///
/// Does not touch the real filesystem. A leading `./` is stripped.
fn normalize_path(p: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for seg in p.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

/// Resolve a normalised base path against the node set, trying bare path,
/// bare + extension, and bare + `/index` + extension.
fn try_resolve_js(base: &str, nodes: &HashSet<String>) -> Option<String> {
    const EXTS: &[&str] = &[".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs"];

    if nodes.contains(base) {
        return Some(base.to_string());
    }
    for (runtime, substitutions) in [
        (".js", &[".ts", ".tsx", ".d.ts", ".js", ".jsx"][..]),
        (".jsx", &[".tsx", ".d.ts", ".jsx"][..]),
        (".mjs", &[".mts", ".d.mts", ".mjs"][..]),
        (".cjs", &[".cts", ".d.cts", ".cjs"][..]),
    ] {
        if let Some(stem) = base.strip_suffix(runtime) {
            for substitution in substitutions {
                let candidate = format!("{stem}{substitution}");
                if nodes.contains(&candidate) {
                    return Some(candidate);
                }
            }
            return None;
        }
    }
    for &ext in EXTS {
        let c = format!("{base}{ext}");
        if nodes.contains(c.as_str()) {
            return Some(c);
        }
    }
    for &ext in EXTS {
        let c = format!("{base}/index{ext}");
        if nodes.contains(c.as_str()) {
            return Some(c);
        }
    }
    None
}

fn try_resolve_php(base: &str, nodes: &HashSet<String>) -> Option<String> {
    let base = normalize_path(base);
    if nodes.contains(&base) {
        return Some(base);
    }
    if Path::new(&base).extension().is_none() {
        for candidate in [format!("{base}.php"), format!("{base}/index.php")] {
            if nodes.contains(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::FileReport;
    use std::collections::BTreeMap;
    use tempfile::tempdir;

    fn file_report(path: &str) -> FileReport {
        FileReport {
            path: PathBuf::from(path),
            language: "Python".to_string(),
            bytes: 0,
            tokens: 0,
            loc: 0,
            sloc: 0,
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

    // -- extract_js_specifiers -----------------------------------------------

    #[test]
    fn js_specifiers_basic() {
        let src = "import x from './a';\nconst y = require(\"../b/c\");\nimport('@/lib/d');\nimport 'side-effect';";
        let specs = extract_js_specifiers(src);
        assert!(specs.contains(&"./a".to_string()), "missing ./a: {specs:?}");
        assert!(
            specs.contains(&"../b/c".to_string()),
            "missing ../b/c: {specs:?}"
        );
        assert!(
            specs.contains(&"@/lib/d".to_string()),
            "missing @/lib/d: {specs:?}"
        );
        assert!(
            specs.contains(&"side-effect".to_string()),
            "missing side-effect: {specs:?}"
        );
    }

    #[test]
    fn js_specifiers_no_false_ident() {
        // `from` inside an identifier must not be extracted.
        let src = "const transformFrom = ''; const x = platformFrom;";
        let specs = extract_js_specifiers(src);
        // Neither of those variable names should produce a specifier.
        assert!(specs.is_empty(), "unexpected specifiers: {specs:?}");
    }

    #[test]
    fn js_specifiers_are_unicode_safe_and_ast_scoped() {
        let src = r#"
const café = "require('./in-string')";
// import './in-comment';
client.require('./member');
const local = require ( './local' );
export { value } from "./reexport";
"#;
        assert_eq!(
            extract_js_specifiers(src),
            vec!["./local".to_string(), "./reexport".to_string()]
        );
    }

    // -- extract_py_specifiers -----------------------------------------------

    #[test]
    fn py_specifiers_basic() {
        let src = "from .foo import x\nimport os\nfrom ..pkg.mod import y";
        let specs = extract_py_specifiers(src);
        assert!(
            specs.contains(&".foo".to_string()),
            "missing .foo: {specs:?}"
        );
        assert!(specs.contains(&"os".to_string()), "missing os: {specs:?}");
        assert!(
            specs.contains(&"..pkg.mod".to_string()),
            "missing ..pkg.mod: {specs:?}"
        );
    }

    #[test]
    fn py_current_package_specifiers_handle_star_and_parenthesized_names() {
        let specs =
            extract_py_specifiers("from . import *\nfrom . import (alpha, beta as renamed)\n");

        assert_eq!(specs, vec![".", ".alpha", ".beta"]);
    }

    #[test]
    fn py_current_package_specifiers_handle_multiline_parenthesized_names() {
        let specs = extract_py_specifiers("from . import (\n    alpha,\n    beta as renamed,\n)\n");

        assert_eq!(specs, vec![".alpha", ".beta"]);
    }

    #[test]
    fn py_specifiers_ignore_comments_and_strings() {
        let source = r#"
example = "from .fake import value"
# from .comment import value
from . import (
    alpha,
    beta as renamed,
)
"#;
        assert_eq!(extract_py_specifiers(source), vec![".alpha", ".beta"]);
    }

    #[test]
    fn php_specifiers_are_ast_scoped_and_keep_static_include_kinds() {
        let source = r#"<?php
use App\Service\{UserService, AuditService as Audit};
use function App\Support\helper;
$example = "use Fake\\Ignored;";
// require __DIR__ . '/ignored.php';
require_once __DIR__ . '/../bootstrap.php';
include 'config/routes.php';
include $dynamic;
"#;
        let extraction = extract_specifiers(FirstClass::Php, source);

        assert_eq!(extraction.parse_errors, 0);
        assert_eq!(
            extraction.specifiers,
            vec![
                ImportSpecifier::PhpNamespace("App\\Service\\UserService".into()),
                ImportSpecifier::PhpNamespace("App\\Service\\AuditService".into()),
                ImportSpecifier::PhpNamespace("App\\Support\\helper".into()),
                ImportSpecifier::PhpInclude(StaticInclude::DirectoryRelative {
                    parents: 0,
                    path: "/../bootstrap.php".into(),
                }),
                ImportSpecifier::PhpInclude(StaticInclude::Literal("config/routes.php".into())),
            ]
        );
    }

    #[test]
    fn rust_specifiers_keep_module_context_and_expand_grouped_uses() {
        let source = r#"
mod api;
#[path = "support/custom.rs"]
mod custom;
use crate::{domain::Service, util};
mod inline {
    mod child;
    use super::api::Client;
}
"#;
        let extraction = extract_specifiers(FirstClass::Rust, source);

        assert_eq!(extraction.parse_errors, 0);
        assert_eq!(
            extraction.specifiers,
            vec![
                ImportSpecifier::Rust(RustImport::Module {
                    name: "api".into(),
                    path: None,
                    inline_modules: vec![],
                }),
                ImportSpecifier::Rust(RustImport::Module {
                    name: "custom".into(),
                    path: Some("support/custom.rs".into()),
                    inline_modules: vec![],
                }),
                ImportSpecifier::Rust(RustImport::Use {
                    path: "crate::domain::Service".into(),
                    inline_modules: vec![],
                }),
                ImportSpecifier::Rust(RustImport::Use {
                    path: "crate::util".into(),
                    inline_modules: vec![],
                }),
                ImportSpecifier::Rust(RustImport::Module {
                    name: "child".into(),
                    path: None,
                    inline_modules: vec!["inline".into()],
                }),
                ImportSpecifier::Rust(RustImport::Use {
                    path: "super::api::Client".into(),
                    inline_modules: vec!["inline".into()],
                }),
            ]
        );
    }

    #[test]
    fn go_specifiers_are_ast_scoped() {
        let source = r#"
package main
import (
    "example.com/project/internal/store"
    alias "example.com/project/pkg/api"
)
var ignored = "example.com/project/not-an-import"
"#;
        let extraction = extract_specifiers(FirstClass::Go, source);

        assert_eq!(extraction.parse_errors, 0);
        assert_eq!(
            extraction.specifiers,
            vec![
                ImportSpecifier::GoPackage("example.com/project/internal/store".into()),
                ImportSpecifier::GoPackage("example.com/project/pkg/api".into()),
            ]
        );
    }

    #[test]
    fn malformed_graph_source_records_parse_errors() {
        let extraction = extract_specifiers(FirstClass::JavaScript, "import { from './x';");
        assert!(extraction.parse_errors > 0);
    }

    #[test]
    fn mixed_graph_retains_every_first_class_language() {
        let dir = tempdir().unwrap();
        for (path, source) in [
            ("sample.rs", "pub fn value() {}\n"),
            ("sample.py", "VALUE = 1\n"),
            ("sample.js", "export const value = 1;\n"),
            ("sample.ts", "export const value: number = 1;\n"),
            ("sample.tsx", "export const value = <div />;\n"),
            ("sample.go", "package sample\nconst Value = 1\n"),
            ("sample.php", "<?php\nconst VALUE = 1;\n"),
        ] {
            std::fs::write(dir.path().join(path), source).unwrap();
        }
        let graph = build(
            &[
                file_report("sample.rs"),
                file_report("sample.py"),
                file_report("sample.js"),
                file_report("sample.ts"),
                file_report("sample.tsx"),
                file_report("sample.go"),
                file_report("sample.php"),
            ],
            dir.path(),
        );

        assert_eq!(graph.nodes, 7);
        assert_eq!(
            graph.languages,
            [
                "Go",
                "JavaScript",
                "PHP",
                "Python",
                "Rust",
                "TSX",
                "TypeScript",
            ]
        );
    }

    #[test]
    fn graph_projects_explicit_type_relationships_separately_from_imports() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src/Http")).unwrap();
        std::fs::write(
            dir.path().join("src/Http/HttpClient.php"),
            "<?php namespace App\\Http; abstract class HttpClient {}\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("src/Http/Guzzle.php"),
            "<?php namespace App\\Http; class Guzzle extends HttpClient {}\n",
        )
        .unwrap();

        let graph = build(
            &[
                file_report("src/Http/HttpClient.php"),
                file_report("src/Http/Guzzle.php"),
            ],
            dir.path(),
        );

        assert_eq!(graph.edges, 0, "type edges must not masquerade as imports");
        assert_eq!(graph.symbol_edges.len(), 1);
        assert_eq!(graph.symbol_edges[0].relation, "extends");
        assert_eq!(graph.unresolved_symbol_relations, 0);
        let base = graph
            .files
            .iter()
            .find(|file| file.path.ends_with("HttpClient.php"))
            .unwrap();
        assert_eq!(base.symbol_reach.as_ref().unwrap().name, "HttpClient");
        assert_eq!(base.symbol_reach.as_ref().unwrap().fan_in, 1);
    }

    #[test]
    fn collect_resolver_configs_follows_tsconfig_extends_and_references() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("configs")).unwrap();
        std::fs::create_dir_all(dir.path().join("packages/shared")).unwrap();
        std::fs::write(
            dir.path().join("tsconfig.json"),
            r#"{
  "extends": "./configs/base.json",
  "references": [{ "path": "./packages/shared" }],
  "compilerOptions": { "baseUrl": "." }
}
"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("configs/base.json"),
            r#"{ "compilerOptions": { "strict": true }, "extends": "./strict.json" }"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("configs/strict.json"),
            r#"{ "compilerOptions": { "noImplicitAny": true } }"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("packages/shared/tsconfig.json"),
            r#"{ "compilerOptions": { "composite": true } }"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("index.ts"), "export const value = 1;\n").unwrap();

        let configs = collect_resolver_configs(
            dir.path(),
            &[PathBuf::from("index.ts")],
            &GraphReadLimits::default(),
        );
        assert!(configs.contains_key("tsconfig.json"));
        assert!(configs.contains_key("configs/base.json"));
        assert!(configs.contains_key("configs/strict.json"));
        assert!(configs.contains_key("packages/shared/tsconfig.json"));
    }

    #[test]
    fn oversized_package_json_is_not_loaded_by_the_resolver() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("index.ts"), "export const value = 1;\n").unwrap();
        // Larger than the graph budget; must not be fully loaded.
        std::fs::write(dir.path().join("package.json"), vec![b'x'; 256 * 1024]).unwrap();
        let files = [file_report("index.ts")];
        let limits = GraphReadLimits {
            max_file_bytes: 1024,
            max_total_bytes: 2048,
            max_files: 10,
            facts_only_sources: false,
            deadline: None,
        };
        let report = build_with_limits(&files, dir.path(), limits, None);
        assert_eq!(report.nodes, 1);
        assert!(
            report
                .config_files
                .iter()
                .all(|path| path != "package.json"),
            "oversized package.json must not contribute resolver config"
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_resolver_config_is_rejected() {
        let dir = tempdir().unwrap();
        let external = dir.path().join("outside-package.json");
        std::fs::write(&external, r#"{"name":"evil"}"#).unwrap();
        std::os::unix::fs::symlink(&external, dir.path().join("package.json")).unwrap();
        std::fs::write(dir.path().join("index.ts"), "export const value = 1;\n").unwrap();
        let files = [file_report("index.ts")];
        let report = build_with_limits(&files, dir.path(), GraphReadLimits::default(), None);
        assert!(
            report
                .config_files
                .iter()
                .all(|path| path != "package.json"),
            "symlink package.json must not be followed"
        );
    }

    #[test]
    fn cached_source_facts_produce_the_same_graph_without_rereading_sources() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        let sources = [
            ("src/base.ts", "export interface Service {}\n"),
            (
                "src/app.ts",
                "import { Service } from './base';\nexport class App implements Service {}\n",
            ),
        ];
        for (path, source) in sources {
            std::fs::write(dir.path().join(path), source).unwrap();
        }
        let files = [file_report("src/base.ts"), file_report("src/app.ts")];
        let expected = analyze_with_query(&files, dir.path(), &[], GraphDirection::Both, 1).report;
        let facts = sources
            .into_iter()
            .map(|(path, source)| {
                (
                    PathBuf::from(path),
                    extract_source_facts(FirstClass::TypeScript, path, source),
                )
            })
            .collect::<BTreeMap<_, _>>();
        for (path, _) in sources {
            std::fs::remove_file(dir.path().join(path)).unwrap();
        }

        let actual = analyze_with_query_facts(
            &files,
            dir.path(),
            &facts,
            None,
            GraphReadLimits::default(),
            &[],
            GraphDirection::Both,
            1,
        )
        .report;

        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    }

    #[test]
    fn parse_errors_reduce_impact_confidence() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("changed.js"), "import { from './dep';\n").unwrap();
        std::fs::write(dir.path().join("dep.js"), "export const value = 1;\n").unwrap();
        let paths = vec![PathBuf::from("changed.js"), PathBuf::from("dep.js")];
        let changed = HashSet::from([PathBuf::from("changed.js")]);

        let impact = impact(&paths, dir.path(), &changed);

        assert!(impact.parse_errors > 0);
        assert_eq!(impact.confidence, "partial");
    }

    #[test]
    fn py_from_current_package_import_resolves_sibling_module() {
        let dir = tempdir().unwrap();
        std::fs::create_dir(dir.path().join("pkg")).unwrap();
        std::fs::write(
            dir.path().join("pkg/consumer.py"),
            "from . import sibling\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("pkg/sibling.py"), "VALUE = 1\n").unwrap();

        let graph = build(
            &[
                file_report("pkg/consumer.py"),
                file_report("pkg/sibling.py"),
            ],
            dir.path(),
        );

        assert_eq!(graph.edges, 1);
        assert_eq!(graph.unresolved_imports, 0);
        assert_eq!(graph.most_dependent[0].path, "pkg/consumer.py");
        assert_eq!(graph.most_dependent[0].fan_out, 1);
        assert_eq!(graph.top_depended[0].path, "pkg/sibling.py");
    }

    #[test]
    fn py_from_current_package_import_resolves_comma_separated_aliased_siblings() {
        let dir = tempdir().unwrap();
        std::fs::create_dir(dir.path().join("pkg")).unwrap();
        std::fs::write(
            dir.path().join("pkg/consumer.py"),
            "from . import alpha, beta as renamed\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("pkg/alpha.py"), "VALUE = 1\n").unwrap();
        std::fs::write(dir.path().join("pkg/beta.py"), "VALUE = 2\n").unwrap();

        let graph = build(
            &[
                file_report("pkg/consumer.py"),
                file_report("pkg/alpha.py"),
                file_report("pkg/beta.py"),
            ],
            dir.path(),
        );

        assert_eq!(graph.edges, 2);
        assert_eq!(graph.unresolved_imports, 0);
        assert_eq!(graph.most_dependent[0].path, "pkg/consumer.py");
        assert_eq!(graph.most_dependent[0].fan_out, 2);
        assert_eq!(
            graph
                .top_depended
                .iter()
                .map(|node| node.path.as_str())
                .collect::<Vec<_>>(),
            vec!["pkg/alpha.py", "pkg/beta.py"]
        );
    }

    #[test]
    fn py_from_named_package_import_keeps_dependency_on_package() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("pkg/subpkg")).unwrap();
        std::fs::write(
            dir.path().join("pkg/consumer.py"),
            "from .subpkg import name\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("pkg/subpkg/__init__.py"), "\n").unwrap();
        std::fs::write(dir.path().join("pkg/subpkg/name.py"), "VALUE = 1\n").unwrap();

        let graph = build(
            &[
                file_report("pkg/consumer.py"),
                file_report("pkg/subpkg/__init__.py"),
                file_report("pkg/subpkg/name.py"),
            ],
            dir.path(),
        );

        assert_eq!(graph.edges, 1);
        assert_eq!(graph.unresolved_imports, 0);
        assert_eq!(graph.top_depended.len(), 1);
        assert_eq!(graph.top_depended[0].path, "pkg/subpkg/__init__.py");
        assert_eq!(graph.top_depended[0].fan_in, 1);
    }

    #[test]
    fn php_composer_and_static_includes_build_edges_with_provenance() {
        let dir = tempdir().unwrap();
        for directory in ["src/Http", "src/Service", "src/Support", "tests"] {
            std::fs::create_dir_all(dir.path().join(directory)).unwrap();
        }
        std::fs::write(
            dir.path().join("composer.json"),
            r#"{
                "autoload": { "psr-4": { "App\\": "src/" } },
                "autoload-dev": { "psr-4": { "Tests\\": ["tests/"] } }
            }"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("src/Http/Controller.php"),
            "<?php\nuse App\\Service\\UserService;\nrequire_once __DIR__ . '/../Support/helpers.php';\nrequire_once __DIR__ . '/../../vendor/autoload.php';\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("src/Service/UserService.php"),
            "<?php\nnamespace App\\Service;\nclass UserService {}\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("src/Support/helpers.php"),
            "<?php\nfunction helper(): void {}\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("tests/ControllerTest.php"),
            "<?php\nuse App\\Http\\Controller;\n",
        )
        .unwrap();

        let graph = build(
            &[
                file_report("src/Http/Controller.php"),
                file_report("src/Service/UserService.php"),
                file_report("src/Support/helpers.php"),
                file_report("tests/ControllerTest.php"),
            ],
            dir.path(),
        );

        assert_eq!(graph.nodes, 4);
        assert_eq!(graph.edges, 3);
        assert_eq!(graph.unresolved_imports, 0);
        assert_eq!(graph.config_errors, 0);
        assert_eq!(graph.config_files, ["composer.json"]);
        let provenance = graph
            .edge_list
            .iter()
            .map(|edge| {
                (
                    (edge.source.as_str(), edge.target.as_str()),
                    edge.resolver.as_str(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(
            provenance.get(&("src/Http/Controller.php", "src/Service/UserService.php")),
            Some(&"composer-psr-4")
        );
        assert_eq!(
            provenance.get(&("src/Http/Controller.php", "src/Support/helpers.php")),
            Some(&"php-include")
        );
        assert_eq!(
            provenance.get(&("tests/ControllerTest.php", "src/Http/Controller.php")),
            Some(&"composer-psr-4")
        );

        let paths = [
            "src/Http/Controller.php",
            "src/Service/UserService.php",
            "src/Support/helpers.php",
            "tests/ControllerTest.php",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
        let impact = impact(
            &paths,
            dir.path(),
            &HashSet::from([PathBuf::from("src/Service/UserService.php")]),
        );
        assert_eq!(impact.direct_dependents, ["src/Http/Controller.php"]);
        assert_eq!(impact.transitive_dependents, ["tests/ControllerTest.php"]);
        assert_eq!(impact.confidence, "high");
    }

    #[test]
    fn rust_modules_uses_and_local_cargo_crates_build_edges_with_provenance() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src/service")).unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"demo-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("src/lib.rs"),
            "pub mod service;\nuse crate::service::Worker;\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("src/main.rs"),
            "use demo_app::service::Worker;\nfn main() {}\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("src/service.rs"),
            "pub mod nested;\npub struct Worker;\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("src/service/nested.rs"),
            "pub struct Nested;\n",
        )
        .unwrap();

        let graph = build(
            &[
                file_report("src/lib.rs"),
                file_report("src/main.rs"),
                file_report("src/service.rs"),
                file_report("src/service/nested.rs"),
            ],
            dir.path(),
        );

        assert_eq!(graph.languages, ["Rust"]);
        assert_eq!(graph.nodes, 4);
        assert_eq!(graph.edges, 3);
        assert_eq!(graph.unresolved_imports, 0);
        assert_eq!(graph.config_errors, 0);
        assert_eq!(graph.config_files, ["Cargo.toml"]);
        let edges = graph
            .edge_list
            .iter()
            .map(|edge| {
                (
                    edge.source.as_str(),
                    edge.target.as_str(),
                    edge.resolver.as_str(),
                )
            })
            .collect::<Vec<_>>();
        assert!(edges.contains(&("src/lib.rs", "src/service.rs", "rust-mod")));
        assert!(edges.contains(&("src/main.rs", "src/service.rs", "rust-workspace")));
        assert!(edges.contains(&("src/service.rs", "src/service/nested.rs", "rust-mod")));
    }

    #[test]
    fn go_module_imports_resolve_to_a_stable_package_representative() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("cmd/app")).unwrap();
        std::fs::create_dir_all(dir.path().join("internal/store")).unwrap();
        std::fs::write(
            dir.path().join("go.mod"),
            "module example.com/demo\n\ngo 1.24\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("cmd/app/main.go"),
            "package main\nimport (\"fmt\"; \"example.com/demo/internal/store\")\nfunc main() { fmt.Println(store.Value) }\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("internal/store/store.go"),
            "package store\nconst Value = 1\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("internal/store/helpers.go"),
            "package store\nfunc helper() {}\n",
        )
        .unwrap();

        let graph = build(
            &[
                file_report("cmd/app/main.go"),
                file_report("internal/store/helpers.go"),
                file_report("internal/store/store.go"),
            ],
            dir.path(),
        );

        assert_eq!(graph.languages, ["Go"]);
        assert_eq!(graph.nodes, 3);
        assert_eq!(graph.edges, 1);
        assert_eq!(graph.unresolved_imports, 0);
        assert_eq!(graph.config_errors, 0);
        assert_eq!(graph.config_files, ["go.mod"]);
        assert!(graph.orphans.is_empty());
        assert_eq!(
            (
                graph.edge_list[0].source.as_str(),
                graph.edge_list[0].target.as_str(),
                graph.edge_list[0].resolver.as_str(),
            ),
            ("cmd/app/main.go", "internal/store/store.go", "go-module")
        );
    }

    #[test]
    fn php_psr_zero_and_invalid_composer_configs_are_accounted_for() {
        let valid = tempdir().unwrap();
        std::fs::create_dir_all(valid.path().join("legacy/Legacy/Service")).unwrap();
        std::fs::write(
            valid.path().join("composer.json"),
            r#"{"autoload":{"psr-0":{"Legacy_":"legacy/"}}}"#,
        )
        .unwrap();
        std::fs::write(
            valid.path().join("consumer.php"),
            "<?php\nuse Legacy_Service_User;\n",
        )
        .unwrap();
        std::fs::write(
            valid.path().join("legacy/Legacy/Service/User.php"),
            "<?php\nclass Legacy_Service_User {}\n",
        )
        .unwrap();
        let graph = build(
            &[
                file_report("consumer.php"),
                file_report("legacy/Legacy/Service/User.php"),
            ],
            valid.path(),
        );
        assert_eq!(graph.edges, 1);
        assert_eq!(graph.edge_list[0].resolver, "composer-psr-0");

        let invalid = tempdir().unwrap();
        std::fs::write(invalid.path().join("composer.json"), "{ invalid").unwrap();
        std::fs::write(invalid.path().join("index.php"), "<?php\n").unwrap();
        let graph = build(&[file_report("index.php")], invalid.path());
        assert_eq!(graph.config_errors, 1);
        assert_eq!(graph.config_files, Vec::<String>::new());
    }

    // -- resolve_js ----------------------------------------------------------

    #[test]
    fn resolve_js_relative() {
        let nodes: HashSet<String> = ["src/a.ts", "src/b/index.ts", "src/c.js"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        assert_eq!(
            resolve_js("src/x.ts", "./a", &nodes),
            Some("src/a.ts".to_string())
        );
        assert_eq!(
            resolve_js("src/x.ts", "./b", &nodes),
            Some("src/b/index.ts".to_string())
        );
        assert_eq!(resolve_js("src/x.ts", "react", &nodes), None);
        assert_eq!(
            resolve_js("src/sub/x.ts", "../c", &nodes),
            Some("src/c.js".to_string())
        );
    }

    #[test]
    fn resolve_js_alias() {
        let nodes: HashSet<String> = ["src/lib/d.ts"].iter().map(|s| s.to_string()).collect();
        assert_eq!(
            resolve_js("app/x.ts", "@/lib/d", &nodes),
            Some("src/lib/d.ts".to_string())
        );
    }

    #[test]
    fn tsconfig_paths_resolve_jsonc_aliases_and_expose_edge_provenance() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("app")).unwrap();
        std::fs::create_dir_all(dir.path().join("src/core")).unwrap();
        std::fs::write(
            dir.path().join("tsconfig.json"),
            r#"{
                "files": [],
                "references": [{ "path": "./tsconfig.app.json" }]
            }"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("tsconfig.app.json"),
            r##"{
                // Repo-local aliases used by agents and builds.
                "compilerOptions": {
                    "baseUrl": ".",
                    "paths": {
                        "@core/*": ["src/core/*"],
                        "#exact": ["src/exact.ts"],
                    },
                },
            }"##,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("app/main.ts"),
            "import { util } from '@core/util';\nimport '#exact';\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("src/core/util.ts"),
            "export const util = 1;\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("src/exact.ts"), "export const exact = 1;\n").unwrap();

        let graph = build(
            &[
                file_report("app/main.ts"),
                file_report("src/core/util.ts"),
                file_report("src/exact.ts"),
            ],
            dir.path(),
        );

        assert_eq!(graph.edges, 2);
        assert_eq!(graph.unresolved_imports, 0);
        assert_eq!(graph.config_errors, 0);
        assert_eq!(graph.config_files, ["tsconfig.app.json"]);
        assert_eq!(
            graph
                .edge_list
                .iter()
                .map(|edge| (
                    edge.source.as_str(),
                    edge.target.as_str(),
                    edge.resolver.as_str()
                ))
                .collect::<Vec<_>>(),
            vec![
                ("app/main.ts", "src/core/util.ts", "tsconfig-paths"),
                ("app/main.ts", "src/exact.ts", "tsconfig-paths"),
            ]
        );
    }

    #[test]
    fn focused_graph_queries_follow_the_requested_direction_and_depth() {
        let dir = tempdir().unwrap();
        for (path, content) in [
            ("app.ts", "import './service';\n"),
            ("service.ts", "import './db';\n"),
            ("db.ts", "export const db = 1;\n"),
        ] {
            std::fs::write(dir.path().join(path), content).unwrap();
        }
        let files = [
            file_report("app.ts"),
            file_report("service.ts"),
            file_report("db.ts"),
        ];

        let dependencies = analyze_with_query(
            &files,
            dir.path(),
            &[PathBuf::from("service.ts")],
            GraphDirection::Dependencies,
            1,
        )
        .report;
        assert_eq!(
            dependencies
                .files
                .iter()
                .map(|file| (file.path.as_str(), file.focus_distance))
                .collect::<Vec<_>>(),
            vec![("db.ts", Some(1)), ("service.ts", Some(0))]
        );
        assert_eq!(dependencies.edges, 1);

        let dependents = analyze_with_query(
            &files,
            dir.path(),
            &[PathBuf::from("service.ts")],
            GraphDirection::Dependents,
            1,
        )
        .report;
        assert_eq!(
            dependents
                .files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            vec!["app.ts", "service.ts"]
        );

        let seed_only = analyze_with_query(
            &files,
            dir.path(),
            &[PathBuf::from("service.ts")],
            GraphDirection::Both,
            0,
        )
        .report;
        assert_eq!(seed_only.nodes, 1);
        assert_eq!(seed_only.edges, 0);
        assert_eq!(seed_only.files[0].path, "service.ts");

        let unmatched = analyze_with_query(
            &files,
            dir.path(),
            &[PathBuf::from("missing")],
            GraphDirection::Both,
            2,
        )
        .report;
        assert_eq!(unmatched.nodes, 0);
        assert_eq!(unmatched.unmatched_focus, ["missing"]);
    }

    #[test]
    fn invalid_tsconfig_is_diagnostic_and_reduces_impact_confidence() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("tsconfig.json"), "{ invalid json").unwrap();
        std::fs::write(dir.path().join("changed.ts"), "export const changed = 1;\n").unwrap();
        std::fs::write(dir.path().join("consumer.ts"), "import './changed';\n").unwrap();
        let paths = vec![PathBuf::from("changed.ts"), PathBuf::from("consumer.ts")];

        let graph = build(
            &[file_report("changed.ts"), file_report("consumer.ts")],
            dir.path(),
        );
        assert_eq!(graph.config_errors, 1);

        let impact = impact(
            &paths,
            dir.path(),
            &HashSet::from([PathBuf::from("changed.ts")]),
        );
        assert_eq!(impact.config_errors, 1);
        assert_eq!(impact.confidence, "partial");
    }

    #[test]
    fn diagnostic_facts_merge_all_errors_for_the_same_path() {
        let analysis = GraphAnalysis {
            report: DepGraph::default(),
            signals: GraphSignals::default(),
            topology: Topology {
                graph_files: vec!["src/main.rs".to_string()],
                edges: Vec::new(),
                unresolved_imports: 2,
                unresolved_by_node: vec![2],
                parse_errors_by_node: vec![1],
                unreadable_nodes: HashSet::new(),
                parse_errors: 1,
                edge_resolvers: BTreeMap::new(),
                config_errors: 3,
                config_errors_by_path: BTreeMap::from([("src/main.rs".to_string(), 3)]),
                config_files: Vec::new(),
                symbols: Vec::new(),
                symbol_edges: Vec::new(),
                unresolved_symbol_relations: 0,
                unresolved_symbol_relations_by_path: HashMap::new(),
            },
        };

        let facts = diagnostic_facts(&analysis);

        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].path, "src/main.rs");
        assert_eq!(facts[0].parse_errors, 1);
        assert_eq!(facts[0].unresolved_imports, 2);
        assert_eq!(facts[0].config_errors, 3);
    }

    #[test]
    fn local_package_exports_and_imports_resolve_with_provenance() {
        let dir = tempdir().unwrap();
        for directory in ["apps/web", "packages/core/src"] {
            std::fs::create_dir_all(dir.path().join(directory)).unwrap();
        }
        std::fs::write(
            dir.path().join("packages/core/package.json"),
            r##"{
                "name": "@acme/core",
                "exports": {
                    ".": "./src/index.js",
                    "./*": { "import": "./src/*.js", "types": "./src/*.d.ts" }
                },
                "imports": { "#internal": "./src/internal.js" }
            }"##,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("apps/web/main.ts"),
            "import '@acme/core';\nimport '@acme/core/feature';\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("packages/core/src/index.ts"),
            "import '#internal';\nexport const core = 1;\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("packages/core/src/feature.ts"),
            "export const feature = 1;\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("packages/core/src/internal.ts"),
            "export const internal = 1;\n",
        )
        .unwrap();

        let graph = build(
            &[
                file_report("apps/web/main.ts"),
                file_report("packages/core/src/index.ts"),
                file_report("packages/core/src/feature.ts"),
                file_report("packages/core/src/internal.ts"),
            ],
            dir.path(),
        );

        assert_eq!(graph.edges, 3);
        assert_eq!(graph.unresolved_imports, 0);
        assert!(
            graph
                .config_files
                .contains(&"packages/core/package.json".to_string())
        );
        assert!(graph.edge_list.iter().any(|edge| {
            edge.source == "apps/web/main.ts"
                && edge.target == "packages/core/src/index.ts"
                && edge.resolver == "package-exports"
        }));
        assert!(graph.edge_list.iter().any(|edge| {
            edge.source == "packages/core/src/index.ts"
                && edge.target == "packages/core/src/internal.ts"
                && edge.resolver == "package-imports"
        }));
    }

    #[test]
    fn explicit_null_package_exports_do_not_fall_back_to_entrypoints() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("apps/web")).unwrap();
        std::fs::create_dir_all(dir.path().join("packages/core/src")).unwrap();
        std::fs::write(
            dir.path().join("packages/core/package.json"),
            r#"{
                "name": "@acme/core",
                "exports": { ".": null },
                "main": "./src/index.js"
            }"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("apps/web/main.ts"),
            "import '@acme/core';\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("packages/core/src/index.ts"),
            "export const core = 1;\n",
        )
        .unwrap();

        let graph = build(
            &[
                file_report("apps/web/main.ts"),
                file_report("packages/core/src/index.ts"),
            ],
            dir.path(),
        );

        assert_eq!(graph.edges, 0);
        assert_eq!(graph.unresolved_imports, 1);
    }

    #[test]
    fn explicit_package_export_blocks_override_broader_wildcards() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("apps/web")).unwrap();
        std::fs::create_dir_all(dir.path().join("packages/core/src")).unwrap();
        std::fs::write(
            dir.path().join("packages/core/package.json"),
            r#"{
                "name": "@acme/core",
                "exports": {
                    "./private": null,
                    "./*": "./src/*.js"
                }
            }"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("apps/web/main.ts"),
            "import '@acme/core/private';\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("packages/core/src/private.ts"),
            "export const privateValue = 1;\n",
        )
        .unwrap();

        let graph = build(
            &[
                file_report("apps/web/main.ts"),
                file_report("packages/core/src/private.ts"),
            ],
            dir.path(),
        );

        assert_eq!(graph.edges, 0);
        assert_eq!(graph.unresolved_imports, 1);
    }

    #[test]
    fn duplicate_local_package_names_are_diagnostic_and_unresolved() {
        let dir = tempdir().unwrap();
        for directory in ["apps/web", "packages/a/src", "packages/b/src"] {
            std::fs::create_dir_all(dir.path().join(directory)).unwrap();
        }
        for package in ["a", "b"] {
            std::fs::write(
                dir.path().join(format!("packages/{package}/package.json")),
                r#"{ "name": "@acme/core", "main": "./src/index.js" }"#,
            )
            .unwrap();
            std::fs::write(
                dir.path().join(format!("packages/{package}/src/index.ts")),
                "export const core = 1;\n",
            )
            .unwrap();
        }
        std::fs::write(
            dir.path().join("apps/web/main.ts"),
            "import '@acme/core';\n",
        )
        .unwrap();

        let graph = build(
            &[
                file_report("apps/web/main.ts"),
                file_report("packages/a/src/index.ts"),
                file_report("packages/b/src/index.ts"),
            ],
            dir.path(),
        );

        assert_eq!(graph.edges, 0);
        assert_eq!(graph.unresolved_imports, 1);
        assert_eq!(graph.config_errors, 1);
    }

    #[test]
    fn python_absolute_imports_resolve_from_conventional_src_roots() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("apps")).unwrap();
        std::fs::create_dir_all(dir.path().join("src/domain")).unwrap();
        std::fs::write(
            dir.path().join("apps/main.py"),
            "from domain.service import VALUE\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("src/domain/__init__.py"), "\n").unwrap();
        std::fs::write(dir.path().join("src/domain/service.py"), "VALUE = 1\n").unwrap();

        let graph = build(
            &[
                file_report("apps/main.py"),
                file_report("src/domain/__init__.py"),
                file_report("src/domain/service.py"),
            ],
            dir.path(),
        );

        assert_eq!(graph.unresolved_imports, 0);
        assert!(graph.edge_list.iter().any(|edge| {
            edge.source == "apps/main.py"
                && edge.target == "src/domain/service.py"
                && edge.resolver == "python-src-root"
        }));
    }

    #[test]
    fn python_absolute_imports_prefer_the_nearest_package_src_root() {
        let dir = tempdir().unwrap();
        for directory in [
            "packages/a/tests",
            "packages/a/src/domain",
            "packages/b/src/domain",
        ] {
            std::fs::create_dir_all(dir.path().join(directory)).unwrap();
        }
        std::fs::write(
            dir.path().join("packages/a/tests/main.py"),
            "from domain.service import VALUE\n",
        )
        .unwrap();
        for path in [
            "packages/a/src/domain/service.py",
            "packages/b/src/domain/service.py",
        ] {
            std::fs::write(dir.path().join(path), "VALUE = 1\n").unwrap();
        }

        let graph = build(
            &[
                file_report("packages/a/tests/main.py"),
                file_report("packages/a/src/domain/service.py"),
                file_report("packages/b/src/domain/service.py"),
            ],
            dir.path(),
        );

        assert_eq!(graph.edges, 1);
        assert!(graph.edge_list.iter().any(|edge| {
            edge.source == "packages/a/tests/main.py"
                && edge.target == "packages/a/src/domain/service.py"
                && edge.resolver == "python-src-root"
        }));
    }

    #[test]
    fn python_absolute_imports_leave_unrelated_src_roots_ambiguous() {
        let dir = tempdir().unwrap();
        for directory in ["apps", "packages/a/src/domain", "packages/b/src/domain"] {
            std::fs::create_dir_all(dir.path().join(directory)).unwrap();
        }
        std::fs::write(
            dir.path().join("apps/main.py"),
            "from domain.service import VALUE\n",
        )
        .unwrap();
        for path in [
            "packages/a/src/domain/service.py",
            "packages/b/src/domain/service.py",
        ] {
            std::fs::write(dir.path().join(path), "VALUE = 1\n").unwrap();
        }

        let graph = build(
            &[
                file_report("apps/main.py"),
                file_report("packages/a/src/domain/service.py"),
                file_report("packages/b/src/domain/service.py"),
            ],
            dir.path(),
        );

        assert_eq!(graph.edges, 0);
        assert_eq!(graph.unresolved_imports, 1);
    }

    #[test]
    fn javascript_runtime_extensions_substitute_typescript_sources() {
        let nodes = HashSet::from(["src/value.ts".to_string(), "src/component.tsx".to_string()]);
        assert_eq!(
            try_resolve_js("src/value.js", &nodes),
            Some("src/value.ts".to_string())
        );
        assert_eq!(
            try_resolve_js("src/component.jsx", &nodes),
            Some("src/component.tsx".to_string())
        );
    }

    #[test]
    fn package_internal_specifiers_without_metadata_are_unresolved() {
        assert!(matches!(
            JsResolver::default().resolve("src/main.ts", "#internal", &HashSet::new()),
            ImportResolution::Unresolved
        ));
    }

    #[test]
    fn virtual_deleted_files_still_seed_existing_dependents() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/consumer.ts"),
            "import './deleted.js';\n",
        )
        .unwrap();
        let virtual_paths = HashSet::from(["src/deleted.ts".to_string()]);
        let analysis = analyze_paths(
            &[
                PathBuf::from("src/consumer.ts"),
                PathBuf::from("src/deleted.ts"),
            ],
            dir.path(),
            &virtual_paths,
        );

        let deleted = analysis.signals.files.get("src/deleted.ts").unwrap();
        assert_eq!(deleted.dependents, ["src/consumer.ts"]);
        assert_eq!(
            deleted
                .dependent_resolvers
                .get("src/consumer.ts")
                .map(String::as_str),
            Some("relative")
        );
        assert!(analysis.topology.unreadable_nodes.is_empty());
    }

    // -- resolve_py ----------------------------------------------------------

    #[test]
    fn resolve_py_relative() {
        let nodes: HashSet<String> = ["pkg/foo.py", "pkg/sub/__init__.py", "pkg/bar/__init__.py"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        assert_eq!(
            resolve_py("pkg/x.py", ".foo", &nodes),
            Some("pkg/foo.py".to_string())
        );
        assert_eq!(
            resolve_py("pkg/sub/x.py", "..bar", &nodes),
            Some("pkg/bar/__init__.py".to_string())
        );
    }

    #[test]
    fn resolve_py_absolute_is_none() {
        let nodes: HashSet<String> = ["os.py"].iter().map(|s| s.to_string()).collect();
        assert_eq!(resolve_py("pkg/x.py", "os", &nodes), None);
    }

    // -- strongly_connected --------------------------------------------------

    #[test]
    fn scc_two_cycle() {
        let nodes: Vec<String> = vec!["a".to_string(), "b".to_string()];
        // 0 -> 1, 1 -> 0
        let edges = vec![(0usize, 1usize), (1, 0)];
        let sccs = strongly_connected(&nodes, &edges);
        let big: Vec<_> = sccs.iter().filter(|c| c.len() >= 2).collect();
        assert_eq!(big.len(), 1, "expected exactly one cycle component");
        let mut comp = big[0].clone();
        comp.sort_unstable();
        assert_eq!(comp, vec![0, 1]);
    }

    #[test]
    fn scc_acyclic() {
        let nodes: Vec<String> = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        // 0 -> 1 -> 2 (no cycle)
        let edges = vec![(0usize, 1usize), (1, 2)];
        let sccs = strongly_connected(&nodes, &edges);
        // Every component must be a singleton.
        assert!(
            sccs.iter().all(|c| c.len() == 1),
            "expected only singletons: {sccs:?}"
        );
    }

    // -- is_entrypoint -------------------------------------------------------

    #[test]
    fn entrypoints() {
        assert!(is_entrypoint("index.ts"));
        assert!(is_entrypoint("src/index.js"));
        assert!(is_entrypoint("__init__.py"));
        assert!(is_entrypoint("pkg/__init__.py"));
        assert!(is_entrypoint("next.config.js"));
        assert!(is_entrypoint("vite.config.ts"));
        assert!(is_entrypoint("types.d.ts"));
        assert!(is_entrypoint("src/main.ts"));
        assert!(is_entrypoint("app.py"));
    }

    #[test]
    fn non_entrypoints() {
        assert!(!is_entrypoint("foo.ts"));
        assert!(!is_entrypoint("util.py"));
        assert!(!is_entrypoint("helper.js"));
    }

    // -- normalize_path / go_up (smoke tests) --------------------------------

    #[test]
    fn normalize_dots() {
        assert_eq!(normalize_path("src/./a"), "src/a");
        assert_eq!(normalize_path("src/sub/../c"), "src/c");
        assert_eq!(normalize_path("./a"), "a");
    }

    #[test]
    fn go_up_levels() {
        assert_eq!(go_up("a/b/c", 1), "a/b");
        assert_eq!(go_up("a/b/c", 2), "a");
        assert_eq!(go_up("a", 1), "");
    }
}
