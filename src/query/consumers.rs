use super::{SourceQueryTarget, SourceSelector};
use crate::config::Config;
use crate::graph::{self, GraphReadLimits};
use crate::metrics::tokens::TokenCounter;
use crate::model::{
    CallReferenceStatus, CallReferenceTopology, CallSymbolIdentity, ConsumerCoverage, ConsumerHit,
    ConsumersDirection, ConsumersQueryReport, FindReadSelector, FindReadTarget, SCHEMA_VERSION,
    SourceRevision,
};
use crate::report::Format;
use crate::scan;
use anyhow::{Context, Result, ensure};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

pub(super) const DEFAULT_DEPTH: usize = 1;
pub(super) const MAX_DEPTH: usize = 8;
pub(super) const DEFAULT_LIMIT: usize = 20;
pub(super) const MAX_LIMIT: usize = 100;
pub(super) const DEFAULT_PATH_LIMIT: usize = 20;
pub(super) const MAX_PATH_LIMIT: usize = 100;
pub(super) const DEFAULT_TOKENS: usize = 4_096;
pub(super) const MIN_TOKENS: usize = 256;
pub(super) const MAX_TOKENS: usize = 65_536;
pub(super) const DEFAULT_BYTES: usize = 65_536;
pub(super) const MIN_BYTES: usize = 1_024;
pub(super) const MAX_BYTES: usize = 1_048_576;

/// Explicit worktree seeds, graph traversal bounds and complete-response output limits.
#[derive(Debug, Clone)]
pub struct ConsumersQueryOptions {
    pub targets: Vec<SourceQueryTarget>,
    pub direction: ConsumersDirection,
    pub depth: usize,
    pub limit: usize,
    pub path_limit: usize,
    pub token_budget: usize,
    pub byte_budget: usize,
    pub format: Format,
    pub pretty_json: bool,
}

pub struct ConsumersQueryOutput {
    pub report: ConsumersQueryReport,
    pub rendered: String,
}

type SymbolKey = (String, String, usize, usize);
fn key(symbol: &CallSymbolIdentity) -> SymbolKey {
    (
        symbol.path.clone(),
        symbol.source_hash.clone(),
        symbol.declaration_span.start_byte,
        symbol.declaration_span.end_byte,
    )
}

/// Find conservatively bound reachable symbols from explicit worktree seeds and render bounded body-free evidence.
///
/// # Errors
///
/// Returns an error for invalid options or roots, unsupported platforms, unresolved or ambiguous
/// seeds, mismatched expected hashes, unrecoverable scan or graph failures, token initialization
/// or serialization failures, or a budget that cannot hold the minimal status envelope.
pub fn consumers(
    target: &Path,
    cfg: &Config,
    exclusions: &[PathBuf],
    options: &ConsumersQueryOptions,
) -> Result<ConsumersQueryOutput> {
    validate(options)?;
    let target_root = target
        .canonicalize()
        .context("consumers root cannot be resolved")?;
    ensure!(target_root.is_dir(), "consumers root must be a directory");
    let root_alias = std::path::absolute(target)?;
    let normalized = options
        .targets
        .iter()
        .map(|target| super::source::normalize_path(&target_root, &root_alias, &target.path))
        .collect::<Vec<_>>();
    ensure!(
        normalized.iter().all(Option::is_some),
        "consumers target escapes the selected root"
    );
    let targets = super::source::with_file_expectations(&options.targets, &normalized)?;
    let config = super::declaration_query_config(cfg);
    let artifacts = scan::run_with_artifacts(
        &target_root,
        &config,
        exclusions,
        scan::ArtifactRequirements {
            graph_facts: true,
            ..scan::ArtifactRequirements::default()
        },
    )?;
    let root = &artifacts.report.root;
    let prefix = target_root
        .strip_prefix(root)
        .context("consumers target is outside captured repository")?;
    let seeds = resolve_seeds(&artifacts, prefix, &targets, &normalized)?;
    let topology = graph::resolve_call_references(
        root,
        &artifacts.graph_facts,
        &artifacts.resolver_configs,
        GraphReadLimits::from_config(&config),
    );
    let coverage = coverage(&artifacts, &topology);
    project(root, &config, options, seeds, coverage, &topology)
}

fn project(
    root: &Path,
    config: &Config,
    options: &ConsumersQueryOptions,
    seeds: Vec<CallSymbolIdentity>,
    coverage: ConsumerCoverage,
    topology: &CallReferenceTopology,
) -> Result<ConsumersQueryOutput> {
    let (hits, depth_omitted) = traverse(topology, &seeds, options);
    let total_matches = hits.len();
    let mut paths = BTreeSet::new();
    let mut admitted = Vec::new();
    let mut path_omitted = 0;
    let mut limit_omitted = 0;
    for hit in hits {
        if !paths.contains(&hit.symbol.path) && paths.len() >= options.path_limit {
            path_omitted += 1;
            continue;
        }
        paths.insert(hit.symbol.path.clone());
        if admitted.len() >= options.limit {
            limit_omitted += 1;
            continue;
        }
        admitted.push(hit);
    }
    let counter = TokenCounter::new(&config.encoding)?;
    let mut report = ConsumersQueryReport {
        kind: "consumers_query".to_string(),
        schema_version: SCHEMA_VERSION.to_string(),
        root: Some(root.to_path_buf()),
        root_omitted: false,
        encoding: counter.name().to_string(),
        direction: options.direction,
        depth: options.depth,
        limit: options.limit,
        path_limit: options.path_limit,
        token_budget: options.token_budget,
        byte_budget: options.byte_budget,
        seeds,
        coverage,
        total_matches,
        returned_matches: 0,
        depth_omitted,
        path_omitted,
        limit_omitted,
        budget_omitted: admitted.len(),
        unresolved: Vec::new(),
        unresolved_omitted: topology.unresolved.len(),
        hits: Vec::new(),
    };
    if !fits(&report, options, &counter)? {
        report.root = None;
        report.root_omitted = true;
    }
    ensure!(
        fits(&report, options, &counter)?,
        "consumers budget cannot fit the status envelope and seed identities"
    );
    for hit in admitted {
        report.hits.push(hit);
        report.returned_matches += 1;
        report.budget_omitted -= 1;
        if !fits(&report, options, &counter)? {
            report.hits.pop();
            report.returned_matches -= 1;
            report.budget_omitted += 1;
        }
    }
    for unresolved in topology.unresolved.iter().take(20) {
        report.unresolved.push(unresolved.clone());
        report.unresolved_omitted -= 1;
        if !fits(&report, options, &counter)? {
            report.unresolved.pop();
            report.unresolved_omitted += 1;
        }
    }
    let rendered = crate::report::consumers::render(&report, options.format, options.pretty_json)?;
    Ok(ConsumersQueryOutput { report, rendered })
}

fn fits(
    report: &ConsumersQueryReport,
    options: &ConsumersQueryOptions,
    counter: &TokenCounter,
) -> Result<bool> {
    let output = crate::report::consumers::render(report, options.format, options.pretty_json)?;
    Ok(output.len() <= options.byte_budget && counter.count(&output) <= options.token_budget)
}

fn validate(options: &ConsumersQueryOptions) -> Result<()> {
    ensure!(
        !options.targets.is_empty() && options.targets.len() <= 32,
        "consumers requires 1..=32 targets"
    );
    ensure!(
        (1..=MAX_DEPTH).contains(&options.depth),
        "consumers depth must be 1..=8"
    );
    ensure!(
        (1..=MAX_LIMIT).contains(&options.limit),
        "consumers limit must be 1..=100"
    );
    ensure!(
        (1..=MAX_PATH_LIMIT).contains(&options.path_limit),
        "consumers path limit must be 1..=100"
    );
    ensure!(
        (MIN_TOKENS..=MAX_TOKENS).contains(&options.token_budget)
            && (MIN_BYTES..=MAX_BYTES).contains(&options.byte_budget),
        "consumers output budget is outside supported limits"
    );
    ensure!(
        matches!(
            options.format,
            Format::Json | Format::Ndjson | Format::Table | Format::Markdown
        ),
        "unsupported consumers output format"
    );
    ensure!(
        !options.pretty_json || options.format == Format::Json,
        "pretty output requires JSON format"
    );
    for target in &options.targets {
        ensure!(
            target.snapshot == SourceRevision::Worktree,
            "consumers supports only captured worktree targets; historical hashes must not be substituted"
        );
        ensure!(
            target.path.as_os_str().len() <= 4096,
            "consumers path exceeds 4096 bytes"
        );
        if let Some(hash) = &target.expected_hash {
            ensure!(
                hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()),
                "consumers expected hash must be a SHA-256 hex digest"
            );
        }
        match &target.selector {
            SourceSelector::Symbol(name) => ensure!(
                !name.is_empty() && name.len() <= 1024,
                "invalid consumers symbol"
            ),
            SourceSelector::Line(line) => ensure!(*line > 0, "consumers line must be positive"),
            SourceSelector::Outline => {
                anyhow::bail!("consumers requires a symbol or line selector")
            }
        }
    }
    Ok(())
}

fn traverse(
    topology: &CallReferenceTopology,
    seeds: &[CallSymbolIdentity],
    options: &ConsumersQueryOptions,
) -> (Vec<ConsumerHit>, usize) {
    let mut adjacency: BTreeMap<SymbolKey, Vec<(CallSymbolIdentity, usize)>> = BTreeMap::new();
    for (index, edge) in topology.edges.iter().enumerate() {
        if options.direction != ConsumersDirection::Outgoing {
            adjacency
                .entry(key(&edge.target))
                .or_default()
                .push((edge.source.clone(), index));
        }
        if options.direction != ConsumersDirection::Incoming {
            adjacency
                .entry(key(&edge.source))
                .or_default()
                .push((edge.target.clone(), index));
        }
    }
    for neighbors in adjacency.values_mut() {
        neighbors.sort_by_key(|(symbol, index)| (key(symbol), *index));
    }
    let mut distances = BTreeMap::new();
    let mut queue = VecDeque::new();
    for seed in seeds {
        distances.insert(key(seed), 0);
        queue.push_back(seed.clone());
    }
    let mut hits: BTreeMap<SymbolKey, ConsumerHit> = BTreeMap::new();
    while let Some(current) = queue.pop_front() {
        let distance = distances[&key(&current)];
        for (neighbor, edge_index) in adjacency.get(&key(&current)).into_iter().flatten() {
            let identity = key(neighbor);
            let next = distance + 1;
            if !distances.contains_key(&identity) {
                distances.insert(identity.clone(), next);
                queue.push_back(neighbor.clone());
            }
            if next > options.depth || distances[&identity] != next {
                continue;
            }
            let hit = hits.entry(identity).or_insert_with(|| ConsumerHit {
                symbol: neighbor.clone(),
                depth: next,
                evidence: Vec::new(),
                read: FindReadTarget {
                    path: PathBuf::from(&neighbor.path),
                    selector: FindReadSelector::Symbol(neighbor.name.clone()),
                    expected_hash: neighbor.source_hash.clone(),
                    snapshot: SourceRevision::Worktree,
                },
            });
            let edge = &topology.edges[*edge_index];
            if !hit.evidence.contains(edge) {
                hit.evidence.push(edge.clone());
            }
        }
    }
    let depth_omitted = distances
        .values()
        .filter(|depth| **depth > options.depth)
        .count();
    let mut hits = hits.into_values().collect::<Vec<_>>();
    hits.sort_by_key(|hit| (hit.depth, key(&hit.symbol)));
    (hits, depth_omitted)
}

pub(super) fn capability() -> crate::model::ConsumersQueryCapability {
    crate::model::ConsumersQueryCapability {
        command: "consumers".to_string(),
        formats: ["table", "json", "markdown", "ndjson"]
            .map(str::to_string)
            .to_vec(),
        snapshots: vec!["worktree".to_string()],
        directions: ["incoming", "outgoing", "both"]
            .map(str::to_string)
            .to_vec(),
        default_depth: DEFAULT_DEPTH,
        max_depth: MAX_DEPTH,
        default_limit: DEFAULT_LIMIT,
        max_limit: MAX_LIMIT,
        default_path_limit: DEFAULT_PATH_LIMIT,
        max_path_limit: MAX_PATH_LIMIT,
        default_tokens: DEFAULT_TOKENS,
        min_tokens: MIN_TOKENS,
        max_tokens: MAX_TOKENS,
        default_bytes: DEFAULT_BYTES,
        min_bytes: MIN_BYTES,
        max_bytes: MAX_BYTES,
    }
}

fn resolve_seeds(
    artifacts: &scan::ScanArtifacts,
    prefix: &Path,
    targets: &[SourceQueryTarget],
    normalized: &[Option<PathBuf>],
) -> Result<Vec<CallSymbolIdentity>> {
    let mut seeds = Vec::new();
    for (requested, relative) in targets.iter().zip(normalized) {
        let path = prefix.join(relative.as_ref().context("invalid consumers path")?);
        let file = artifacts
            .graph_facts
            .get(&path)
            .and_then(|facts| facts.call_references.as_ref())
            .context("consumers seed has no captured call facts")?;
        if let Some(hash) = &requested.expected_hash {
            ensure!(
                file.source_hash.eq_ignore_ascii_case(hash),
                "consumers seed hash mismatch for {}",
                path.display()
            );
        }
        let definitions = artifacts
            .definitions
            .get(&path)
            .context("consumers seed has no captured definitions")?;
        let mut matches = file
            .declarations
            .iter()
            .filter(|declaration| match &requested.selector {
                SourceSelector::Symbol(name) => {
                    declaration.symbol.name == *name
                        || declaration
                            .symbol
                            .name
                            .rsplit([':', '.'])
                            .find(|part| !part.is_empty())
                            == Some(name.as_str())
                }
                SourceSelector::Line(line) => definitions.definitions.iter().any(|definition| {
                    definition.declaration_span == declaration.symbol.declaration_span
                        && definition
                            .source_span
                            .as_ref()
                            .unwrap_or(&definition.declaration_span)
                            .start_line
                            <= *line
                        && *line
                            <= definition
                                .source_span
                                .as_ref()
                                .unwrap_or(&definition.declaration_span)
                                .end_line
                }),
                SourceSelector::Outline => false,
            })
            .collect::<Vec<_>>();
        match &requested.selector {
            SourceSelector::Symbol(name)
                if matches
                    .iter()
                    .any(|declaration| declaration.symbol.name == *name) =>
            {
                matches.retain(|declaration| declaration.symbol.name == *name);
            }
            SourceSelector::Line(_) => {
                let lengths = definitions
                    .definitions
                    .iter()
                    .map(|definition| {
                        let span = definition
                            .source_span
                            .as_ref()
                            .unwrap_or(&definition.declaration_span);
                        (
                            definition.declaration_span.start_byte,
                            span.end_byte.saturating_sub(span.start_byte),
                        )
                    })
                    .collect::<BTreeMap<_, _>>();
                let smallest = matches
                    .iter()
                    .filter_map(|declaration| {
                        lengths.get(&declaration.symbol.declaration_span.start_byte)
                    })
                    .min();
                matches.retain(|declaration| {
                    lengths.get(&declaration.symbol.declaration_span.start_byte) == smallest
                });
            }
            SourceSelector::Symbol(_) | SourceSelector::Outline => {}
        }
        ensure!(
            matches.len() == 1,
            "consumers seed must identify exactly one declaration in {} (found {})",
            path.display(),
            matches.len()
        );
        seeds.push(matches[0].symbol.clone());
    }
    seeds.sort_by_key(key);
    seeds.dedup_by(|left, right| key(left) == key(right));
    Ok(seeds)
}

fn coverage(artifacts: &scan::ScanArtifacts, topology: &CallReferenceTopology) -> ConsumerCoverage {
    let mut coverage = ConsumerCoverage {
        files_total: artifacts.report.files.len(),
        discovery_omitted: artifacts.report.diagnostics.files_omitted_by_limit,
        discovery_omitted_count_incomplete: artifacts
            .report
            .diagnostics
            .files_omitted_count_incomplete,
        unreadable_files: artifacts.report.diagnostics.unreadable_files,
        oversized_files: artifacts.report.diagnostics.oversized_files,
        walker_errors: artifacts.report.diagnostics.walker_errors,
        scan_truncated: artifacts.report.diagnostics.scan_truncated,
        deadline_reached: artifacts.report.diagnostics.duration_limit_reached,
        resolution: topology.coverage.clone(),
        ..ConsumerCoverage::default()
    };
    for path in artifacts.report.files.iter().map(|file| &file.path) {
        let Some(file) = artifacts
            .graph_facts
            .get(path)
            .and_then(|facts| facts.call_references.as_ref())
        else {
            coverage.files_unavailable += 1;
            continue;
        };
        match file.status {
            CallReferenceStatus::Available => coverage.files_available += 1,
            CallReferenceStatus::Unsupported => coverage.files_unsupported += 1,
            CallReferenceStatus::Unavailable => coverage.files_unavailable += 1,
            CallReferenceStatus::ParseErrors => coverage.files_parse_errors += 1,
            CallReferenceStatus::FactTruncated | CallReferenceStatus::WorkTruncated => {
                coverage.files_truncated += 1;
            }
        }
        coverage.declarations_omitted += file.coverage.omitted_declarations;
        coverage.relations_omitted += file.coverage.omitted_relations;
    }
    coverage
}
