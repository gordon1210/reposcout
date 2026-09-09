use super::{
    ArtifactRequirements, Cache, Config, Path, PathBuf, Result, TokenCounter, cache, file_analysis,
    scan_exclusions, walk,
};
use crate::fs_budget::{self, ReadBudget, ReadOutcome};
use crate::model::DefinitionFacts;
use anyhow::Context;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Component;
use std::time::{Duration, Instant};

pub(crate) struct ExplicitSourceBatch {
    pub root: PathBuf,
    pub files: BTreeMap<PathBuf, ExplicitSourceFile>,
    pub failures: BTreeMap<PathBuf, ExplicitSourceFailure>,
}

pub(crate) struct ExplicitSourceFile {
    pub content: String,
    pub language: String,
    pub definitions: DefinitionFacts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExplicitSourceFailure {
    InvalidPath,
    Excluded,
    IgnoreError,
    Unsupported,
    Unreadable,
    NotRegularFile,
    Oversized,
    BudgetExceeded,
    DeadlineExceeded,
}

/// Capture each policy-eligible explicit file once under shared input limits, reusing the ordinary analysis profile and cache.
pub(crate) fn load_explicit_sources(
    root: &Path,
    paths: &[PathBuf],
    cfg: &Config,
    exclusions: &[PathBuf],
) -> Result<ExplicitSourceBatch> {
    let root = root
        .canonicalize()
        .context("source root cannot be resolved")?;
    anyhow::ensure!(root.is_dir(), "source root must be a directory");
    let started = Instant::now();
    let deadline = Some(
        started
            .checked_add(Duration::from_secs(cfg.max_scan_seconds))
            .unwrap_or(started),
    );
    let mut budget = ReadBudget {
        max_file_bytes: cfg.max_file_bytes,
        remaining_total_bytes: cfg.max_total_bytes,
        remaining_files: cfg.max_files,
        deadline,
    };
    let exclusions = scan_exclusions(cfg, exclusions)
        .iter()
        .map(|path| walk::exact_path_identity(path))
        .collect::<Result<BTreeSet<_>>>()?;
    let mut matcher = walk::build_path_matcher(&root, cfg)?;
    let cache = Cache::open(
        &root,
        cfg.use_cache,
        &cache::AnalysisProfile::from_config(cfg),
    );
    let health_policy = cfg.health_policy()?;
    let counter = cfg
        .enabled
        .tokens
        .then(|| TokenCounter::new(&cfg.encoding))
        .transpose()?;
    let mut batch = ExplicitSourceBatch {
        root,
        files: BTreeMap::new(),
        failures: BTreeMap::new(),
    };
    let mut ignore_error = false;
    for path in paths.iter().collect::<BTreeSet<_>>() {
        let failure = if ignore_error {
            Some(ExplicitSourceFailure::IgnoreError)
        } else {
            validate_target(&batch.root, path, &exclusions, &mut matcher)
        };
        ignore_error |= failure == Some(ExplicitSourceFailure::IgnoreError);
        if let Some(failure) = failure {
            batch.failures.insert(path.clone(), failure);
            continue;
        }
        if crate::lang::detect(path).is_none() {
            batch
                .failures
                .insert(path.clone(), ExplicitSourceFailure::Unsupported);
            continue;
        }
        let content = match fs_budget::read_text_under_root(&batch.root, path, &mut budget) {
            ReadOutcome::Content(content) => content,
            outcome => {
                batch.failures.insert(path.clone(), read_failure(&outcome));
                continue;
            }
        };
        let outcome = file_analysis::analyze_loaded_file(
            path,
            content,
            cfg,
            &health_policy,
            counter.as_ref(),
            &cache,
            ArtifactRequirements {
                symbol_outlines: true,
                graph_facts: false,
            },
        );
        match outcome {
            super::AnalysisOutcome::Analyzed(file) => {
                batch.files.insert(
                    path.clone(),
                    ExplicitSourceFile {
                        content: file.content,
                        language: file.report.language,
                        definitions: file.definitions.unwrap_or_default(),
                    },
                );
            }
            _ => {
                batch
                    .failures
                    .insert(path.clone(), ExplicitSourceFailure::Unsupported);
            }
        }
    }
    cache.save(false)?;
    Ok(batch)
}

fn validate_target(
    root: &Path,
    path: &Path,
    exclusions: &BTreeSet<PathBuf>,
    matcher: &mut ignore::IncrementalIgnore,
) -> Option<ExplicitSourceFailure> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Some(ExplicitSourceFailure::InvalidPath);
    }
    let mut candidate = root.to_path_buf();
    for component in path.components() {
        candidate.push(component);
        match std::fs::symlink_metadata(&candidate) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Some(ExplicitSourceFailure::NotRegularFile);
            }
            Ok(_) => {}
            Err(_) => return Some(ExplicitSourceFailure::Unreadable),
        }
    }
    if exclusions.contains(&candidate) {
        return Some(ExplicitSourceFailure::Excluded);
    }
    let (decision, errors) = matcher.matched_with_errors(path, false);
    if errors.is_some() {
        return Some(ExplicitSourceFailure::IgnoreError);
    }
    decision
        .is_ignore()
        .then_some(ExplicitSourceFailure::Excluded)
}

fn read_failure(outcome: &ReadOutcome) -> ExplicitSourceFailure {
    match outcome {
        ReadOutcome::Oversized(_) => ExplicitSourceFailure::Oversized,
        ReadOutcome::BudgetExceeded => ExplicitSourceFailure::BudgetExceeded,
        ReadOutcome::DeadlineExceeded => ExplicitSourceFailure::DeadlineExceeded,
        ReadOutcome::NotRegularFile => ExplicitSourceFailure::NotRegularFile,
        ReadOutcome::Unreadable | ReadOutcome::Content(_) => ExplicitSourceFailure::Unreadable,
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn config() -> Config {
        Config {
            use_cache: false,
            ..Config::default()
        }
    }

    #[test]
    fn explicit_source_reads_only_targets_and_reloads_edits() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("lib.rs"), "fn first() {}\n").unwrap();
        std::fs::write(dir.path().join("other.rs"), "fn unrelated() {}\n").unwrap();
        let targets = vec![PathBuf::from("lib.rs"), PathBuf::from("lib.rs")];
        let batch = load_explicit_sources(dir.path(), &targets, &config(), &[]).unwrap();
        assert_eq!(batch.files.len(), 1);
        assert_eq!(
            batch.files[&targets[0]].definitions.definitions[0]
                .symbol
                .name,
            "first"
        );
        assert_eq!(batch.files[&targets[0]].content, "fn first() {}\n");
        std::fs::write(dir.path().join("lib.rs"), "fn second() { let n = 2; }\n").unwrap();
        let batch = load_explicit_sources(dir.path(), &targets, &config(), &[]).unwrap();
        assert_eq!(
            batch.files[&targets[0]].definitions.definitions[0]
                .symbol
                .name,
            "second"
        );
        assert!(batch.files[&targets[0]].content.contains("let n = 2"));
    }

    #[test]
    fn explicit_source_honors_hidden_nested_ignores_and_exact_exclusions() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("nested")).unwrap();
        std::fs::write(dir.path().join("nested/.reposcoutignore"), "ignored.rs\n").unwrap();
        for path in [
            ".hidden.rs",
            "nested/ignored.rs",
            "output.rs",
            "output-extra.rs",
        ] {
            std::fs::write(dir.path().join(path), "fn value() {}\n").unwrap();
        }
        let paths = [
            ".hidden.rs",
            "nested/ignored.rs",
            "output.rs",
            "output-extra.rs",
        ]
        .map(PathBuf::from);
        let batch = load_explicit_sources(
            dir.path(),
            &paths,
            &config(),
            &[dir.path().join("output.rs")],
        )
        .unwrap();
        assert_eq!(batch.files.len(), 1);
        assert!(batch.files.contains_key(Path::new("output-extra.rs")));
        assert_eq!(batch.failures.len(), 3);
        assert!(
            batch
                .failures
                .values()
                .all(|value| *value == ExplicitSourceFailure::Excluded)
        );
    }

    #[cfg(unix)]
    #[test]
    fn explicit_source_rejects_symlinks_escape_and_malformed_ignore() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("value.rs"), "fn value() {}\n").unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("linked")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("value.rs"), dir.path().join("leaf.rs"))
            .unwrap();
        let paths = ["linked/value.rs", "leaf.rs", "../value.rs"].map(PathBuf::from);
        let batch = load_explicit_sources(dir.path(), &paths, &config(), &[]).unwrap();
        assert_eq!(
            batch.failures[Path::new("linked/value.rs")],
            ExplicitSourceFailure::NotRegularFile
        );
        assert_eq!(
            batch.failures[Path::new("leaf.rs")],
            ExplicitSourceFailure::NotRegularFile
        );
        assert_eq!(
            batch.failures[Path::new("../value.rs")],
            ExplicitSourceFailure::InvalidPath
        );
        std::fs::write(dir.path().join(".reposcoutignore"), "[z-a]\n").unwrap();
        std::fs::write(dir.path().join("ok.rs"), "fn ok() {}\n").unwrap();
        let batch =
            load_explicit_sources(dir.path(), &[PathBuf::from("ok.rs")], &config(), &[]).unwrap();
        assert_eq!(
            batch.failures[Path::new("ok.rs")],
            ExplicitSourceFailure::IgnoreError
        );
    }

    #[test]
    fn explicit_source_has_shared_byte_file_and_time_limits() {
        let dir = tempfile::tempdir().unwrap();
        for path in ["a.rs", "b.rs"] {
            std::fs::write(dir.path().join(path), "fn x() {}\n").unwrap();
        }
        let paths = [PathBuf::from("b.rs"), PathBuf::from("a.rs")];
        let mut cfg = config();
        cfg.max_total_bytes = 10;
        let batch = load_explicit_sources(dir.path(), &paths, &cfg, &[]).unwrap();
        assert!(batch.files.contains_key(Path::new("a.rs")));
        assert_eq!(
            batch.failures[Path::new("b.rs")],
            ExplicitSourceFailure::BudgetExceeded
        );
        cfg.max_total_bytes = 100;
        cfg.max_files = 1;
        let batch = load_explicit_sources(dir.path(), &paths, &cfg, &[]).unwrap();
        assert_eq!(batch.files.len(), 1);
        cfg.max_files = 2;
        cfg.max_file_bytes = 1;
        let batch = load_explicit_sources(dir.path(), &paths, &cfg, &[]).unwrap();
        assert!(
            batch
                .failures
                .values()
                .all(|failure| *failure == ExplicitSourceFailure::Oversized)
        );
        cfg.max_scan_seconds = 0;
        let batch = load_explicit_sources(dir.path(), &paths, &cfg, &[]).unwrap();
        assert!(
            batch
                .failures
                .values()
                .all(|failure| *failure == ExplicitSourceFailure::DeadlineExceeded)
        );
    }
}
