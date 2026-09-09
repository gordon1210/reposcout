use super::declaration_query_config;
use crate::config::Config;
use crate::lang::FirstClass;
use crate::metrics::tokens::TokenCounter;
use crate::model::{
    DefinitionStatus, SCHEMA_VERSION, SourceQueryCapability, SourceQueryFile, SourceQueryLanguage,
    SourceQueryReport,
};
use crate::report::Format;
use crate::scan;
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};

mod budget;
mod selection;

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

/// An explicit file-and-symbol, file-and-line, or body-free outline selection.
#[derive(Debug, Clone)]
pub struct SourceQueryTarget {
    pub path: PathBuf,
    pub selector: SourceSelector,
    pub expected_hash: Option<String>,
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

/// Resolve explicit worktree targets and render complete definitions or body-free outlines within the shared token and byte budgets.
///
/// # Errors
///
/// Returns an error for invalid query options or target roots, unrecoverable source-loading or analysis
/// failures, token-counter initialization or serialization failures, or a budget that cannot hold
/// the minimal status envelope.
pub fn read_source(
    target: &Path,
    cfg: &Config,
    exclusions: &[PathBuf],
    options: &SourceQueryOptions,
) -> Result<SourceQueryOutput> {
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
    let selected_paths = paths.iter().flatten().cloned().collect::<Vec<_>>();
    let mut query_cfg = declaration_query_config(cfg);
    query_cfg.max_file_bytes = query_cfg.max_file_bytes.min(MAX_INPUT_FILE_BYTES);
    query_cfg.max_total_bytes = query_cfg.max_total_bytes.min(MAX_INPUT_TOTAL_BYTES);
    query_cfg.max_files = query_cfg.max_files.min(MAX_TARGETS);
    let batch = scan::load_explicit_sources(&root, &selected_paths, &query_cfg, exclusions)?;
    let files = describe_files(&batch, &selected_paths);
    let counter = TokenCounter::new(&cfg.encoding)?;
    let mut report = empty_report(&batch.root, options, counter.name());
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
        let resolved = selection::resolve(
            index + 1,
            target,
            path.as_deref(),
            &batch,
            &files,
            &mut outline_remaining,
        );
        budget::admit(&mut report, resolved, options, &counter)?;
    }
    let rendered = crate::report::source::render(&report, options.format, options.pretty_json)?;
    ensure!(
        rendered.len() <= options.byte_budget && counter.count(&rendered) <= options.token_budget,
        "source output exceeded the validated budget"
    );
    Ok(SourceQueryOutput { report, rendered })
}

fn with_file_expectations(
    targets: &[SourceQueryTarget],
    paths: &[Option<PathBuf>],
) -> Result<Vec<SourceQueryTarget>> {
    let mut expectations = BTreeMap::<&Path, String>::new();
    for (target, path) in targets.iter().zip(paths) {
        if let (Some(hash), Some(path)) = (&target.expected_hash, path) {
            if let Some(previous) = expectations.get(path.as_path()) {
                ensure!(
                    previous.eq_ignore_ascii_case(hash),
                    "conflicting expected hashes for a selected file"
                );
            }
            expectations.insert(path.as_path(), hash.to_ascii_lowercase());
        }
    }
    Ok(targets
        .iter()
        .zip(paths)
        .map(|(target, path)| {
            let mut target = target.clone();
            if let Some(hash) = path
                .as_ref()
                .and_then(|path| expectations.get(path.as_path()))
            {
                target.expected_hash = Some(hash.clone());
            }
            target
        })
        .collect())
}

fn validate_options(options: &SourceQueryOptions) -> Result<()> {
    ensure!(
        (1..=MAX_TARGETS).contains(&options.targets.len()),
        "source query requires between 1 and 32 targets"
    );
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
    let outline = matches!(options.targets[0].selector, SourceSelector::Outline);
    for target in &options.targets {
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

fn describe_files(
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

fn empty_report(root: &Path, options: &SourceQueryOptions, encoding: &str) -> SourceQueryReport {
    SourceQueryReport {
        kind: "source_query".to_string(),
        schema_version: SCHEMA_VERSION.to_string(),
        root: root.to_str().map(|_| root.to_path_buf()),
        root_omitted: root.to_str().is_none(),
        encoding: encoding.to_string(),
        mode: if matches!(options.targets[0].selector, SourceSelector::Outline) {
            "outline"
        } else {
            "source"
        }
        .to_string(),
        token_budget: options.token_budget,
        byte_budget: options.byte_budget,
        requested_targets: options.targets.len(),
        omitted_targets: options.targets.len(),
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
        formats: ["table", "json", "markdown", "ndjson"]
            .map(str::to_string)
            .to_vec(),
        selectors: ["--symbol FILE SYMBOL", "--line FILE LINE", "--outline FILE"]
            .map(str::to_string)
            .to_vec(),
        snapshot: "worktree".to_string(),
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
