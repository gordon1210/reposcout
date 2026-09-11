use super::declaration_query_config;
use crate::config::Config;
use crate::lang::FirstClass;
use crate::metrics::tokens::TokenCounter;
use crate::model::{
    DefinitionStatus, SCHEMA_VERSION, SourceQueryCapability, SourceQueryFile, SourceQueryLanguage,
    SourceQueryReport, SourceRevision,
};
use crate::report::Format;
use crate::scan;
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};

pub(super) mod budget;
pub(super) mod selection;

const DEFAULT_TOKENS: usize = 4_096;
const MIN_TOKENS: usize = 256;
const MAX_TOKENS: usize = 65_536;
const DEFAULT_BYTES: usize = 65_536;
const MIN_BYTES: usize = 1_024;
const MAX_BYTES: usize = 1_048_576;
const MAX_TARGETS: usize = 32;
const MAX_CANDIDATES: usize = 8;
const MAX_OUTLINE_DECLARATIONS: usize = 100;
const MAX_INPUT_FILE_BYTES: u64 = 8 * 1_024 * 1_024;
const MAX_INPUT_TOTAL_BYTES: u64 = 32 * 1_024 * 1_024;

#[derive(Debug, Clone)]
pub enum SourceSelector {
    Symbol(String),
    Line(usize),
    Outline,
}

/// An explicit file-and-symbol, file-and-line, or body-free outline selection within the requested snapshot.
#[derive(Debug, Clone)]
pub struct SourceQueryTarget {
    pub path: PathBuf,
    pub selector: SourceSelector,
    pub expected_hash: Option<String>,
    pub snapshot: SourceRevision,
}

#[derive(Debug, Clone)]
pub struct SourceQueryOptions {
    pub targets: Vec<SourceQueryTarget>,
    pub token_budget: usize,
    pub byte_budget: usize,
    pub format: Format,
    pub pretty_json: bool,
}

/// A source-query result together with its complete, budget-checked rendered output.
pub struct SourceQueryOutput {
    pub report: SourceQueryReport,
    pub rendered: String,
}

/// Resolve explicit targets in the requested source snapshot and render complete definitions or body-free outlines within shared output budgets.
///
/// # Errors
///
/// Returns an error on non-Unix platforms before source I/O, or for invalid options, target roots or
/// unresolvable revisions, unrecoverable capture or analysis failures, token-counter initialization
/// or serialization failures, or a budget that cannot hold the minimal status envelope.
pub fn read_source(
    target: &Path,
    cfg: &Config,
    exclusions: &[PathBuf],
    options: &SourceQueryOptions,
) -> Result<SourceQueryOutput> {
    ensure!(cfg!(unix), "read is available only on Unix platforms");
    validate_options(options)?;
    let root = target
        .canonicalize()
        .context("source root cannot be resolved")?;
    ensure!(root.is_dir(), "source root must be a directory");
    let root_alias = std::path::absolute(target).context("source root cannot be made absolute")?;
    let paths = options
        .targets
        .iter()
        .map(|target| normalize_path(&root, &root_alias, &target.path))
        .collect::<Vec<_>>();
    let targets = with_file_expectations(&options.targets, &paths)?;
    let mut groups = BTreeMap::<SourceRevision, Vec<PathBuf>>::new();
    for (target, path) in targets.iter().zip(&paths) {
        if let Some(path) = path {
            groups
                .entry(target.snapshot.clone())
                .or_default()
                .push(path.clone());
        }
    }
    let requests = groups.into_iter().collect::<Vec<_>>();
    let query_cfg = query_config(cfg);
    let batches = scan::load_revision_sources(&root, &requests, &query_cfg, exclusions)?;
    let target_groups = targets
        .iter()
        .map(|target| {
            requests
                .iter()
                .position(|(revision, _)| *revision == target.snapshot)
        })
        .collect::<Vec<_>>();
    let mut canonical_targets = targets;
    for (target, group) in canonical_targets.iter_mut().zip(&target_groups) {
        if let Some(group) = group {
            target.snapshot = batches[*group].0.clone();
        }
    }
    let targets = with_file_expectations(&canonical_targets, &paths)?;
    let mut identities = BTreeMap::new();
    let files = batches
        .iter()
        .zip(&requests)
        .map(|((revision, batch), (_, paths))| {
            let mut files = describe_files(batch, paths);
            for file in files.values_mut() {
                let next_id = identities.len() + 1;
                file.id = *identities
                    .entry((revision.clone(), file.path.clone()))
                    .or_insert(next_id);
                file.snapshot = revision.clone();
            }
            files
        })
        .collect::<Vec<_>>();
    let counter = TokenCounter::new(&cfg.encoding)?;
    let mut report = empty_report(&root, options, counter.name());
    if !budget::fits(&report, options, &counter)? {
        report.root = None;
        report.root_omitted = true;
    }
    ensure!(
        budget::fits(&report, options, &counter)?,
        "source budget cannot fit the status envelope"
    );
    let mut outline_remaining = MAX_OUTLINE_DECLARATIONS;
    for (index, (target, path)) in targets.iter().zip(&paths).enumerate() {
        let group = target_groups[index];
        let resolved = if let Some(group) = group {
            selection::resolve(
                index + 1,
                target,
                path.as_deref(),
                &batches[group].1,
                &files[group],
                &mut outline_remaining,
            )
        } else {
            selection::ResolvedTarget {
                file: None,
                result: crate::model::SourceQueryResult {
                    status: crate::model::SourceQueryStatus::InvalidPath,
                    ..selection::empty_result(index + 1, None)
                },
                source: None,
            }
        };
        budget::admit(&mut report, resolved, options, &counter)?;
    }
    let rendered = crate::report::source::render(&report, options.format, options.pretty_json)?;
    ensure!(
        rendered.len() <= options.byte_budget && counter.count(&rendered) <= options.token_budget,
        "source output exceeded the validated budget"
    );
    Ok(SourceQueryOutput { report, rendered })
}

pub(super) fn query_config(cfg: &Config) -> Config {
    let mut query_cfg = declaration_query_config(cfg);
    query_cfg.max_file_bytes = query_cfg.max_file_bytes.min(MAX_INPUT_FILE_BYTES);
    query_cfg.max_total_bytes = query_cfg.max_total_bytes.min(MAX_INPUT_TOTAL_BYTES);
    query_cfg.max_files = query_cfg.max_files.min(MAX_TARGETS);
    query_cfg
}

#[cfg(all(test, not(unix)))]
mod platform_tests {
    use super::*;

    #[test]
    fn unsupported_platform_rejects_source_before_resolving_the_root() {
        let options = SourceQueryOptions {
            targets: vec![SourceQueryTarget {
                path: PathBuf::from("lib.rs"),
                selector: SourceSelector::Symbol("example".to_string()),
                expected_hash: None,
                snapshot: SourceRevision::Worktree,
            }],
            token_budget: DEFAULT_TOKENS,
            byte_budget: DEFAULT_BYTES,
            format: Format::Json,
            pretty_json: false,
        };
        assert!(!capability().available);
        let result = read_source(
            Path::new("missing-source-root"),
            &Config::default(),
            &[],
            &options,
        );
        assert!(
            matches!(result, Err(error) if error.to_string() == "read is available only on Unix platforms")
        );
    }
}

fn with_file_expectations(
    targets: &[SourceQueryTarget],
    paths: &[Option<PathBuf>],
) -> Result<Vec<SourceQueryTarget>> {
    let mut expectations = BTreeMap::<(SourceRevision, PathBuf), String>::new();
    for (target, path) in targets.iter().zip(paths) {
        if let (Some(hash), Some(path)) = (&target.expected_hash, path) {
            let key = (target.snapshot.clone(), path.clone());
            if let Some(previous) = expectations.get(&key) {
                ensure!(
                    previous.eq_ignore_ascii_case(hash),
                    "conflicting expected hashes for a selected file"
                );
            }
            expectations.insert(key, hash.to_ascii_lowercase());
        }
    }
    Ok(targets
        .iter()
        .zip(paths)
        .map(|(target, path)| {
            let mut target = target.clone();
            if let Some(hash) = path
                .as_ref()
                .and_then(|path| expectations.get(&(target.snapshot.clone(), path.clone())))
            {
                target.expected_hash = Some(hash.clone());
            }
            target
        })
        .collect())
}

pub(super) fn validate_options(options: &SourceQueryOptions) -> Result<()> {
    ensure!(
        (1..=MAX_TARGETS).contains(&options.targets.len()),
        "source query requires between 1 and 32 targets"
    );
    validate_output_options(options)?;
    validate_targets(options)
}

pub(super) fn validate_output_options(options: &SourceQueryOptions) -> Result<()> {
    ensure!(
        (MIN_TOKENS..=MAX_TOKENS).contains(&options.token_budget),
        "source token budget must be between 256 and 65536"
    );
    ensure!(
        (MIN_BYTES..=MAX_BYTES).contains(&options.byte_budget),
        "source byte budget must be between 1024 and 1048576"
    );
    ensure!(
        matches!(
            options.format,
            Format::Json | Format::Ndjson | Format::Table | Format::Markdown
        ),
        "read supports table, JSON, Markdown, or NDJSON output"
    );
    ensure!(
        !options.pretty_json || options.format == Format::Json,
        "pretty output requires JSON format"
    );
    Ok(())
}

fn validate_targets(options: &SourceQueryOptions) -> Result<()> {
    let outline = matches!(options.targets[0].selector, SourceSelector::Outline);
    for target in &options.targets {
        match &target.snapshot {
            SourceRevision::Tree(reference) => ensure!(
                !reference.is_empty() && reference.len() <= 1_024,
                "source revision must contain between 1 and 1024 bytes"
            ),
            SourceRevision::Empty => {
                anyhow::bail!("an empty base is not an explicit source revision")
            }
            SourceRevision::Worktree | SourceRevision::Index => {}
        }
        ensure!(
            matches!(target.selector, SourceSelector::Outline) == outline,
            "outline selections cannot be combined with source selections"
        );
        match &target.selector {
            SourceSelector::Symbol(name) => ensure!(
                !name.trim().is_empty() && name.len() <= 1_024,
                "source symbol must contain between 1 and 1024 bytes"
            ),
            SourceSelector::Line(line) => ensure!(*line > 0, "source line must be positive"),
            SourceSelector::Outline => {}
        }
        if let Some(hash) = &target.expected_hash {
            ensure!(
                hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()),
                "expected source hash must contain exactly 64 hexadecimal SHA-256 digits"
            );
        }
    }
    Ok(())
}

fn normalize_path(root: &Path, root_alias: &Path, path: &Path) -> Option<PathBuf> {
    let relative = if path.is_absolute() {
        path.strip_prefix(root)
            .or_else(|_| path.strip_prefix(root_alias))
            .ok()?
    } else {
        path
    };
    let mut normalized = PathBuf::new();
    for component in relative.components() {
        match component {
            Component::Normal(value) => normalized.push(value),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!normalized.as_os_str().is_empty()
        && normalized.to_str().is_some_and(|path| path.len() <= 4_096))
    .then_some(normalized)
}

pub(super) fn describe_files(
    batch: &scan::ExplicitSourceBatch,
    paths: &[PathBuf],
) -> BTreeMap<PathBuf, SourceQueryFile> {
    paths
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .enumerate()
        .map(|(index, path)| {
            let loaded = batch.files.get(path);
            let available = loaded.is_some_and(|file| {
                matches!(
                    file.definitions.status,
                    DefinitionStatus::Available | DefinitionStatus::ParseErrors
                )
            });
            (
                path.clone(),
                SourceQueryFile {
                    id: index + 1,
                    path: path.clone(),
                    snapshot: SourceRevision::Worktree,
                    status: batch
                        .failures
                        .get(path)
                        .map(|failure| selection::failure_status(*failure)),
                    language: loaded.map(|file| file.language.clone()),
                    sha256: loaded.map(|file| source_hash(&file.content)),
                    extraction: loaded.map(|file| file.definitions.status),
                    declarations: loaded
                        .filter(|_| available)
                        .map(|file| file.definitions.definitions.len()),
                    source_definitions: loaded.filter(|_| available).map(|file| {
                        file.definitions
                            .definitions
                            .iter()
                            .filter(|definition| definition.source_span.is_some())
                            .count()
                    }),
                },
            )
        })
        .collect()
}

fn source_hash(content: &str) -> String {
    Sha256::digest(content.as_bytes())
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            let _ = write!(output, "{byte:02x}");
            output
        })
}

pub(super) fn empty_report(
    root: &Path,
    options: &SourceQueryOptions,
    encoding: &str,
) -> SourceQueryReport {
    SourceQueryReport {
        kind: "source_query".to_string(),
        schema_version: SCHEMA_VERSION.to_string(),
        root: root.to_str().map(|_| root.to_path_buf()),
        root_omitted: root.to_str().is_none(),
        encoding: encoding.to_string(),
        mode: if options
            .targets
            .first()
            .is_some_and(|target| matches!(target.selector, SourceSelector::Outline))
        {
            "outline"
        } else {
            "source"
        }
        .to_string(),
        token_budget: options.token_budget,
        byte_budget: options.byte_budget,
        requested_targets: options.targets.len(),
        omitted_targets: options.targets.len(),
        change: None,
        files: Vec::new(),
        results: Vec::new(),
        sources: Vec::new(),
    }
}

pub(super) fn capability() -> SourceQueryCapability {
    let languages = [
        (FirstClass::Rust, "Rust"),
        (FirstClass::Python, "Python"),
        (FirstClass::JavaScript, "JavaScript"),
        (FirstClass::TypeScript, "TypeScript"),
        (FirstClass::Tsx, "TSX"),
        (FirstClass::Go, "Go"),
        (FirstClass::Php, "PHP"),
        (FirstClass::GdScript, "GDScript"),
        (FirstClass::GdShader, "Godot Shader"),
    ]
    .into_iter()
    .map(|(language, name)| SourceQueryLanguage {
        language: name.to_string(),
        kinds: crate::metrics::symbols::definition_kinds(language)
            .iter()
            .map(|kind| (*kind).to_string())
            .collect(),
    })
    .collect();
    SourceQueryCapability {
        command: "read".to_string(),
        available: cfg!(unix),
        platforms: vec!["unix".to_string()],
        formats: ["table", "json", "markdown", "ndjson"]
            .map(str::to_string)
            .to_vec(),
        selectors: ["--symbol FILE SYMBOL", "--line FILE LINE", "--outline FILE"]
            .map(str::to_string)
            .to_vec(),
        snapshot: "worktree".to_string(),
        snapshots: ["worktree", "index", "git-tree"]
            .map(str::to_string)
            .to_vec(),
        hash_algorithm: "sha256".to_string(),
        default_tokens: DEFAULT_TOKENS,
        min_tokens: MIN_TOKENS,
        max_tokens: MAX_TOKENS,
        default_bytes: DEFAULT_BYTES,
        min_bytes: MIN_BYTES,
        max_bytes: MAX_BYTES,
        max_targets: MAX_TARGETS,
        max_candidates: MAX_CANDIDATES,
        max_outline_declarations: MAX_OUTLINE_DECLARATIONS,
        max_input_file_bytes: MAX_INPUT_FILE_BYTES,
        max_input_total_bytes: MAX_INPUT_TOTAL_BYTES,
        languages,
    }
}
