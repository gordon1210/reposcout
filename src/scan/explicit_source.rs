use super::{
    ArtifactRequirements, Cache, Config, Path, PathBuf, Result, TokenCounter, cache, file_analysis,
    scan_exclusions, walk,
};
use crate::fs_budget::{self, ReadBudget, ReadOutcome};
use crate::model::{DefinitionFacts, SourceRevision};
use crate::snapshot::{GitCapture, GitCaptureFailure};
use anyhow::Context;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Component;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub(crate) struct ExplicitSourceBatch {
    pub root: PathBuf,
    pub files: BTreeMap<PathBuf, Arc<ExplicitSourceFile>>,
    pub failures: BTreeMap<PathBuf, ExplicitSourceFailure>,
}

#[derive(Clone)]
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
    Missing,
    Binary,
    Conflict,
}

/// Capture each policy-eligible explicit file once under shared input limits, reusing the ordinary analysis profile and cache.
pub(crate) fn load_explicit_sources(
    root: &Path,
    paths: &[PathBuf],
    cfg: &Config,
    exclusions: &[PathBuf],
) -> Result<ExplicitSourceBatch> {
    let mut capture = CaptureSession::new(root, cfg, exclusions, None)?;
    let batch = capture.batch(&SourceRevision::Worktree, paths);
    capture.cache.save(false)?;
    Ok(batch)
}

/// Capture requested worktree, index or tree files with pinned revisions, one captured index and shared input and cache policy.
pub(crate) fn load_revision_sources(
    root: &Path,
    requests: &[(SourceRevision, Vec<PathBuf>)],
    cfg: &Config,
    exclusions: &[PathBuf],
) -> Result<Vec<(SourceRevision, ExplicitSourceBatch)>> {
    if let [(SourceRevision::Worktree, paths)] = requests {
        return Ok(vec![(
            SourceRevision::Worktree,
            load_explicit_sources(root, paths, cfg, exclusions)?,
        )]);
    }
    let git = requests
        .iter()
        .any(|(revision, _)| !matches!(revision, SourceRevision::Worktree))
        .then(|| GitCapture::open(root))
        .transpose()?;
    let mut capture = CaptureSession::new(root, cfg, exclusions, git)?;
    let mut pinned: BTreeMap<SourceRevision, SourceRevision> = BTreeMap::new();
    let mut batches = Vec::new();
    for (revision, paths) in requests {
        let selected = if let Some(selected) = pinned.get(revision) {
            selected.clone()
        } else {
            let selected = capture
                .git
                .as_ref()
                .map_or_else(|| Ok(revision.clone()), |git| git.pin(revision))?;
            pinned.insert(revision.clone(), selected.clone());
            selected
        };
        let batch = capture.batch(&selected, paths);
        batches.push((selected, batch));
    }
    capture.cache.save(false)?;
    Ok(batches)
}

/// A Git change candidate with separate old/new target-scope flags; an out-of-scope side is not an absent file.
pub(crate) struct CapturedChangedFile {
    pub change: crate::model::ReviewChangedFile,
    pub old_outside_scope: bool,
    pub new_outside_scope: bool,
}

/// Captured old/new file sides with pinned revision identity and explicit changed-file admission counts.
pub(crate) struct CapturedSourceChanges {
    pub base_revision: SourceRevision,
    pub current_revision: SourceRevision,
    pub changed_files: Vec<CapturedChangedFile>,
    pub old: ExplicitSourceBatch,
    pub new: ExplicitSourceBatch,
    pub total_changed_files: usize,
    pub omitted_changed_files: usize,
}

/// Capture candidate old/new file sides from pinned Git inputs under the shared pair, file and byte limits.
pub(crate) fn capture_changed_sources(
    root: &Path,
    cfg: &Config,
    exclusions: &[PathBuf],
    scope: &crate::git::DiffScope,
) -> Result<CapturedSourceChanges> {
    let target = root
        .canonicalize()
        .context("source target cannot be resolved")?;
    let selected_file = target.is_file().then(|| target.clone());
    let root = if selected_file.is_some() {
        target
            .parent()
            .context("source file has no parent")?
            .to_path_buf()
    } else {
        target
    };
    let git = GitCapture::open(&root)?;
    let base_revision = git.base(scope)?;
    let current_revision = if matches!(scope, crate::git::DiffScope::Staged) {
        SourceRevision::Index
    } else {
        SourceRevision::Worktree
    };
    let tree = match &base_revision {
        SourceRevision::Tree(id) => Some(git.repo.find_tree(git2::Oid::from_str(id)?)?),
        _ => None,
    };
    let candidates =
        crate::git::candidate_changes(&git.repo, tree.as_ref(), Some(&git.index), scope)?;
    let prefix = root
        .strip_prefix(&git.root)
        .context("source target is outside the repository")?;
    let mut changed_files = Vec::new();
    for mut change in candidates {
        let old_inside = change
            .old_path
            .as_ref()
            .filter(|path| {
                selected_file
                    .as_ref()
                    .is_none_or(|selected| git.root.join(path) == *selected)
            })
            .and_then(|path| path.strip_prefix(prefix).ok())
            .map(Path::to_path_buf);
        let new_inside = change
            .path
            .as_ref()
            .filter(|path| {
                selected_file
                    .as_ref()
                    .is_none_or(|selected| git.root.join(path) == *selected)
            })
            .and_then(|path| path.strip_prefix(prefix).ok())
            .map(Path::to_path_buf);
        if old_inside.is_none() && new_inside.is_none() {
            continue;
        }
        let old_outside_scope = change.old_path.is_some() && old_inside.is_none();
        let new_outside_scope = change.path.is_some() && new_inside.is_none();
        change.old_path = old_inside;
        change.path = new_inside;
        changed_files.push(CapturedChangedFile {
            change,
            old_outside_scope,
            new_outside_scope,
        });
    }
    drop(tree);
    let total_changed_files = changed_files.len();
    let pair_limit = cfg.max_files.min(32);
    changed_files.truncate(pair_limit);
    let omitted_changed_files = total_changed_files.saturating_sub(changed_files.len());
    let old_paths = changed_files
        .iter()
        .filter_map(|file| file.change.old_path.clone())
        .collect::<Vec<_>>();
    let new_paths = changed_files
        .iter()
        .filter_map(|file| file.change.path.clone())
        .collect::<Vec<_>>();
    let mut capture_cfg = cfg.clone();
    capture_cfg.max_files = cfg.max_files.saturating_mul(2).min(64);
    capture_cfg.max_file_bytes = cfg
        .max_file_bytes
        .min(cfg.max_git_blob_bytes)
        .min(8 * 1024 * 1024);
    capture_cfg.max_total_bytes = cfg.max_total_bytes.min(32 * 1024 * 1024);
    let mut capture = CaptureSession::new(&root, &capture_cfg, exclusions, Some(git))?;
    let old = capture.batch(&base_revision, &old_paths);
    let new = capture.batch(&current_revision, &new_paths);
    capture.cache.save(false)?;
    Ok(CapturedSourceChanges {
        base_revision,
        current_revision,
        changed_files,
        old,
        new,
        total_changed_files,
        omitted_changed_files,
    })
}

struct CaptureSession<'a> {
    root: PathBuf,
    cfg: &'a Config,
    budget: ReadBudget,
    exclusions: BTreeSet<PathBuf>,
    matcher: ignore::IncrementalIgnore,
    ignore_error: bool,
    cache: Cache,
    health_policy: crate::config::HealthPolicy,
    counter: Option<TokenCounter>,
    git: Option<GitCapture>,
    captured: BTreeMap<
        (SourceRevision, PathBuf),
        std::result::Result<Arc<ExplicitSourceFile>, ExplicitSourceFailure>,
    >,
}

impl<'a> CaptureSession<'a> {
    fn new(
        root: &Path,
        cfg: &'a Config,
        exclusions: &[PathBuf],
        git: Option<GitCapture>,
    ) -> Result<Self> {
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
        Ok(Self {
            budget: ReadBudget {
                max_file_bytes: cfg.max_file_bytes,
                remaining_total_bytes: cfg.max_total_bytes,
                remaining_files: cfg.max_files,
                deadline,
            },
            exclusions: scan_exclusions(cfg, exclusions)
                .iter()
                .map(|path| walk::exact_path_identity(path))
                .collect::<Result<_>>()?,
            matcher: walk::build_path_matcher(&root, cfg)?,
            cache: Cache::open(
                &root,
                cfg.use_cache,
                &cache::AnalysisProfile::from_config(cfg),
            ),
            health_policy: cfg.health_policy()?,
            counter: cfg
                .enabled
                .tokens
                .then(|| TokenCounter::new(&cfg.encoding))
                .transpose()?,
            root,
            cfg,
            git,
            ignore_error: false,
            captured: BTreeMap::new(),
        })
    }

    fn batch(&mut self, revision: &SourceRevision, paths: &[PathBuf]) -> ExplicitSourceBatch {
        let mut batch = ExplicitSourceBatch {
            root: self.root.clone(),
            files: BTreeMap::new(),
            failures: BTreeMap::new(),
        };
        for path in paths.iter().collect::<BTreeSet<_>>() {
            let key = (revision.clone(), path.clone());
            let outcome = if let Some(outcome) = self.captured.get(&key) {
                outcome.clone()
            } else {
                let outcome = self.file(revision, path);
                self.captured.insert(key, outcome.clone());
                outcome
            };
            match outcome {
                Ok(file) => {
                    batch.files.insert(path.clone(), file);
                }
                Err(failure) => {
                    batch.failures.insert(path.clone(), failure);
                }
            }
        }
        batch
    }

    fn file(
        &mut self,
        revision: &SourceRevision,
        path: &Path,
    ) -> std::result::Result<Arc<ExplicitSourceFile>, ExplicitSourceFailure> {
        if self.ignore_error {
            return Err(ExplicitSourceFailure::IgnoreError);
        }
        let failure = validate_target(
            &self.root,
            path,
            &self.exclusions,
            &mut self.matcher,
            matches!(revision, SourceRevision::Worktree),
        );
        self.ignore_error |= failure == Some(ExplicitSourceFailure::IgnoreError);
        if let Some(failure) = failure {
            return Err(failure);
        }
        if crate::lang::detect(path).is_none() {
            return Err(ExplicitSourceFailure::Unsupported);
        }
        let content = if matches!(revision, SourceRevision::Worktree) {
            match fs_budget::read_text_under_root(&self.root, path, &mut self.budget) {
                ReadOutcome::Content(content) => content,
                outcome => return Err(read_failure(&outcome)),
            }
        } else {
            let git = self.git.as_ref().ok_or(ExplicitSourceFailure::Unreadable)?;
            let prefix = self
                .root
                .strip_prefix(&git.root)
                .map_err(|_| ExplicitSourceFailure::InvalidPath)?;
            git.read(
                revision,
                &prefix.join(path),
                &mut self.budget,
                self.cfg.max_git_blob_bytes,
            )
            .map_err(git_failure)?
        };
        if content.contains('\0') {
            return Err(ExplicitSourceFailure::Binary);
        }
        match file_analysis::analyze_loaded_file(
            path,
            content,
            self.cfg,
            &self.health_policy,
            self.counter.as_ref(),
            &self.cache,
            ArtifactRequirements {
                symbol_outlines: true,
                graph_facts: false,
            },
        ) {
            super::AnalysisOutcome::Analyzed(file) => Ok(Arc::new(ExplicitSourceFile {
                content: file.content,
                language: file.report.language,
                definitions: file.definitions.unwrap_or_default(),
            })),
            _ => Err(ExplicitSourceFailure::Unsupported),
        }
    }
}

fn validate_target(
    root: &Path,
    path: &Path,
    exclusions: &BTreeSet<PathBuf>,
    matcher: &mut ignore::IncrementalIgnore,
    worktree: bool,
) -> Option<ExplicitSourceFailure> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Some(ExplicitSourceFailure::InvalidPath);
    }
    let candidate = root.join(path);
    let mut prefix = root.to_path_buf();
    for component in path.components() {
        prefix.push(component);
        if !worktree && prefix == candidate {
            break;
        }
        match std::fs::symlink_metadata(&prefix) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Some(ExplicitSourceFailure::NotRegularFile);
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if worktree {
                    return Some(ExplicitSourceFailure::Missing);
                }
                break;
            }
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

fn git_failure(failure: GitCaptureFailure) -> ExplicitSourceFailure {
    match failure {
        GitCaptureFailure::Missing => ExplicitSourceFailure::Missing,
        GitCaptureFailure::Conflict => ExplicitSourceFailure::Conflict,
        GitCaptureFailure::NotRegularFile => ExplicitSourceFailure::NotRegularFile,
        GitCaptureFailure::Oversized => ExplicitSourceFailure::Oversized,
        GitCaptureFailure::BudgetExceeded => ExplicitSourceFailure::BudgetExceeded,
        GitCaptureFailure::DeadlineExceeded => ExplicitSourceFailure::DeadlineExceeded,
        GitCaptureFailure::Binary => ExplicitSourceFailure::Binary,
        GitCaptureFailure::Unreadable => ExplicitSourceFailure::Unreadable,
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
    fn revision_groups_share_budgets_and_preserve_index_vs_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        std::fs::write(dir.path().join("lib.rs"), "fn staged() {}\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("lib.rs")).unwrap();
        index.write().unwrap();
        std::fs::write(dir.path().join("lib.rs"), "fn working() {}\n").unwrap();
        let requests = vec![
            (SourceRevision::Index, vec![PathBuf::from("lib.rs")]),
            (SourceRevision::Index, vec![PathBuf::from("lib.rs")]),
            (SourceRevision::Worktree, vec![PathBuf::from("lib.rs")]),
        ];
        let mut cfg = config();
        cfg.max_files = 2;
        let groups = load_revision_sources(dir.path(), &requests, &cfg, &[]).unwrap();
        assert_eq!(
            groups[0].1.files[Path::new("lib.rs")].content,
            "fn staged() {}\n"
        );
        assert_eq!(
            groups[1].1.files[Path::new("lib.rs")].content,
            "fn staged() {}\n"
        );
        assert_eq!(
            groups[2].1.files[Path::new("lib.rs")].content,
            "fn working() {}\n"
        );
        cfg.max_total_bytes = 15;
        let groups = load_revision_sources(dir.path(), &requests, &cfg, &[]).unwrap();
        assert!(groups[0].1.files.contains_key(Path::new("lib.rs")));
        assert_eq!(
            groups[2].1.failures[Path::new("lib.rs")],
            ExplicitSourceFailure::BudgetExceeded
        );
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
