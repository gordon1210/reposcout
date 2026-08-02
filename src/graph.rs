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
#[derive(Debug, Clone, Copy)]
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
    #[must_use]
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

#[derive(Clone, Copy)]
struct GraphQuery<'a> {
    root: &'a Path,
    focus: &'a [PathBuf],
    direction: GraphDirection,
    depth: usize,
}

/// Build structural topology from the already-scanned file list.
#[must_use]
pub fn build(files: &[crate::model::FileReport], root: &Path) -> DepGraph {
    build_with_limits(files, root, GraphReadLimits::default(), None)
}

/// Build structural topology with explicit read limits and optional source facts.
#[must_use]
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
#[must_use]
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
#[must_use]
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
    let is_json = Path::new(relative)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"));
    name == "tsconfig.json"
        || name == "jsconfig.json"
        || (name.starts_with("tsconfig.") && is_json)
        || (name.starts_with("jsconfig.") && is_json)
        || (is_json
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

#[expect(
    clippy::too_many_arguments,
    reason = "this internal adapter keeps graph facts, resolver config, resource limits, and query projection independently selectable"
)]
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

#[expect(
    clippy::too_many_arguments,
    reason = "this internal graph entry point exposes independent source, trust-policy, and projection inputs without a public options type"
)]
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
    build_from_paths_with_query(GraphBuildRequest {
        paths: &paths,
        root,
        virtual_paths: &HashSet::new(),
        source_facts: facts,
        resolver_configs,
        limits,
        focus,
        direction,
        depth,
    })
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
    build_from_paths_with_query(GraphBuildRequest {
        paths,
        root,
        virtual_paths,
        source_facts: None,
        resolver_configs: None,
        limits: GraphReadLimits::default(),
        focus: &[],
        direction: GraphDirection::Both,
        depth: 1,
    })
}

pub(crate) fn analyze_paths_with_facts(
    paths: &[PathBuf],
    root: &Path,
    virtual_paths: &HashSet<String>,
    facts: &BTreeMap<PathBuf, SourceFacts>,
    resolver_configs: Option<&BTreeMap<String, String>>,
    limits: GraphReadLimits,
) -> GraphAnalysis {
    build_from_paths_with_query(GraphBuildRequest {
        paths,
        root,
        virtual_paths,
        source_facts: Some(facts),
        resolver_configs,
        limits: GraphReadLimits {
            facts_only_sources: true,
            ..limits
        },
        focus: &[],
        direction: GraphDirection::Both,
        depth: 1,
    })
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
#[must_use]
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
    let built = build_from_paths_with_query(GraphBuildRequest {
        paths: &paths,
        root,
        virtual_paths: &HashSet::new(),
        source_facts: Some(facts),
        resolver_configs,
        limits: GraphReadLimits {
            facts_only_sources: true,
            ..limits
        },
        focus: &[],
        direction: GraphDirection::Both,
        depth: 1,
    });
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
#[must_use]
pub fn impact<S>(paths: &[PathBuf], root: &Path, changed: &HashSet<PathBuf, S>) -> ImpactAnalysis
where
    S: std::hash::BuildHasher,
{
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
    build_from_paths_with_query(GraphBuildRequest {
        paths,
        root,
        virtual_paths,
        source_facts: None,
        resolver_configs: None,
        limits: GraphReadLimits::default(),
        focus: &[],
        direction: GraphDirection::Both,
        depth: 1,
    })
}

#[derive(Clone, Copy)]
struct GraphBuildRequest<'a> {
    paths: &'a [PathBuf],
    root: &'a Path,
    virtual_paths: &'a HashSet<String>,
    source_facts: Option<&'a BTreeMap<PathBuf, SourceFacts>>,
    resolver_configs: Option<&'a BTreeMap<String, String>>,
    limits: GraphReadLimits,
    focus: &'a [PathBuf],
    direction: GraphDirection,
    depth: usize,
}

struct GraphUniverse {
    files: Vec<String>,
    languages: Vec<String>,
    node_set: HashSet<String>,
    node_index: HashMap<String, usize>,
}

struct Resolvers {
    javascript: JsResolver,
    python: PythonResolver,
    php: PhpResolver,
    rust: RustResolver,
    go: GoResolver,
}

struct TopologyState {
    edge_set: HashSet<(usize, usize)>,
    self_cycles: BTreeSet<String>,
    unresolved_imports: usize,
    unresolved_by_node: Vec<usize>,
    unreadable_nodes: HashSet<usize>,
    parse_errors: usize,
    parse_errors_by_node: Vec<usize>,
    edge_resolvers: BTreeMap<(usize, usize), String>,
    symbols: symbols::Collector,
}

impl TopologyState {
    fn new(node_count: usize) -> Self {
        Self {
            edge_set: HashSet::new(),
            self_cycles: BTreeSet::new(),
            unresolved_imports: 0,
            unresolved_by_node: vec![0; node_count],
            unreadable_nodes: HashSet::new(),
            parse_errors: 0,
            parse_errors_by_node: vec![0; node_count],
            edge_resolvers: BTreeMap::new(),
            symbols: symbols::Collector::default(),
        }
    }
}

struct TopologyExtractor<'a> {
    root: &'a Path,
    virtual_paths: &'a HashSet<String>,
    source_facts: Option<&'a BTreeMap<PathBuf, SourceFacts>>,
    facts_only_sources: bool,
    universe: &'a GraphUniverse,
    resolvers: &'a Resolvers,
}

impl TopologyExtractor<'_> {
    fn run(&self, budget: &mut ReadBudget) -> (Topology, Vec<String>) {
        let mut state = TopologyState::new(self.universe.files.len());
        for (index, path) in self.universe.files.iter().enumerate() {
            self.process_file(index, path, budget, &mut state);
        }
        let self_cycles = std::mem::take(&mut state.self_cycles).into_iter().collect();
        (self.finish(state), self_cycles)
    }

    fn process_file(
        &self,
        index: usize,
        path: &str,
        budget: &mut ReadBudget,
        state: &mut TopologyState,
    ) {
        let Some(language) = detect(Path::new(path)).and_then(|info| info.first_class) else {
            return;
        };
        let Some(facts) = self.load_source_facts(index, path, language, budget, state) else {
            return;
        };
        state.symbols.add_facts(facts.symbols);
        state.parse_errors_by_node[index] = facts.parse_errors;
        state.parse_errors = state.parse_errors.saturating_add(facts.parse_errors);
        for specifier in facts.specifiers {
            let resolution = self.resolve_import(language, path, specifier);
            self.record_resolution(index, path, resolution, state);
        }
    }

    fn load_source_facts(
        &self,
        index: usize,
        path: &str,
        language: FirstClass,
        budget: &mut ReadBudget,
        state: &mut TopologyState,
    ) -> Option<SourceFacts> {
        if let Some(facts) = self
            .source_facts
            .and_then(|facts| facts.get(Path::new(path)))
        {
            return Some(facts.clone());
        }
        if self.facts_only_sources {
            self.mark_unreadable(index, path, state);
            return None;
        }
        let absolute = if self.root.is_file() {
            self.root.to_path_buf()
        } else {
            self.root.join(path)
        };
        let ReadOutcome::Content(content) = fs_budget::read_text(&absolute, budget) else {
            self.mark_unreadable(index, path, state);
            return None;
        };
        Some(extract_source_facts(language, path, &content))
    }

    fn mark_unreadable(&self, index: usize, path: &str, state: &mut TopologyState) {
        if !self.virtual_paths.contains(path) {
            state.unreadable_nodes.insert(index);
        }
    }

    fn resolve_import(
        &self,
        language: FirstClass,
        importer: &str,
        specifier: ImportSpecifier,
    ) -> ImportResolution {
        match specifier {
            ImportSpecifier::Module(specifier) if language == FirstClass::Python => self
                .resolvers
                .python
                .resolve(importer, &specifier, &self.universe.node_set),
            ImportSpecifier::Module(specifier) => {
                self.resolvers
                    .javascript
                    .resolve(importer, &specifier, &self.universe.node_set)
            }
            ImportSpecifier::PhpNamespace(symbol) => {
                self.resolvers
                    .php
                    .resolve_namespace(importer, &symbol, &self.universe.node_set)
            }
            ImportSpecifier::PhpInclude(include) => {
                PhpResolver::resolve_include(importer, &include, &self.universe.node_set)
            }
            ImportSpecifier::Rust(import) => {
                self.resolvers
                    .rust
                    .resolve(importer, &import, &self.universe.node_set)
            }
            ImportSpecifier::GoPackage(package) => self.resolvers.go.resolve(importer, &package),
        }
    }

    fn record_resolution(
        &self,
        importer: usize,
        importer_path: &str,
        resolution: ImportResolution,
        state: &mut TopologyState,
    ) {
        match resolution {
            ImportResolution::Resolved {
                target,
                resolver: _,
            } if target == importer_path => {
                state.self_cycles.insert(importer_path.to_string());
            }
            ImportResolution::Resolved { target, resolver } => {
                if let Some(&imported) = self.universe.node_index.get(&target) {
                    state.edge_set.insert((importer, imported));
                    state
                        .edge_resolvers
                        .entry((importer, imported))
                        .or_insert_with(|| resolver.to_string());
                }
            }
            ImportResolution::Unresolved => {
                state.unresolved_imports = state.unresolved_imports.saturating_add(1);
                state.unresolved_by_node[importer] =
                    state.unresolved_by_node[importer].saturating_add(1);
            }
            ImportResolution::Local | ImportResolution::NonGraph | ImportResolution::External => {}
        }
    }

    fn finish(&self, state: TopologyState) -> Topology {
        let symbol_topology = state.symbols.finish();
        let mut edges = state.edge_set.into_iter().collect::<Vec<_>>();
        edges.sort_unstable();
        Topology {
            graph_files: self.universe.files.clone(),
            edges,
            unresolved_imports: state.unresolved_imports,
            unresolved_by_node: state.unresolved_by_node,
            parse_errors_by_node: state.parse_errors_by_node,
            unreadable_nodes: state.unreadable_nodes,
            parse_errors: state.parse_errors,
            edge_resolvers: state.edge_resolvers,
            config_errors: self.resolvers.config_error_count(),
            config_errors_by_path: self.resolvers.config_errors_by_path(),
            config_files: self.resolvers.config_files(),
            symbols: symbol_topology.symbols,
            symbol_edges: symbol_topology.edges,
            unresolved_symbol_relations: symbol_topology.unresolved_relations,
            unresolved_symbol_relations_by_path: symbol_topology.unresolved_by_path,
        }
    }
}

impl Resolvers {
    fn discover(
        files: &[String],
        root: &Path,
        resolver_configs: Option<&BTreeMap<String, String>>,
        budget: &mut ReadBudget,
    ) -> Self {
        let mut access = ConfigAccess {
            root,
            budget,
            snapshot: resolver_configs,
        };
        Self {
            javascript: JsResolver::discover(files, &mut access),
            python: PythonResolver::discover(files),
            php: PhpResolver::discover(files, &mut access),
            rust: RustResolver::discover(files, &mut access),
            go: GoResolver::discover(files, &mut access),
        }
    }

    fn config_error_count(&self) -> usize {
        self.javascript
            .config_errors
            .saturating_add(self.php.config_errors)
            .saturating_add(self.rust.config_errors)
            .saturating_add(self.go.config_errors)
    }

    fn config_errors_by_path(&self) -> BTreeMap<String, usize> {
        combined_config_errors([
            &self.javascript.config_errors_by_path,
            &self.php.config_errors_by_path,
            &self.rust.config_errors_by_path,
            &self.go.config_errors_by_path,
        ])
    }

    fn config_files(&self) -> Vec<String> {
        combined_config_files([
            self.javascript.config_files.as_slice(),
            self.php.config_files.as_slice(),
            self.rust.config_files.as_slice(),
            self.go.config_files.as_slice(),
        ])
    }
}

fn build_from_paths_with_query(request: GraphBuildRequest<'_>) -> GraphAnalysis {
    let mut budget = request.limits.budget();
    let universe = select_graph_universe(request.paths);
    let resolvers = Resolvers::discover(
        &universe.files,
        request.root,
        request.resolver_configs,
        &mut budget,
    );
    let extractor = TopologyExtractor {
        root: request.root,
        virtual_paths: request.virtual_paths,
        source_facts: request.source_facts,
        facts_only_sources: request.limits.facts_only_sources,
        universe: &universe,
        resolvers: &resolvers,
    };
    let (topology, self_cycles) = extractor.run(&mut budget);
    let (fan_in, fan_out) = fan_counts(universe.files.len(), &topology.edges);
    let cycles = graph_cycles(&universe.files, &topology.edges, self_cycles);
    let orphans = orphan_paths(&universe.files, &fan_in, &resolvers.go);
    let signals = graph_signals(&universe, &topology, &fan_in, &fan_out);
    let report = project_graph(
        &topology,
        &signals,
        cycles,
        orphans,
        GraphQuery {
            root: request.root,
            focus: request.focus,
            direction: request.direction,
            depth: request.depth,
        },
    );
    GraphAnalysis {
        report,
        signals,
        topology,
    }
}

fn select_graph_universe(paths: &[PathBuf]) -> GraphUniverse {
    let mut languages = BTreeSet::new();
    let mut files = paths
        .iter()
        .filter_map(|path| {
            let info = detect(path)?;
            info.first_class?;
            languages.insert(info.name.to_string());
            Some(path.to_string_lossy().replace('\\', "/"))
        })
        .collect::<Vec<_>>();
    files.sort();
    files.dedup();
    let node_set = files.iter().cloned().collect();
    let node_index = files
        .iter()
        .enumerate()
        .map(|(index, path)| (path.clone(), index))
        .collect();
    GraphUniverse {
        files,
        languages: languages.into_iter().collect(),
        node_set,
        node_index,
    }
}

fn fan_counts(node_count: usize, edges: &[(usize, usize)]) -> (Vec<usize>, Vec<usize>) {
    let mut fan_in = vec![0usize; node_count];
    let mut fan_out = vec![0usize; node_count];
    for &(importer, imported) in edges {
        fan_out[importer] = fan_out[importer].saturating_add(1);
        fan_in[imported] = fan_in[imported].saturating_add(1);
    }
    (fan_in, fan_out)
}

fn graph_cycles(
    files: &[String],
    edges: &[(usize, usize)],
    self_cycles: Vec<String>,
) -> Vec<Vec<String>> {
    let mut cycles = strongly_connected(files, edges)
        .into_iter()
        .filter(|component| component.len() >= 2)
        .map(|component| {
            let mut paths = component
                .into_iter()
                .map(|index| files[index].clone())
                .collect::<Vec<_>>();
            paths.sort();
            paths
        })
        .collect::<Vec<_>>();
    cycles.extend(self_cycles.into_iter().map(|path| vec![path]));
    cycles.sort();
    cycles
}

fn orphan_paths(files: &[String], fan_in: &[usize], go: &GoResolver) -> Vec<String> {
    let mut orphans = files
        .iter()
        .enumerate()
        .filter(|(index, path)| {
            fan_in[*index] == 0
                && !is_entrypoint(path)
                && !testcov::is_test_file(path)
                && !is_non_representative_go_file(path, go)
        })
        .map(|(_, path)| path.clone())
        .collect::<Vec<_>>();
    orphans.sort();
    orphans
}

fn is_non_representative_go_file(path: &str, resolver: &GoResolver) -> bool {
    detect(Path::new(path)).and_then(|info| info.first_class) == Some(FirstClass::Go)
        && resolver
            .packages
            .get(&path_parent(path))
            .is_some_and(|representative| representative != path)
}

fn graph_signals(
    universe: &GraphUniverse,
    topology: &Topology,
    fan_in: &[usize],
    fan_out: &[usize],
) -> GraphSignals {
    let node_count = universe.files.len();
    let mut dependencies = vec![Vec::new(); node_count];
    let mut dependents = vec![Vec::new(); node_count];
    let mut dependency_resolvers = vec![BTreeMap::new(); node_count];
    let mut dependent_resolvers = vec![BTreeMap::new(); node_count];
    for &(importer, imported) in &topology.edges {
        let imported_path = universe.files[imported].clone();
        let importer_path = universe.files[importer].clone();
        dependencies[importer].push(imported_path.clone());
        dependents[imported].push(importer_path.clone());
        let resolver = topology
            .edge_resolvers
            .get(&(importer, imported))
            .cloned()
            .unwrap_or_else(|| "heuristic".to_string());
        dependency_resolvers[importer].insert(imported_path, resolver.clone());
        dependent_resolvers[imported].insert(importer_path, resolver);
    }
    let files = universe
        .files
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
    GraphSignals {
        languages: universe.languages.clone(),
        files,
        unresolved_imports: topology.unresolved_imports,
        parse_errors: topology.parse_errors,
        config_errors: topology.config_errors,
    }
}

mod project;
use project::{impact_from_topology, project_graph};

mod source;
pub use source::SourceFacts;
use source::{ImportSpecifier, RustImport};
pub(crate) use source::{extract_source_facts, extract_source_facts_from_tree};

// ---------------------------------------------------------------------------
// Path resolvers
// ---------------------------------------------------------------------------

mod rust_python;
use rust_python::{
    ConfigAccess, ImportResolution, PythonResolver, RustResolver, candidate_resolver_config_paths,
    read_repo_text, repo_is_regular_file,
};

mod go_php;
use go_php::{
    GoResolver, PhpResolver, combined_config_errors, combined_config_files, path_in_scope,
    python_root_rank, strip_graph_prefix,
};

mod javascript;
use javascript::{JsResolver, join_graph_path, sanitize_jsonc};

mod algorithms;
pub(crate) use algorithms::is_entrypoint;
use algorithms::{
    go_up, normalize_path, path_parent, resolve_py, strongly_connected, try_resolve_js,
    try_resolve_php,
};

#[cfg(test)]
mod tests;
