//! Watch mutable Git inputs, including metadata outside a linked worktree.

use anyhow::{Context, Result};
use notify::{Event, EventKind, RecursiveMode, Watcher, event::ModifyKind};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Default)]
pub(super) struct GitWatchPaths {
    files: BTreeSet<PathBuf>,
    trees: BTreeSet<PathBuf>,
    directories: BTreeSet<PathBuf>,
}

impl GitWatchPaths {
    pub(super) fn discover(target: &Path) -> Self {
        let Ok(repo) = git2::Repository::discover(target) else {
            return Self::default();
        };
        let mut paths = Self::default();
        // A linked worktree owns HEAD/index; refs and packed-refs are shared.
        for directory in [repo.path(), repo.commondir()] {
            let Ok(directory) = directory.canonicalize() else {
                continue;
            };
            paths.directories.insert(directory.clone());
            for name in ["HEAD", "index", "packed-refs", "shallow"] {
                paths.files.insert(directory.join(name));
            }
            paths.trees.insert(directory.join("refs"));
            for name in ["info/exclude", "info/grafts"] {
                paths.files.insert(directory.join(name));
            }
            paths.directories.insert(directory.join("info"));
        }
        paths
    }

    pub(super) fn register(&self, watcher: &mut impl Watcher) -> Result<()> {
        for (paths, mode) in [
            (&self.directories, RecursiveMode::NonRecursive),
            (&self.trees, RecursiveMode::Recursive),
        ] {
            for path in paths.iter().filter(|path| path.is_dir()) {
                watcher
                    .watch(path, mode)
                    .with_context(|| format!("failed to watch Git metadata {}", path.display()))?;
            }
        }
        Ok(())
    }

    pub(super) fn requires_rescan(&self, event: &Event) -> bool {
        if !matches!(
            event.kind,
            EventKind::Any
                | EventKind::Create(_)
                | EventKind::Remove(_)
                | EventKind::Modify(ModifyKind::Any | ModifyKind::Data(_) | ModifyKind::Name(_))
        ) {
            return false;
        }
        event.paths.iter().any(|path| {
            path.extension().is_none_or(|extension| extension != "lock")
                && (self.files.contains(path)
                    || self.trees.iter().any(|tree| path.starts_with(tree)))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_inputs_refresh_without_watching_objects_or_lock_files() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let paths = GitWatchPaths::discover(dir.path());
        let git_dir = repo.path().canonicalize().unwrap();
        for name in [
            "HEAD",
            "index",
            "packed-refs",
            "refs/heads/main",
            "info/exclude",
            "info/grafts",
        ] {
            let event = Event::new(EventKind::Any).add_path(git_dir.join(name));
            assert!(paths.requires_rescan(&event), "{name}");
        }
        for name in [
            "objects/ab/012345",
            "logs/HEAD",
            "index.lock",
            "refs/heads/main.lock",
        ] {
            let event = Event::new(EventKind::Any).add_path(git_dir.join(name));
            assert!(!paths.requires_rescan(&event), "{name}");
        }
    }

    #[test]
    fn linked_worktree_uses_private_head_and_shared_refs() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path().join("main")).unwrap();
        let tree_id = repo.index().unwrap().write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        // Synthetic test history is created in libgit2 without touching user signing.
        let signature = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
        repo.commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
            .unwrap();
        let linked = dir.path().join("linked");
        repo.worktree("linked", &linked, None).unwrap();
        let paths = GitWatchPaths::discover(&linked);
        let linked_repo = git2::Repository::open(&linked).unwrap();
        assert!(
            paths
                .files
                .contains(&linked_repo.path().canonicalize().unwrap().join("HEAD"))
        );
        assert!(
            paths
                .trees
                .contains(&repo.path().canonicalize().unwrap().join("refs"))
        );
        assert!(!paths.trees.iter().any(|path| path.ends_with("objects")));
    }
}
