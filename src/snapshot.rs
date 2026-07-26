//! Source snapshots used by changed-line review.

use crate::config::Config;
use crate::git::DiffScope;
use crate::{lang, walk};
use anyhow::Result;
use git2::{ObjectType, Repository, Tree, TreeWalkMode, TreeWalkResult};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ignore::overrides::{Override, OverrideBuilder};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Debug, Default)]
pub(crate) struct SourceSnapshot {
    sources: BTreeMap<PathBuf, String>,
    pub unreadable_files: usize,
    pub oversized_files: usize,
    pub oversized_bytes: u64,
    pub files_omitted_by_limit: usize,
    pub files_omitted_count_incomplete: bool,
    pub scan_truncated: bool,
    pub duration_limit_reached: bool,
}

impl SourceSnapshot {
    #[cfg(test)]
    pub fn from_sources(sources: impl IntoIterator<Item = (PathBuf, String)>) -> SourceSnapshot {
        SourceSnapshot {
            sources: sources.into_iter().collect(),
            unreadable_files: 0,
            oversized_files: 0,
            oversized_bytes: 0,
            files_omitted_by_limit: 0,
            files_omitted_count_incomplete: false,
            scan_truncated: false,
            duration_limit_reached: false,
        }
    }

    pub fn worktree(
        root: &Path,
        cfg: &Config,
        exclusions: &[PathBuf],
        deadline: Option<Instant>,
    ) -> Result<Self> {
        let discovered = walk::discover_with_exclusions_until(root, cfg, exclusions, deadline)?;
        let mut snapshot = Self {
            oversized_files: discovered.oversized_files,
            oversized_bytes: discovered.oversized_bytes,
            files_omitted_by_limit: discovered.files_omitted_by_limit,
            files_omitted_count_incomplete: discovered.files_omitted_count_incomplete,
            scan_truncated: discovered.scan_truncated,
            duration_limit_reached: discovered.duration_limit_reached,
            ..Self::default()
        };
        let file_count = discovered.files.len();
        for (index, file) in discovered.files.into_iter().enumerate() {
            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                snapshot.files_omitted_by_limit = snapshot
                    .files_omitted_by_limit
                    .saturating_add(file_count.saturating_sub(index));
                snapshot.scan_truncated = true;
                snapshot.duration_limit_reached = true;
                break;
            }
            if lang::detect(&file.report_path).is_none() {
                continue;
            }
            match walk::read_text_bounded(&file.absolute_path, cfg.max_file_bytes) {
                walk::BoundedText::Content(content) => {
                    snapshot.sources.insert(file.report_path, content);
                }
                walk::BoundedText::Oversized(bytes) => {
                    snapshot.oversized_files = snapshot.oversized_files.saturating_add(1);
                    snapshot.oversized_bytes = snapshot.oversized_bytes.saturating_add(bytes);
                    snapshot.scan_truncated = true;
                }
                walk::BoundedText::Unreadable => snapshot.unreadable_files += 1,
            }
        }
        Ok(snapshot)
    }

    pub fn base(
        root: &Path,
        cfg: &Config,
        scope: &DiffScope,
        base_tree_id: Option<&str>,
        exclusions: &[PathBuf],
        deadline: Option<Instant>,
    ) -> Result<Self> {
        let repo = Repository::discover(root)
            .map_err(|error| anyhow::anyhow!("deep review requires a git repository: {error}"))?;
        let tree = if let Some(id) = base_tree_id {
            repo.find_tree(git2::Oid::from_str(id)?)?
        } else {
            match scope {
                DiffScope::Since(reference) => repo.revparse_single(reference)?.peel_to_tree()?,
                DiffScope::Staged | DiffScope::Working => {
                    match repo.head().and_then(|head| head.peel_to_tree()) {
                        Ok(tree) => tree,
                        Err(error)
                            if matches!(
                                error.code(),
                                git2::ErrorCode::UnbornBranch | git2::ErrorCode::NotFound
                            ) =>
                        {
                            return Ok(Self::default());
                        }
                        Err(error) => return Err(error.into()),
                    }
                }
            }
        };
        Self::from_tree(
            &repo,
            &tree,
            &SnapshotFilter::new(root, cfg, exclusions)?,
            cfg,
            deadline,
        )
    }

    pub fn current(
        root: &Path,
        cfg: &Config,
        scope: &DiffScope,
        exclusions: &[PathBuf],
        deadline: Option<Instant>,
    ) -> Result<Self> {
        if matches!(scope, DiffScope::Staged) {
            Self::from_index(
                root,
                &SnapshotFilter::new(root, cfg, exclusions)?,
                cfg,
                deadline,
            )
        } else {
            Self::worktree(root, cfg, exclusions, deadline)
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Path, &str)> {
        self.sources
            .iter()
            .map(|(path, content)| (path.as_path(), content.as_str()))
    }

    fn from_tree(
        repo: &Repository,
        tree: &Tree<'_>,
        filter: &SnapshotFilter,
        cfg: &Config,
        deadline: Option<Instant>,
    ) -> Result<Self> {
        let mut snapshot = Self::default();
        let mut observed_files = 0usize;
        let mut accepted_bytes = 0u64;
        let mut stopped_at_limit = false;
        let walk_result = tree.walk(TreeWalkMode::PreOrder, |directory, entry| {
            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                snapshot.scan_truncated = true;
                snapshot.duration_limit_reached = true;
                snapshot.files_omitted_count_incomplete = true;
                stopped_at_limit = true;
                return TreeWalkResult::Abort;
            }
            if entry.kind() != Some(ObjectType::Blob) {
                return TreeWalkResult::Ok;
            }
            observed_files = observed_files.saturating_add(1);
            if observed_files > cfg.max_files {
                snapshot.files_omitted_by_limit = snapshot.files_omitted_by_limit.saturating_add(1);
                snapshot.files_omitted_count_incomplete = true;
                snapshot.scan_truncated = true;
                stopped_at_limit = true;
                return TreeWalkResult::Abort;
            }
            let name = match entry.name() {
                Ok(name) => name,
                Err(_) => {
                    snapshot.unreadable_files += 1;
                    return TreeWalkResult::Ok;
                }
            };
            let path = PathBuf::from(directory).join(name);
            if !filter.allows(&path) {
                return TreeWalkResult::Ok;
            }
            match repo.find_blob(entry.id()) {
                Ok(blob) if blob.size() as u64 > cfg.max_git_blob_bytes => {
                    snapshot.oversized_files = snapshot.oversized_files.saturating_add(1);
                    snapshot.oversized_bytes =
                        snapshot.oversized_bytes.saturating_add(blob.size() as u64);
                    snapshot.scan_truncated = true;
                }
                Ok(blob)
                    if accepted_bytes.saturating_add(blob.size() as u64) > cfg.max_total_bytes =>
                {
                    snapshot.files_omitted_by_limit =
                        snapshot.files_omitted_by_limit.saturating_add(1);
                    snapshot.scan_truncated = true;
                }
                Ok(blob) => {
                    accepted_bytes = accepted_bytes.saturating_add(blob.size() as u64);
                    match std::str::from_utf8(blob.content()) {
                        Ok(content) => {
                            snapshot.sources.insert(path, content.to_string());
                        }
                        Err(_) => snapshot.unreadable_files += 1,
                    }
                }
                Err(_) => snapshot.unreadable_files += 1,
            }
            TreeWalkResult::Ok
        });
        if let Err(error) = walk_result
            && !stopped_at_limit
        {
            return Err(error.into());
        }
        Ok(snapshot)
    }

    fn from_index(
        root: &Path,
        filter: &SnapshotFilter,
        cfg: &Config,
        deadline: Option<Instant>,
    ) -> Result<Self> {
        let repo = Repository::discover(root)
            .map_err(|error| anyhow::anyhow!("staged review requires a git repository: {error}"))?;
        let index = repo.index()?;
        let mut snapshot = Self::default();
        let mut observed_files = 0usize;
        let mut accepted_bytes = 0u64;
        let entry_count = index.len();
        for (position, entry) in index.iter().enumerate() {
            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                snapshot.files_omitted_by_limit = snapshot
                    .files_omitted_by_limit
                    .saturating_add(entry_count.saturating_sub(position));
                snapshot.scan_truncated = true;
                snapshot.duration_limit_reached = true;
                break;
            }
            observed_files = observed_files.saturating_add(1);
            if observed_files > cfg.max_files {
                snapshot.files_omitted_by_limit = snapshot
                    .files_omitted_by_limit
                    .saturating_add(entry_count.saturating_sub(position));
                snapshot.scan_truncated = true;
                break;
            }
            let Some(path) = git_path(&entry.path) else {
                snapshot.unreadable_files += 1;
                continue;
            };
            if !filter.allows(&path) {
                continue;
            }
            match repo.find_blob(entry.id) {
                Ok(blob) if blob.size() as u64 > cfg.max_git_blob_bytes => {
                    snapshot.oversized_files = snapshot.oversized_files.saturating_add(1);
                    snapshot.oversized_bytes =
                        snapshot.oversized_bytes.saturating_add(blob.size() as u64);
                    snapshot.scan_truncated = true;
                }
                Ok(blob)
                    if accepted_bytes.saturating_add(blob.size() as u64) > cfg.max_total_bytes =>
                {
                    snapshot.files_omitted_by_limit =
                        snapshot.files_omitted_by_limit.saturating_add(1);
                    snapshot.scan_truncated = true;
                }
                Ok(blob) => {
                    accepted_bytes = accepted_bytes.saturating_add(blob.size() as u64);
                    match std::str::from_utf8(blob.content()) {
                        Ok(content) => {
                            snapshot.sources.insert(path, content.to_string());
                        }
                        Err(_) => snapshot.unreadable_files += 1,
                    }
                }
                Err(_) => snapshot.unreadable_files += 1,
            }
        }
        Ok(snapshot)
    }
}

#[cfg(unix)]
fn git_path(bytes: &[u8]) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;

    Some(std::ffi::OsString::from_vec(bytes.to_vec()).into())
}

#[cfg(not(unix))]
fn git_path(bytes: &[u8]) -> Option<PathBuf> {
    String::from_utf8(bytes.to_vec()).ok().map(PathBuf::from)
}

struct SnapshotFilter {
    root: PathBuf,
    include_hidden: bool,
    exclude_lockfiles: bool,
    exclusions: HashSet<PathBuf>,
    overrides: Override,
    info_ignores: Gitignore,
    ignores: Vec<Gitignore>,
    global_ignores: Gitignore,
}

impl SnapshotFilter {
    fn new(root: &Path, cfg: &Config, exclusions: &[PathBuf]) -> Result<Self> {
        let exclusions = exclusions
            .iter()
            .map(|path| walk::exact_path_identity(path))
            .collect::<Result<HashSet<_>>>()?;

        let mut overrides = OverrideBuilder::new(root);
        for pattern in &cfg.extra_excludes {
            overrides.add(&format!("!{pattern}"))?;
        }
        let overrides = overrides.build()?;

        let mut ignore_files = collect_ignore_files(root, cfg.respect_gitignore);
        ignore_files.sort();
        let mut info_builder = GitignoreBuilder::new(root);
        if cfg.respect_gitignore {
            let info_exclude = root.join(".git/info/exclude");
            if let Ok(content) = std::fs::read_to_string(info_exclude) {
                for line in content.lines() {
                    let _ = info_builder.add_line(Some(root.join(".git/info/exclude")), line);
                }
            }
        }
        let info_ignores = info_builder.build()?;
        let ignores = ignore_files
            .into_iter()
            .map(|path| Gitignore::new(path).0)
            .collect();
        let global_ignores = if cfg.respect_gitignore {
            GitignoreBuilder::new(root).build_global().0
        } else {
            Gitignore::empty()
        };

        Ok(Self {
            root: root.to_path_buf(),
            include_hidden: cfg.include_hidden,
            exclude_lockfiles: cfg.exclude_lockfiles,
            exclusions,
            overrides,
            info_ignores,
            ignores,
            global_ignores,
        })
    }

    fn allows(&self, path: &Path) -> bool {
        if lang::detect(path).is_none()
            || (!self.include_hidden && has_hidden_component(path))
            || (self.exclude_lockfiles && walk::is_lockfile(path))
        {
            return false;
        }
        let absolute = self.root.join(path);
        let identity = absolute.canonicalize().unwrap_or_else(|_| absolute.clone());
        if self.exclusions.contains(&identity)
            || self.override_ignored(&absolute)
            || self.is_ignored(&absolute)
        {
            return false;
        }
        true
    }

    fn override_ignored(&self, absolute: &Path) -> bool {
        if self.overrides.matched(absolute, false).is_ignore() {
            return true;
        }
        let mut ancestor = absolute.parent();
        while let Some(path) = ancestor.filter(|path| path.starts_with(&self.root)) {
            if self.overrides.matched(path, true).is_ignore() {
                return true;
            }
            if path == self.root {
                break;
            }
            ancestor = path.parent();
        }
        false
    }

    fn is_ignored(&self, absolute: &Path) -> bool {
        let Ok(relative) = absolute.strip_prefix(&self.root) else {
            return true;
        };
        let mut candidate = self.root.clone();
        for component in relative.components() {
            candidate.push(component);
            let is_dir = candidate != absolute;
            let mut ignored = self.global_ignores.matched(&candidate, is_dir).is_ignore();
            for matcher in std::iter::once(&self.info_ignores).chain(self.ignores.iter()) {
                match matcher.matched(&candidate, is_dir) {
                    ignore::Match::Ignore(_) => ignored = true,
                    ignore::Match::Whitelist(_) => ignored = false,
                    ignore::Match::None => {}
                }
            }
            if ignored {
                return true;
            }
        }
        false
    }
}

fn collect_ignore_files(root: &Path, respect_gitignore: bool) -> Vec<PathBuf> {
    let mut builder = ignore::WalkBuilder::new(root);
    builder
        .hidden(false)
        .ignore(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .parents(false)
        .follow_links(false)
        .filter_entry(|entry| entry.file_name() != ".git");
    builder
        .build()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_some_and(|kind| kind.is_file()))
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?;
            ((name == ".reposcoutignore")
                || (respect_gitignore && matches!(name, ".gitignore" | ".ignore")))
            .then(|| entry.into_path())
        })
        .collect()
}

fn has_hidden_component(path: &Path) -> bool {
    path.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(|component| component.starts_with('.') && component != ".")
    })
}

#[cfg(test)]
mod tests {
    use super::{SnapshotFilter, SourceSnapshot};
    use crate::config::Config;
    use crate::git::DiffScope;
    use std::fs;
    use std::path::{Path, PathBuf};

    #[test]
    fn in_memory_adapter_preserves_stable_paths_and_content() {
        let snapshot = SourceSnapshot::from_sources([(
            PathBuf::from("src/lib.rs"),
            "fn example() {}\n".to_string(),
        )]);
        let sources = snapshot.iter().collect::<Vec<_>>();

        assert_eq!(sources, [(Path::new("src/lib.rs"), "fn example() {}\n")]);
    }

    #[test]
    fn ignored_parent_cannot_be_reincluded_by_a_child_rule() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("ignored")).unwrap();
        fs::write(
            dir.path().join(".gitignore"),
            "ignored/\n!ignored/keep.py\n",
        )
        .unwrap();
        fs::write(dir.path().join("ignored/keep.py"), "VALUE = 1\n").unwrap();

        let filter = SnapshotFilter::new(dir.path(), &Config::default(), &[]).unwrap();
        assert!(!filter.allows(Path::new("ignored/keep.py")));
    }

    #[test]
    fn unborn_head_produces_an_empty_base_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        git2::Repository::init(dir.path()).unwrap();
        fs::write(dir.path().join("new.py"), "VALUE = 1\n").unwrap();

        let snapshot = SourceSnapshot::base(
            dir.path(),
            &Config::default(),
            &DiffScope::Working,
            None,
            &[],
            None,
        )
        .unwrap();
        assert_eq!(snapshot.iter().count(), 0);
    }

    #[test]
    fn base_snapshot_skips_oversized_git_blobs() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        fs::write(dir.path().join("large.rs"), "x".repeat(128)).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("large.rs")).unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let signature = git2::Signature::now("RepoScout Test", "test@example.com").unwrap();
        repo.commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
            .unwrap();
        let cfg = Config {
            max_git_blob_bytes: 32,
            ..Config::default()
        };

        let snapshot =
            SourceSnapshot::base(dir.path(), &cfg, &DiffScope::Working, None, &[], None).unwrap();

        assert_eq!(snapshot.iter().count(), 0);
        assert_eq!(snapshot.oversized_files, 1);
        assert_eq!(snapshot.oversized_bytes, 128);
        assert!(snapshot.scan_truncated);
    }

    #[test]
    fn tree_snapshot_marks_file_limit_omissions_as_a_lower_bound() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
        fs::write(dir.path().join("b.rs"), "fn b() {}\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("a.rs")).unwrap();
        index.add_path(Path::new("b.rs")).unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let signature = git2::Signature::now("RepoScout Test", "test@example.com").unwrap();
        repo.commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
            .unwrap();
        let cfg = Config {
            max_files: 1,
            ..Config::default()
        };

        let snapshot =
            SourceSnapshot::base(dir.path(), &cfg, &DiffScope::Working, None, &[], None).unwrap();

        assert_eq!(snapshot.files_omitted_by_limit, 1);
        assert!(snapshot.files_omitted_count_incomplete);
        assert!(snapshot.scan_truncated);
    }

    #[test]
    fn index_snapshot_counts_all_remaining_entries_at_the_file_limit() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        for name in ["a.rs", "b.rs", "c.rs"] {
            fs::write(dir.path().join(name), format!("fn {}() {{}}\n", &name[..1])).unwrap();
        }
        let mut index = repo.index().unwrap();
        for name in ["a.rs", "b.rs", "c.rs"] {
            index.add_path(Path::new(name)).unwrap();
        }
        index.write().unwrap();
        let cfg = Config {
            max_files: 1,
            ..Config::default()
        };

        let snapshot =
            SourceSnapshot::current(dir.path(), &cfg, &DiffScope::Staged, &[], None).unwrap();

        assert_eq!(snapshot.files_omitted_by_limit, 2);
        assert!(!snapshot.files_omitted_count_incomplete);
        assert!(snapshot.scan_truncated);
    }
}
