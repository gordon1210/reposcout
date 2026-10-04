use super::{CaptureSession, ExplicitSourceBatch, ExplicitSourceFile, source_requirements};
use crate::config::Config;
use crate::model::{ReviewComparison, ReviewRevisionCoverage, SourceRevision};
use crate::snapshot::comparison::{self, ComparisonCapture, ComparisonChange};
use anyhow::Result;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(crate) const MAX_FILES: usize = 10_000;
pub(crate) const MAX_BYTES: u64 = 32 * 1024 * 1024;
pub(crate) const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;

pub(crate) struct ReviewRevision {
    pub tree: String,
    pub sources: ExplicitSourceBatch,
    pub coverage: ReviewRevisionCoverage,
}

pub(crate) struct ReviewCapture {
    pub root: PathBuf,
    pub deadline: Instant,
    pub comparison: ReviewComparison,
    pub changes: Vec<ComparisonChange>,
    pub total_changes: usize,
    pub revisions: [ReviewRevision; 2],
}

/// Capture changed sides first, then the bounded repository universe, sharing
/// ordinary file analysis and cache across both immutable revisions.
pub(crate) fn capture_review(
    target: &Path,
    cfg: &Config,
    exclusions: &[PathBuf],
    base: &str,
    head: &str,
    merge_base: bool,
) -> Result<ReviewCapture> {
    let limit = cfg.max_files.min(MAX_FILES);
    let started = Instant::now();
    let deadline = started
        .checked_add(Duration::from_secs(cfg.max_scan_seconds))
        .unwrap_or(started);
    let comparison = ComparisonCapture::open(
        target,
        base,
        head,
        merge_base,
        comparison::ComparisonLimits {
            files: limit,
            file_bytes: cfg
                .max_file_bytes
                .min(cfg.max_git_blob_bytes)
                .min(MAX_FILE_BYTES),
            total_bytes: cfg.max_total_bytes.min(MAX_BYTES),
            deadline,
        },
    )?;
    let root = comparison.git.root.clone();
    let trees = [
        &comparison.identity.base_tree,
        &comparison.identity.head_tree,
    ];
    let inventories =
        trees.map(|tree| comparison::inventory(&comparison.git, tree, limit, deadline));
    let [base_inventory, head_inventory] = inventories;
    let inventories = [base_inventory?, head_inventory?];
    let mut config = cfg.clone();
    config.max_files = limit;
    config.max_total_bytes = cfg.max_total_bytes.min(MAX_BYTES);
    config.max_file_bytes = cfg
        .max_file_bytes
        .min(cfg.max_git_blob_bytes)
        .min(MAX_FILE_BYTES);
    let requirements = crate::scan::ArtifactRequirements {
        graph_facts: true,
        ..source_requirements()
    };
    let revisions = trees.map(|tree| SourceRevision::Tree(tree.clone()));
    let mut session = CaptureSession::new(
        &root,
        &config,
        exclusions,
        Some(comparison.git),
        requirements,
    )?;
    session.mode = super::CaptureMode::ReviewTree;
    session.budget.deadline = Some(deadline);
    let mut batches = [empty(&root), empty(&root)];
    for change in &comparison.changes {
        for (side, file) in [change.base.as_ref(), change.head.as_ref()]
            .into_iter()
            .enumerate()
        {
            if let Some(file) = file {
                append(
                    &mut batches[side],
                    session.batch(&revisions[side], std::slice::from_ref(&file.path)),
                );
            }
        }
    }
    // Alternate sides so unchanged base files cannot consume the entire head allowance.
    for index in 0..inventories[0].paths.len().max(inventories[1].paths.len()) {
        for side in 0..2 {
            if let Some(path) = inventories[side].paths.get(index) {
                if crate::lang::detect(path).is_none()
                    && path.file_name().is_none_or(|name| name != "go.mod")
                    && path.extension().is_none_or(|extension| extension != "uid")
                {
                    continue;
                }
                append(
                    &mut batches[side],
                    session.batch(&revisions[side], std::slice::from_ref(path)),
                );
            }
        }
    }
    session.cache.save_best_effort(false, "review_context");
    let [old, new] = batches;
    let [old_inventory, new_inventory] = inventories;
    let revisions = [
        revision(&comparison.identity.base_tree, old, &old_inventory),
        revision(&comparison.identity.head_tree, new, &new_inventory),
    ];
    Ok(ReviewCapture {
        root,
        deadline,
        comparison: comparison.identity,
        changes: comparison.changes,
        total_changes: comparison.total_changes,
        revisions,
    })
}

fn empty(root: &Path) -> ExplicitSourceBatch {
    ExplicitSourceBatch {
        root: root.to_path_buf(),
        files: BTreeMap::new(),
        failures: BTreeMap::new(),
    }
}

fn append(target: &mut ExplicitSourceBatch, source: ExplicitSourceBatch) {
    target.files.extend(source.files);
    target.failures.extend(source.failures);
}

fn revision(
    tree: &str,
    sources: ExplicitSourceBatch,
    inventory: &comparison::TreeInventory,
) -> ReviewRevision {
    let mut coverage = ReviewRevisionCoverage {
        tree: tree.into(),
        observed_files: inventory.observed,
        inventory_truncated: inventory.truncated,
        captured_files: sources.files.len(),
        unsupported_inventory_files: inventory
            .paths
            .iter()
            .filter(|path| {
                !sources.files.contains_key(*path) && !sources.failures.contains_key(*path)
            })
            .count(),
        unsupported_graph_paths: sources
            .files
            .keys()
            .filter(|path| !ReviewRevision::supports_graph_path(path))
            .count(),
        ..ReviewRevisionCoverage::default()
    };
    for failure in sources.failures.values() {
        *coverage
            .unavailable_files
            .entry(super::failure_name(*failure).into())
            .or_default() += 1;
    }
    ReviewRevision {
        tree: tree.into(),
        sources,
        coverage,
    }
}

impl ReviewRevision {
    pub(crate) fn supports_graph_path(path: &Path) -> bool {
        // The shared graph uses slash-normalized string keys. Do not let a literal
        // Unix backslash alias another captured file or resolver configuration.
        path.components()
            .all(|component| !component.as_os_str().as_encoded_bytes().contains(&b'\\'))
    }

    pub(crate) fn files(&self) -> &BTreeMap<PathBuf, Arc<ExplicitSourceFile>> {
        &self.sources.files
    }

    pub(crate) fn graph_inputs(&self) -> crate::graph::GraphInputs {
        crate::graph::GraphInputs {
            source_facts: self
                .files()
                .iter()
                .filter(|(path, _)| Self::supports_graph_path(path))
                .filter_map(|(path, file)| {
                    file.graph_facts.clone().map(|facts| (path.clone(), facts))
                })
                .collect(),
            // ConfigAccess consults only this immutable map. Keeping captured text also
            // handles referenced JSON fragments and Godot UID sidecars without live probes.
            resolver_configs: self
                .files()
                .iter()
                .filter(|(path, _)| Self::supports_graph_path(path))
                .map(|(path, file)| {
                    (
                        path.to_string_lossy().replace('\\', "/"),
                        file.content.clone(),
                    )
                })
                .collect(),
        }
    }
}
