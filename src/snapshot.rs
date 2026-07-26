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

#[derive(Debug, Default)]
pub(crate) struct SourceSnapshot {
    sources: BTreeMap<PathBuf, String>,
    pub unreadable_files: usize,
}

impl SourceSnapshot {
    #[cfg(test)]
    pub fn from_sources(sources: impl IntoIterator<Item = (PathBuf, String)>) -> SourceSnapshot {
        SourceSnapshot {
            sources: sources.into_iter().collect(),
            unreadable_files: 0,
        }
    }

    pub fn worktree(root: &Path, cfg: &Config, exclusions: &[PathBuf]) -> Result<Self> {
        let discovered = walk::discover_with_exclusions(root, cfg, exclusions)?;
        let mut snapshot = Self::default();
        for file in discovered.files {
            if lang::detect(&file.report_path).is_none() {
                continue;
            }
            match std::fs::read_to_string(&file.absolute_path) {
                Ok(content) => {
                    snapshot.sources.insert(file.report_path, content);
                }
                Err(_) => snapshot.unreadable_files += 1,
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
        Self::from_tree(&repo, &tree, &SnapshotFilter::new(root, cfg, exclusions)?)
    }

    pub fn current(
        root: &Path,
        cfg: &Config,
        scope: &DiffScope,
        exclusions: &[PathBuf],
    ) -> Result<Self> {
        if matches!(scope, DiffScope::Staged) {
            Self::from_index(root, &SnapshotFilter::new(root, cfg, exclusions)?)
        } else {
            Self::worktree(root, cfg, exclusions)
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Path, &str)> {
        self.sources
            .iter()
            .map(|(path, content)| (path.as_path(), content.as_str()))
    }

    fn from_tree(repo: &Repository, tree: &Tree<'_>, filter: &SnapshotFilter) -> Result<Self> {
        let mut snapshot = Self::default();
        tree.walk(TreeWalkMode::PreOrder, |directory, entry| {
            if entry.kind() != Some(ObjectType::Blob) {
                return TreeWalkResult::Ok;
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
                Ok(blob) => match std::str::from_utf8(blob.content()) {
                    Ok(content) => {
                        snapshot.sources.insert(path, content.to_string());
                    }
                    Err(_) => snapshot.unreadable_files += 1,
                },
                Err(_) => snapshot.unreadable_files += 1,
            }
            TreeWalkResult::Ok
        })?;
        Ok(snapshot)
    }

    fn from_index(root: &Path, filter: &SnapshotFilter) -> Result<Self> {
        let repo = Repository::discover(root)
            .map_err(|error| anyhow::anyhow!("staged review requires a git repository: {error}"))?;
        let index = repo.index()?;
        let mut snapshot = Self::default();
        for entry in index.iter() {
            let Some(path) = git_path(&entry.path) else {
                snapshot.unreadable_files += 1;
                continue;
            };
            if !filter.allows(&path) {
                continue;
            }
            match repo.find_blob(entry.id) {
                Ok(blob) => match std::str::from_utf8(blob.content()) {
                    Ok(content) => {
                        snapshot.sources.insert(path, content.to_string());
                    }
                    Err(_) => snapshot.unreadable_files += 1,
                },
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
        )
        .unwrap();
        assert_eq!(snapshot.iter().count(), 0);
    }
}
