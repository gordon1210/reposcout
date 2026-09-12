use super::source::{
    self, SourceQueryOptions, SourceQueryTarget, SourceSelector, budget, selection,
};
use crate::config::Config;
use crate::context::definitions::{
    self, DefinitionPlanInput, DefinitionPlanLimits, DefinitionSeed, DefinitionSelector,
};
use crate::metrics::tokens::TokenCounter;
use crate::model::{
    DefinitionPlanCapability, DefinitionPlanOmission, DefinitionPlanReport,
    DefinitionPlanningFacts, SourceRevision,
};
use crate::report::Format;
use crate::scan::{self, ArtifactRequirements};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
/// Explicit snapshot targets, separate planning and rendered-output budgets, and optional source delivery.
pub struct DefinitionPlanQueryOptions {
    pub targets: Vec<SourceQueryTarget>,
    pub snapshot: SourceRevision,
    pub context_budget: usize,
    pub token_budget: usize,
    pub byte_budget: usize,
    pub max_files: usize,
    pub max_definitions: usize,
    pub include_source: bool,
    pub format: Format,
    pub pretty_json: bool,
}

/// A definition plan and its complete budget-checked rendering, including any explicitly requested source.
pub struct DefinitionPlanOutput {
    pub report: DefinitionPlanReport,
    pub rendered: String,
}

struct CapturedPlan {
    inputs: Vec<DefinitionPlanInput>,
    seeds: Vec<DefinitionSeed>,
    failures: Vec<DefinitionPlanOmission>,
    unavailable_files: usize,
    incomplete: bool,
}

/// Plan captured definitions and supported environment, optionally delivering selected source under one combined rendered-output budget.
///
/// # Errors
///
/// Returns an error for invalid options, non-Unix platforms, invalid roots or revisions, unrecoverable
/// discovery, capture or analysis failures, token initialization or serialization failures, or a
/// budget that cannot hold the minimal status envelope.
pub fn plan_definitions(
    target: &Path,
    cfg: &Config,
    exclusions: &[PathBuf],
    options: &DefinitionPlanQueryOptions,
) -> Result<DefinitionPlanOutput> {
    ensure!(cfg!(unix), "plan is available only on Unix platforms");
    validate(options)?;
    let root = target
        .canonicalize()
        .context("plan root cannot be resolved")?;
    ensure!(root.is_dir(), "plan root must be a directory");
    let alias = std::path::absolute(target)?;
    let mut query_cfg = super::declaration_query_config(cfg);
    query_cfg.enabled.tokens = true;
    let captured = if options.targets.is_empty() {
        discover(&root, &query_cfg, exclusions)?
    } else {
        explicit(
            &root,
            &alias,
            &source::query_config(&query_cfg),
            exclusions,
            options,
        )?
    };
    let mut report = definitions::plan(
        &captured.inputs,
        &captured.seeds,
        &DefinitionPlanLimits {
            token_budget: options.context_budget,
            byte_budget: options.byte_budget,
            max_files: options.max_files,
            max_definitions: options.max_definitions,
        },
    );
    if !options.targets.is_empty() && captured.seeds.is_empty() {
        report.selected.clear();
        report.selected_tokens = 0;
        report.selected_files = 0;
        report.candidate_definitions = 0;
        report.omitted_definitions = 0;
        report.omissions.clear();
        report.omitted_details = 0;
    }
    report.token_budget = options.token_budget;
    report.discovery_incomplete = captured.incomplete;
    report.input_files += captured.unavailable_files;
    report.unavailable_files += captured.unavailable_files;
    for failure in captured.failures {
        if failure.reason == "not-found" || failure.reason == "invalid-path" {
            report.unresolved_seeds += 1;
        } else {
            report.unavailable_seeds += 1;
        }
        if report.omissions.len() < 32 {
            report.omissions.push(failure);
        } else {
            report.omitted_details += 1;
        }
    }
    let counter = TokenCounter::new(&cfg.encoding)?;
    if report.encoding.is_empty() {
        report.encoding = counter.name().into();
    }
    if options.include_source {
        include_source(
            &root,
            &source::query_config(&query_cfg),
            exclusions,
            options,
            &counter,
            &mut report,
        )?;
    } else {
        project(&mut report, options, &counter)?;
    }
    let rendered = crate::report::plan::render(&report, options.format, options.pretty_json)?;
    ensure!(
        rendered.len() <= options.byte_budget && counter.count(&rendered) <= options.token_budget,
        "plan output exceeded the validated budget"
    );
    Ok(DefinitionPlanOutput { report, rendered })
}

fn validate(options: &DefinitionPlanQueryOptions) -> Result<()> {
    ensure!(
        (1..=65_536).contains(&options.context_budget),
        "plan context budget must be between 1 and 65536"
    );
    ensure!(
        (1..=32).contains(&options.max_files),
        "plan file limit must be between 1 and 32"
    );
    ensure!(
        (1..=32).contains(&options.max_definitions),
        "plan definition limit must be between 1 and 32"
    );
    ensure!(
        options.targets.len() <= 32,
        "plan accepts at most 32 explicit targets"
    );
    let output = source_options(Vec::new(), options);
    source::validate_output_options(&output)?;
    if options.targets.is_empty() {
        ensure!(
            options.snapshot == SourceRevision::Worktree,
            "index and tree plans require explicit file, symbol, or line targets"
        );
    }
    for target in &options.targets {
        let mut selected = target.clone();
        selected.snapshot = options.snapshot.clone();
        source::validate_options(&source_options(vec![selected], options))?;
    }
    Ok(())
}

fn source_options(
    targets: Vec<SourceQueryTarget>,
    options: &DefinitionPlanQueryOptions,
) -> SourceQueryOptions {
    SourceQueryOptions {
        targets,
        token_budget: options.token_budget,
        byte_budget: options.byte_budget,
        format: options.format,
        pretty_json: options.pretty_json,
    }
}

fn requirements() -> ArtifactRequirements {
    ArtifactRequirements {
        symbol_outlines: true,
        definition_plans: true,
        ..ArtifactRequirements::default()
    }
}

fn discover(root: &Path, cfg: &Config, exclusions: &[PathBuf]) -> Result<CapturedPlan> {
    let artifacts = scan::run_with_artifacts(root, cfg, exclusions, requirements())?;
    let prefix = root
        .strip_prefix(&artifacts.report.root)
        .context("plan target is outside the scan report root")?;
    let inputs = artifacts
        .definitions
        .into_iter()
        .map(|(path, definitions)| {
            let planning = artifacts
                .definition_plans
                .get(&path)
                .cloned()
                .unwrap_or_default();
            Ok(DefinitionPlanInput {
                path: path
                    .strip_prefix(prefix)
                    .context("discovered definition is outside the plan target")?
                    .to_path_buf(),
                snapshot: SourceRevision::Worktree,
                definitions,
                planning,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(CapturedPlan {
        inputs,
        seeds: Vec::new(),
        failures: Vec::new(),
        unavailable_files: artifacts.report.diagnostics.unreadable_files,
        incomplete: artifacts.report.diagnostics.scan_truncated
            || artifacts.report.diagnostics.walker_errors > 0,
    })
}

fn explicit(
    root: &Path,
    alias: &Path,
    cfg: &Config,
    exclusions: &[PathBuf],
    options: &DefinitionPlanQueryOptions,
) -> Result<CapturedPlan> {
    let paths = options
        .targets
        .iter()
        .map(|target| source::normalize_path(root, alias, &target.path))
        .collect::<Vec<_>>();
    let targets = options
        .targets
        .iter()
        .cloned()
        .map(|mut target| {
            target.snapshot = options.snapshot.clone();
            target
        })
        .collect::<Vec<_>>();
    let targets = source::with_file_expectations(&targets, &paths)?;
    let requested = paths.iter().flatten().cloned().collect::<Vec<_>>();
    let mut groups = scan::load_revision_sources_with_requirements(
        root,
        &[(options.snapshot.clone(), requested)],
        cfg,
        exclusions,
        requirements(),
    )?;
    let (revision, batch) = groups.pop().context("plan capture returned no snapshot")?;
    let mut inputs = BTreeMap::new();
    let mut seeds = Vec::new();
    let mut failures = Vec::new();
    let mut failed_files = std::collections::BTreeSet::new();
    for (target, path) in targets.iter().zip(&paths) {
        let mut failure = None;
        if let Some(path) = path {
            if let Some(problem) = batch.failures.get(path) {
                failure = Some(
                    serde_json::to_value(selection::failure_status(*problem))?
                        .as_str()
                        .unwrap_or("unavailable")
                        .to_string(),
                );
            } else if let Some(file) = batch.files.get(path) {
                let planning = file
                    .planning
                    .clone()
                    .unwrap_or_else(DefinitionPlanningFacts::default);
                if target
                    .expected_hash
                    .as_ref()
                    .is_some_and(|hash| !hash.eq_ignore_ascii_case(&planning.sha256))
                {
                    failure = Some("stale".into());
                } else {
                    inputs
                        .entry(path.clone())
                        .or_insert_with(|| DefinitionPlanInput {
                            path: path.clone(),
                            snapshot: revision.clone(),
                            definitions: file.definitions.clone(),
                            planning,
                        });
                    seeds.push(DefinitionSeed {
                        path: path.clone(),
                        snapshot: revision.clone(),
                        selector: match &target.selector {
                            SourceSelector::Symbol(name) => {
                                DefinitionSelector::Symbol(name.clone())
                            }
                            SourceSelector::Line(line) => DefinitionSelector::Line(*line),
                            SourceSelector::Outline => DefinitionSelector::File,
                        },
                    });
                }
            } else {
                failure = Some("unavailable".into());
            }
        } else {
            failure = Some("invalid-path".into());
        }
        if let Some(reason) = failure {
            failed_files.insert(path.clone().unwrap_or_else(|| target.path.clone()));
            failures.push(DefinitionPlanOmission {
                path: path.clone().unwrap_or_else(|| target.path.clone()),
                name: match &target.selector {
                    SourceSelector::Symbol(name) => Some(name.clone()),
                    SourceSelector::Line(line) => Some(format!("line:{line}")),
                    SourceSelector::Outline => None,
                },
                reason,
                explicit: true,
            });
        }
    }
    Ok(CapturedPlan {
        inputs: inputs.into_values().collect(),
        seeds,
        failures,
        unavailable_files: failed_files.len(),
        incomplete: false,
    })
}

fn include_source(
    root: &Path,
    cfg: &Config,
    exclusions: &[PathBuf],
    options: &DefinitionPlanQueryOptions,
    counter: &TokenCounter,
    report: &mut DefinitionPlanReport,
) -> Result<()> {
    let targets = report
        .selected
        .iter()
        .map(|item| {
            let file = report
                .files
                .iter()
                .find(|file| file.id == item.file)
                .context("planned definition has no file identity")?;
            Ok(SourceQueryTarget {
                path: file.path.clone(),
                selector: SourceSelector::Symbol(item.name.clone()),
                expected_hash: Some(file.sha256.clone()),
                snapshot: file.snapshot.clone(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let read_options = source_options(targets, options);
    let mut source_report = source::empty_report(root, &read_options, counter.name());
    report.source = Some(source_report.clone());
    project(report, options, counter)?;
    source_report = report
        .source
        .clone()
        .context("source envelope disappeared during plan projection")?;
    let mut requests = BTreeMap::<SourceRevision, Vec<PathBuf>>::new();
    for target in &read_options.targets {
        requests
            .entry(target.snapshot.clone())
            .or_default()
            .push(target.path.clone());
    }
    let requests = requests.into_iter().collect::<Vec<_>>();
    let batches = scan::load_revision_sources(root, &requests, cfg, exclusions)?;
    let mut descriptions = Vec::new();
    let mut offset = 0;
    for ((revision, batch), (_, paths)) in batches.iter().zip(&requests) {
        let mut files = source::describe_files(batch, paths);
        for file in files.values_mut() {
            file.id += offset;
            file.snapshot = revision.clone();
        }
        offset += files.len();
        descriptions.push(files);
    }
    for (index, target) in read_options.targets.iter().enumerate() {
        let group = requests
            .iter()
            .position(|(revision, _)| *revision == target.snapshot)
            .context("planned source snapshot was not captured")?;
        let mut outline_remaining = 0;
        let resolved = selection::resolve(
            index + 1,
            target,
            Some(&target.path),
            &batches[group].1,
            &descriptions[group],
            &mut outline_remaining,
        );
        budget::admit_with_fit(&mut source_report, resolved, &read_options, |candidate| {
            let mut combined = report.clone();
            combined.source = Some(candidate.clone());
            fits(&combined, options, counter)
        })?;
    }
    report.source = Some(source_report);
    Ok(())
}

fn fits(
    report: &DefinitionPlanReport,
    options: &DefinitionPlanQueryOptions,
    counter: &TokenCounter,
) -> Result<bool> {
    let rendered = crate::report::plan::render(report, options.format, options.pretty_json)?;
    Ok(rendered.len() <= options.byte_budget && counter.count(&rendered) <= options.token_budget)
}

fn project(
    report: &mut DefinitionPlanReport,
    options: &DefinitionPlanQueryOptions,
    counter: &TokenCounter,
) -> Result<()> {
    while !fits(report, options, counter)? {
        if report.omissions.pop().is_some() {
            report.omitted_details += 1;
            continue;
        }
        if report.selected.pop().is_some() {
            report.output_omitted += 1;
        } else if !report.files.is_empty() {
            report.output_omitted_files += report.files.len();
            report.files.clear();
        } else if let Some(source) = report
            .source
            .as_mut()
            .filter(|source| source.root.is_some())
        {
            source.root = None;
            source.root_omitted = true;
        } else {
            anyhow::bail!("plan budget cannot fit the status envelope");
        }
        let before = report.files.len();
        report
            .files
            .retain(|file| report.selected.iter().any(|item| item.file == file.id));
        report.output_omitted_files += before.saturating_sub(report.files.len());
    }
    Ok(())
}

#[must_use]
pub(super) fn capability() -> DefinitionPlanCapability {
    DefinitionPlanCapability {
        command: "plan".into(),
        available: cfg!(unix),
        strategy_version: 1,
        formats: ["table", "json", "markdown", "ndjson"]
            .map(str::to_string)
            .to_vec(),
        snapshots: ["worktree", "index", "git-tree"]
            .map(str::to_string)
            .to_vec(),
        selectors: ["symbol", "line", "file"].map(str::to_string).to_vec(),
        default_context_budget: 12_000,
        min_context_budget: 1,
        max_context_budget: 65_536,
        default_token_budget: 4_096,
        min_token_budget: 256,
        max_token_budget: 65_536,
        default_byte_budget: 65_536,
        min_byte_budget: 1_024,
        max_byte_budget: 1_048_576,
        default_files: 8,
        default_definitions: 16,
        max_files: 32,
        max_definitions: 32,
        max_costed_definitions_per_file: crate::metrics::planning::MAX_COSTED_DEFINITIONS,
        environment_forms: vec!["local-signature-type".into()],
        environment_languages: ["Rust", "TypeScript", "TSX"].map(str::to_string).to_vec(),
        max_environment_definitions: 8,
        source_flag: "--source".into(),
    }
}
