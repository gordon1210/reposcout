use super::{blob_size_hint, load_blob_text};
use crate::fs_budget::ReadBudget;
use crate::git::DiffScope;
use crate::model::SourceRevision;
use anyhow::{Context, Result};
use git2::{Index, Oid, Repository};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Pinned Git inputs for bounded regular-file blob reads with distinct conflict, missing, binary and unreadable outcomes.
pub(crate) struct GitCapture {
    pub repo: Repository,
    pub index: Index,
    pub root: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GitCaptureFailure {
    Missing,
    Conflict,
    NotRegularFile,
    Oversized,
    BudgetExceeded,
    DeadlineExceeded,
    Binary,
    Unreadable,
}

impl GitCapture {
    pub(crate) fn open(target: &Path) -> Result<Self> {
        let repo =
            Repository::discover(target).context("snapshot source requires a Git repository")?;
        let root = repo
            .workdir()
            .context("snapshot source requires a Git worktree")?
            .canonicalize()?;
        let index = repo.index()?;
        Ok(Self { repo, index, root })
    }

    pub(crate) fn pin(&self, revision: &SourceRevision) -> Result<SourceRevision> {
        match revision {
            SourceRevision::Tree(reference) => Ok(SourceRevision::Tree(
                self.repo
                    .revparse_single(reference)?
                    .peel_to_tree()?
                    .id()
                    .to_string(),
            )),
            other => Ok(other.clone()),
        }
    }

    pub(crate) fn base(&self, scope: &DiffScope) -> Result<SourceRevision> {
        if let DiffScope::Since(reference) = scope {
            return self.pin(&SourceRevision::Tree(reference.clone()));
        }
        match self.repo.head().and_then(|head| head.peel_to_tree()) {
            Ok(tree) => Ok(SourceRevision::Tree(tree.id().to_string())),
            Err(error)
                if matches!(
                    error.code(),
                    git2::ErrorCode::UnbornBranch | git2::ErrorCode::NotFound
                ) =>
            {
                Ok(SourceRevision::Empty)
            }
            Err(error) => Err(error.into()),
        }
    }

    pub(crate) fn read(
        &self,
        revision: &SourceRevision,
        path: &Path,
        budget: &mut ReadBudget,
        max_blob_bytes: u64,
    ) -> Result<String, GitCaptureFailure> {
        if budget
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Err(GitCaptureFailure::DeadlineExceeded);
        }
        if budget.remaining_files == 0 || budget.remaining_total_bytes == 0 {
            return Err(GitCaptureFailure::BudgetExceeded);
        }
        let (oid, mode) = self.entry(revision, path)?;
        if !matches!(mode, 0o100_644 | 0o100_755) {
            return Err(GitCaptureFailure::NotRegularFile);
        }
        let size = blob_size_hint(&self.repo, oid).ok_or(GitCaptureFailure::Unreadable)?;
        let cap = budget.max_file_bytes.min(max_blob_bytes);
        if size > cap {
            return Err(GitCaptureFailure::Oversized);
        }
        if size > budget.remaining_total_bytes {
            return Err(GitCaptureFailure::BudgetExceeded);
        }
        budget.remaining_files = budget.remaining_files.saturating_sub(1);
        budget.remaining_total_bytes = budget.remaining_total_bytes.saturating_sub(size);
        match load_blob_text(&self.repo, oid, cap) {
            Ok(Some(content)) if !content.contains('\0') => Ok(content),
            Ok(Some(_)) | Err(super::BlobTextError::Binary) => Err(GitCaptureFailure::Binary),
            Ok(None) => Err(GitCaptureFailure::Oversized),
            Err(super::BlobTextError::Unreadable) => Err(GitCaptureFailure::Unreadable),
        }
    }

    fn entry(
        &self,
        revision: &SourceRevision,
        path: &Path,
    ) -> Result<(Oid, u32), GitCaptureFailure> {
        match revision {
            SourceRevision::Tree(id) => {
                let oid = Oid::from_str(id).map_err(|_| GitCaptureFailure::Unreadable)?;
                let tree = self
                    .repo
                    .find_tree(oid)
                    .map_err(|_| GitCaptureFailure::Unreadable)?;
                let entry = tree.get_path(path).map_err(|error| {
                    if error.code() == git2::ErrorCode::NotFound {
                        GitCaptureFailure::Missing
                    } else {
                        GitCaptureFailure::Unreadable
                    }
                })?;
                Ok((
                    entry.id(),
                    u32::try_from(entry.filemode_raw())
                        .map_err(|_| GitCaptureFailure::NotRegularFile)?,
                ))
            }
            SourceRevision::Index => {
                if (1..=3).any(|stage| self.index.get_path(path, stage).is_some()) {
                    return Err(GitCaptureFailure::Conflict);
                }
                let entry = self
                    .index
                    .get_path(path, 0)
                    .ok_or(GitCaptureFailure::Missing)?;
                Ok((entry.id, entry.mode))
            }
            SourceRevision::Empty => Err(GitCaptureFailure::Missing),
            SourceRevision::Worktree => Err(GitCaptureFailure::Unreadable),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn captured_index_remains_stable_after_index_and_worktree_edits() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        fs::write(dir.path().join("lib.rs"), "fn before() {}\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("lib.rs")).unwrap();
        index.write().unwrap();
        let capture = GitCapture::open(dir.path()).unwrap();
        fs::write(dir.path().join("lib.rs"), "fn after() {}\n").unwrap();
        index.add_path(Path::new("lib.rs")).unwrap();
        index.write().unwrap();
        let mut budget = ReadBudget::from_limits(1024, 1024, 4);
        let content = capture
            .read(
                &SourceRevision::Index,
                Path::new("lib.rs"),
                &mut budget,
                1024,
            )
            .unwrap();
        assert_eq!(content, "fn before() {}\n");
        let reloaded = GitCapture::open(dir.path()).unwrap();
        assert_eq!(
            reloaded
                .read(
                    &SourceRevision::Index,
                    Path::new("lib.rs"),
                    &mut budget,
                    1024
                )
                .unwrap(),
            "fn after() {}\n"
        );
    }

    #[test]
    fn git_capture_distinguishes_absent_binary_mode_and_limits() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        fs::write(dir.path().join("binary.rs"), b"\xff\x00").unwrap();
        fs::write(dir.path().join("regular.rs"), "fn value() {}\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("binary.rs")).unwrap();
        index.add_path(Path::new("regular.rs")).unwrap();
        index.write().unwrap();
        let capture = GitCapture::open(dir.path()).unwrap();
        let mut budget = ReadBudget::from_limits(1024, 1024, 8);
        assert_eq!(
            capture.read(
                &SourceRevision::Index,
                Path::new("missing.rs"),
                &mut budget,
                1024
            ),
            Err(GitCaptureFailure::Missing)
        );
        assert_eq!(
            capture.read(
                &SourceRevision::Index,
                Path::new("binary.rs"),
                &mut budget,
                1024
            ),
            Err(GitCaptureFailure::Binary)
        );
        assert_eq!(budget.remaining_total_bytes, 1022);
        assert_eq!(
            capture.read(
                &SourceRevision::Index,
                Path::new("regular.rs"),
                &mut budget,
                1
            ),
            Err(GitCaptureFailure::Oversized)
        );
        let mut entry = index.get_path(Path::new("regular.rs"), 0).unwrap();
        entry.mode = 0o120_000;
        index.add(&entry).unwrap();
        index.write().unwrap();
        let capture = GitCapture::open(dir.path()).unwrap();
        assert_eq!(
            capture.read(
                &SourceRevision::Index,
                Path::new("regular.rs"),
                &mut budget,
                1024
            ),
            Err(GitCaptureFailure::NotRegularFile)
        );
    }
}
