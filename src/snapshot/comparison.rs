use super::GitCapture;
use crate::model::ReviewComparison;
use anyhow::{Context, Result, ensure};
use git2::{Delta, DiffFindOptions, DiffOptions, ObjectType, TreeWalkMode, TreeWalkResult};
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Clone, Copy)]
pub(crate) struct ComparisonLimits {
    pub files: usize,
    pub file_bytes: u64,
    pub total_bytes: u64,
    pub deadline: Instant,
}

pub(crate) struct ComparisonCapture {
    pub git: GitCapture,
    pub identity: ReviewComparison,
    pub changes: Vec<ComparisonChange>,
    pub total_changes: usize,
}

pub(crate) struct ComparisonChange {
    pub status: String,
    pub base: Option<ComparisonFile>,
    pub head: Option<ComparisonFile>,
}

pub(crate) struct ComparisonFile {
    pub path: PathBuf,
    pub mode: u32,
    pub blob: String,
}

impl ComparisonCapture {
    pub(crate) fn open(
        target: &Path,
        base: &str,
        head: &str,
        merge_base: bool,
        limits: ComparisonLimits,
    ) -> Result<Self> {
        for reference in [base, head] {
            ensure!(
                !reference.is_empty() && reference.len() <= 1024,
                "review revisions must contain 1 to 1024 bytes"
            );
        }
        let target = target
            .canonicalize()
            .context("review target cannot be resolved")?;
        ensure!(target.is_dir(), "review-context target must be a directory");
        let git = GitCapture::open(&target)?;
        let prefix = target.strip_prefix(&git.root)?;
        let requested = git.repo.revparse_single(base)?.peel_to_commit()?.id();
        let head = git.repo.revparse_single(head)?.peel_to_commit()?;
        let base_id = comparison_base(&git.repo, requested, head.id(), merge_base)?;
        let base = git.repo.find_commit(base_id)?;
        let base_tree = base.tree()?;
        let head_tree = head.tree()?;
        let mut options = DiffOptions::new();
        options.include_typechange(true);
        let mut diff =
            git.repo
                .diff_tree_to_tree(Some(&base_tree), Some(&head_tree), Some(&mut options))?;
        let rename_detection_complete = rename_safe(&git.repo, &diff, &limits);
        if rename_detection_complete {
            diff.find_similar(Some(
                DiffFindOptions::new()
                    .renames(true)
                    .rename_limit(limits.files),
            ))?;
        }
        let mut changes = Vec::new();
        let mut total_changes = 0usize;
        for delta in diff.deltas() {
            ensure!(
                Instant::now() < limits.deadline,
                "review comparison exceeded the configured duration limit"
            );
            for path in [delta.old_file().path(), delta.new_file().path()]
                .into_iter()
                .flatten()
            {
                ensure!(
                    path.to_str().is_some(),
                    "review-context requires UTF-8 Git paths"
                );
            }
            if ![delta.old_file().path(), delta.new_file().path()]
                .into_iter()
                .flatten()
                .any(|path| path.starts_with(prefix))
            {
                continue;
            }
            total_changes = total_changes.saturating_add(1);
            if changes.len() >= limits.files {
                continue;
            }
            changes.push(ComparisonChange {
                status: status(delta.status()).into(),
                base: (delta.status() != Delta::Added)
                    .then(|| file(&delta.old_file()))
                    .flatten(),
                head: (delta.status() != Delta::Deleted)
                    .then(|| file(&delta.new_file()))
                    .flatten(),
            });
        }
        changes.sort_by(|a, b| {
            a.head
                .as_ref()
                .or(a.base.as_ref())
                .map(|file| &file.path)
                .cmp(&b.head.as_ref().or(b.base.as_ref()).map(|file| &file.path))
        });
        let identity = ReviewComparison {
            mode: if merge_base { "merge-base" } else { "direct" }.into(),
            requested_base_commit: requested.to_string(),
            base_commit: base.id().to_string(),
            head_commit: head.id().to_string(),
            base_tree: base_tree.id().to_string(),
            head_tree: head_tree.id().to_string(),
            rename_detection_complete,
        };
        drop(diff);
        drop(base_tree);
        drop(head_tree);
        drop(base);
        drop(head);
        Ok(Self {
            git,
            identity,
            changes,
            total_changes,
        })
    }
}

fn comparison_base(
    repo: &git2::Repository,
    requested: git2::Oid,
    head: git2::Oid,
    merge_base: bool,
) -> Result<git2::Oid> {
    if !merge_base {
        return Ok(requested);
    }
    let bases = repo.merge_bases(requested, head)?;
    ensure!(
        bases.len() == 1,
        "comparison requires one unambiguous merge base; supply a direct base instead"
    );
    Ok(bases[0])
}

fn rename_safe(repo: &git2::Repository, diff: &git2::Diff<'_>, limits: &ComparisonLimits) -> bool {
    if diff.deltas().len() > limits.files {
        return false;
    }
    let mut bytes = 0u64;
    for delta in diff
        .deltas()
        .filter(|delta| matches!(delta.status(), Delta::Added | Delta::Deleted))
    {
        if Instant::now() >= limits.deadline {
            return false;
        }
        let file = if delta.status() == Delta::Added {
            delta.new_file()
        } else {
            delta.old_file()
        };
        if !matches!(u32::from(file.mode()), 0o100_644 | 0o100_755) {
            continue;
        }
        let Some(size) = super::blob_size_hint(repo, file.id()) else {
            return false;
        };
        bytes = bytes.saturating_add(size);
        if size > limits.file_bytes || bytes > limits.total_bytes {
            return false;
        }
    }
    true
}

fn file(file: &git2::DiffFile<'_>) -> Option<ComparisonFile> {
    Some(ComparisonFile {
        path: file.path()?.to_path_buf(),
        mode: u32::from(file.mode()),
        blob: file.id().to_string(),
    })
}

fn status(delta: Delta) -> &'static str {
    match delta {
        Delta::Added => "added",
        Delta::Deleted => "deleted",
        Delta::Modified => "modified",
        Delta::Renamed => "renamed",
        Delta::Copied => "copied",
        Delta::Typechange => "typechange",
        Delta::Unreadable => "unreadable",
        Delta::Conflicted => "conflicted",
        Delta::Unmodified => "unmodified",
        Delta::Ignored => "ignored",
        Delta::Untracked => "untracked",
    }
}

pub(crate) struct TreeInventory {
    pub paths: Vec<PathBuf>,
    pub observed: usize,
    pub truncated: bool,
}

pub(crate) fn inventory(
    git: &GitCapture,
    tree: &str,
    limit: usize,
    deadline: Instant,
) -> Result<TreeInventory> {
    let tree = git.repo.find_tree(git2::Oid::from_str(tree)?)?;
    let mut result = TreeInventory {
        paths: Vec::new(),
        observed: 0,
        truncated: false,
    };
    let walked = tree.walk(TreeWalkMode::PreOrder, |directory, entry| {
        if Instant::now() >= deadline {
            result.truncated = true;
            return TreeWalkResult::Abort;
        }
        if entry.kind() == Some(ObjectType::Tree) {
            return TreeWalkResult::Ok;
        }
        result.observed += 1;
        if result.paths.len() >= limit {
            result.truncated = true;
            return TreeWalkResult::Abort;
        }
        if let Ok(name) = entry.name() {
            result.paths.push(PathBuf::from(directory).join(name));
        } else {
            result.truncated = true;
        }
        TreeWalkResult::Ok
    });
    if !result.truncated {
        walked?;
    }
    result.paths.sort();
    Ok(result)
}
