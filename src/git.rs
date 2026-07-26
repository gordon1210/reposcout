//! Git churn / hotspot analysis.
//!
//! ## Contract (frozen)
//! `collect(root, files, max_commits) -> HashMap<PathBuf, Churn>`
//! Walk history from HEAD (bounded by `max_commits`, 0 = unlimited) and, for
//! each repo-relative path in `files`, report commit count, distinct author
//! count, and first/last commit dates (RFC3339). Paths not touched in history
//! are simply absent from the map.
//!
//! NOTE: Implementation provided by the churn analyzer (git2 revwalk plus a
//! native Git tree-diff stream with a libgit2 fallback). Baseline returns an
//! empty map.

mod churn_cache;
mod native_churn;

use crate::model::{Churn, LineRange, ReviewChangedFile};
use anyhow::Result;
use chrono::DateTime;
use churn_cache::{CachedDelta, ChurnCache, CommitEvent, DeltaKind, ViewIdentity};
use git2::{
    Delta, Diff, DiffDelta, DiffFindOptions, DiffFormat, DiffLineType, DiffOptions, Repository,
    Sort,
};
use native_churn::NativeGit;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use xxhash_rust::xxh3::xxh3_64;

pub(crate) fn churn_cache_directory(root: &Path) -> Option<PathBuf> {
    churn_cache::cache_directory(root)
}

pub fn collect(root: &Path, files: &[PathBuf], max_commits: usize) -> HashMap<PathBuf, Churn> {
    let mut cache = ChurnCache::for_repo(root, false);
    collect_impl(root, files, max_commits, &mut cache).0
}

/// Collect churn while reusing immutable commit events and exact result views
/// from the OS cache. The frozen [`collect`] adapter remains uncached for
/// library callers and deterministic tests.
pub fn collect_with_cache(
    root: &Path,
    files: &[PathBuf],
    max_commits: usize,
    use_cache: bool,
) -> HashMap<PathBuf, Churn> {
    let mut cache = ChurnCache::for_repo(root, use_cache);
    collect_impl(root, files, max_commits, &mut cache).0
}

#[derive(Default)]
struct Acc {
    commits: usize,
    authors: HashSet<String>,
    first: Option<i64>,
    last: Option<i64>,
}

#[derive(Debug, Default)]
struct CollectStats {
    tree_diffs: usize,
    rename_probes: usize,
    native_batches: usize,
    native_events: usize,
    native_fallbacks: usize,
    event_hits: usize,
    view_hits: usize,
}

fn collect_impl(
    root: &Path,
    files: &[PathBuf],
    max_commits: usize,
    cache: &mut ChurnCache,
) -> (HashMap<PathBuf, Churn>, CollectStats) {
    collect_impl_with_native(root, files, max_commits, cache, &NativeGit::default())
}

fn collect_impl_with_native(
    root: &Path,
    files: &[PathBuf],
    max_commits: usize,
    cache: &mut ChurnCache,
    native_git: &NativeGit,
) -> (HashMap<PathBuf, Churn>, CollectStats) {
    let mut stats = CollectStats::default();
    let repo = match Repository::discover(root) {
        Ok(repo) => repo,
        Err(_) => return (HashMap::new(), stats),
    };

    let wanted: HashSet<PathBuf> = files.iter().cloned().collect();
    if wanted.is_empty() {
        return (HashMap::new(), stats);
    }

    let head = match repo.head().and_then(|head| head.peel_to_commit()) {
        Ok(head) => head,
        Err(_) => return (HashMap::new(), stats),
    };
    let history_state = history_state(&repo);
    let view_identity = ViewIdentity::new(
        head.id().to_string(),
        history_state.clone(),
        max_commits,
        wanted.iter().cloned(),
    );
    if let Some(churn) = cache.get_view(&view_identity) {
        stats.view_hits += 1;
        cache.save();
        return (churn, stats);
    }
    cache.load_events(&history_state);

    let mut acc: HashMap<PathBuf, Acc> = HashMap::new();
    let mut aliases: HashMap<PathBuf, HashSet<PathBuf>> = wanted
        .iter()
        .cloned()
        .map(|path| (path.clone(), HashSet::from([path])))
        .collect();
    let mut revwalk = match repo.revwalk() {
        Ok(revwalk) => revwalk,
        Err(_) => return (HashMap::new(), stats),
    };
    if revwalk.push(head.id()).is_err() {
        return (HashMap::new(), stats);
    }
    // Rename aliases must be discovered from children before their parents,
    // even when commit timestamps are skewed or identical.
    let _ = revwalk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME);

    let mut walked = 0usize;
    let mut complete = true;
    let mut oids = Vec::new();
    for oid in revwalk {
        if max_commits > 0 && walked >= max_commits {
            break;
        }
        let oid = match oid {
            Ok(oid) => oid,
            Err(_) => {
                complete = false;
                continue;
            }
        };
        walked += 1;

        oids.push(oid);
    }

    let uncached = oids
        .iter()
        .copied()
        .filter(|oid| cache.event(&oid.to_string()).is_none())
        .collect::<Vec<_>>();
    let mut streamed_events = if uncached.is_empty() {
        HashMap::new()
    } else {
        match native_git.collect_events(&repo, &uncached) {
            Ok(events) => {
                stats.native_batches += usize::from(!events.is_empty());
                stats.native_events += events.len();
                events
            }
            Err(_) => {
                stats.native_fallbacks += 1;
                HashMap::new()
            }
        }
    };

    for oid in oids {
        let oid_string = oid.to_string();
        let mut event = if let Some(event) = cache.event(&oid_string).cloned() {
            stats.event_hits += 1;
            event
        } else if let Some(event) = streamed_events.remove(&oid_string) {
            cache.put_event(event.clone());
            event
        } else {
            let Some(event) = analyze_commit(&repo, oid, &aliases, &mut stats) else {
                complete = false;
                continue;
            };
            cache.put_event(event.clone());
            event
        };

        if !event.renames_resolved
            && event_needs_rename_resolution(&event.deltas, &aliases)
            && let Some(resolved) = resolve_commit_renames(&repo, oid, &mut stats)
        {
            cache.put_event(resolved.clone());
            event = resolved;
        }
        apply_event(&event, &mut aliases, &mut acc);
    }

    let churn = finish_churn(acc);
    if complete {
        cache.put_view(view_identity, &churn);
    }
    cache.save();
    (churn, stats)
}

fn analyze_commit(
    repo: &Repository,
    oid: git2::Oid,
    aliases: &HashMap<PathBuf, HashSet<PathBuf>>,
    stats: &mut CollectStats,
) -> Option<CommitEvent> {
    let commit = repo.find_commit(oid).ok()?;
    let new_tree = commit.tree().ok()?;
    let old_tree = if commit.parent_count() == 0 {
        None
    } else {
        Some(commit.parent(0).and_then(|parent| parent.tree()).ok()?)
    };

    let mut opts = DiffOptions::new();
    opts.context_lines(0).skip_binary_check(true);
    let mut diff = repo
        .diff_tree_to_tree(old_tree.as_ref(), Some(&new_tree), Some(&mut opts))
        .ok()?;
    stats.tree_diffs += 1;
    let mut deltas = cached_deltas(&diff);
    let mut renames_resolved = false;
    if event_needs_rename_resolution(&deltas, aliases) {
        stats.rename_probes += 1;
        let mut find = DiffFindOptions::new();
        find.renames(true);
        if diff.find_similar(Some(&mut find)).is_ok() {
            deltas = cached_deltas(&diff);
            renames_resolved = true;
        }
    }

    Some(commit_event(&commit, oid, deltas, renames_resolved))
}

fn resolve_commit_renames(
    repo: &Repository,
    oid: git2::Oid,
    stats: &mut CollectStats,
) -> Option<CommitEvent> {
    let commit = repo.find_commit(oid).ok()?;
    let new_tree = commit.tree().ok()?;
    let old_tree = if commit.parent_count() == 0 {
        None
    } else {
        Some(commit.parent(0).and_then(|parent| parent.tree()).ok()?)
    };
    let mut opts = DiffOptions::new();
    opts.context_lines(0);
    let mut diff = repo
        .diff_tree_to_tree(old_tree.as_ref(), Some(&new_tree), Some(&mut opts))
        .ok()?;
    stats.tree_diffs += 1;
    stats.rename_probes += 1;
    let mut find = DiffFindOptions::new();
    find.renames(true);
    diff.find_similar(Some(&mut find)).ok()?;
    Some(commit_event(&commit, oid, cached_deltas(&diff), true))
}

fn commit_event(
    commit: &git2::Commit<'_>,
    oid: git2::Oid,
    deltas: Vec<CachedDelta>,
    renames_resolved: bool,
) -> CommitEvent {
    CommitEvent {
        oid: oid.to_string(),
        author: commit.author().email().ok().map(str::to_owned),
        seconds: commit.time().seconds(),
        deltas,
        renames_resolved,
    }
}

fn cached_deltas(diff: &Diff<'_>) -> Vec<CachedDelta> {
    diff.deltas()
        .map(|delta| CachedDelta {
            kind: match delta.status() {
                Delta::Added => DeltaKind::Added,
                Delta::Deleted => DeltaKind::Deleted,
                Delta::Renamed => DeltaKind::Renamed,
                _ => DeltaKind::Other,
            },
            old_path: delta.old_file().path().map(Path::to_path_buf),
            new_path: delta.new_file().path().map(Path::to_path_buf),
        })
        .collect()
}

fn event_needs_rename_resolution(
    deltas: &[CachedDelta],
    aliases: &HashMap<PathBuf, HashSet<PathBuf>>,
) -> bool {
    let has_deletion = deltas.iter().any(|delta| delta.kind == DeltaKind::Deleted);
    has_deletion
        && deltas.iter().any(|delta| {
            delta.kind == DeltaKind::Added
                && delta
                    .new_path
                    .as_ref()
                    .is_some_and(|path| aliases.contains_key(path))
        })
}

fn apply_event(
    event: &CommitEvent,
    aliases: &mut HashMap<PathBuf, HashSet<PathBuf>>,
    acc: &mut HashMap<PathBuf, Acc>,
) {
    let mut touched = HashSet::new();
    for delta in &event.deltas {
        if let Some(paths) = delta.new_path.as_ref().and_then(|path| aliases.get(path)) {
            touched.extend(paths.iter().cloned());
        }
        if let Some(paths) = delta.old_path.as_ref().and_then(|path| aliases.get(path)) {
            touched.extend(paths.iter().cloned());
        }
    }
    for path in touched {
        let entry = acc.entry(path).or_default();
        entry.commits += 1;
        if let Some(author) = &event.author {
            entry.authors.insert(author.clone());
        }
        entry.first = Some(
            entry
                .first
                .map_or(event.seconds, |first| first.min(event.seconds)),
        );
        entry.last = Some(
            entry
                .last
                .map_or(event.seconds, |last| last.max(event.seconds)),
        );
    }
    for delta in &event.deltas {
        if delta.kind != DeltaKind::Renamed {
            continue;
        }
        let (Some(old_path), Some(new_path)) = (&delta.old_path, &delta.new_path) else {
            continue;
        };
        let Some(current_paths) = aliases.remove(new_path) else {
            continue;
        };
        aliases
            .entry(old_path.clone())
            .or_default()
            .extend(current_paths);
    }
}

fn finish_churn(acc: HashMap<PathBuf, Acc>) -> HashMap<PathBuf, Churn> {
    acc.into_iter()
        .map(|(path, acc)| {
            (
                path,
                Churn {
                    commits: acc.commits,
                    authors: acc.authors.len(),
                    first_commit: acc.first.and_then(seconds_to_rfc3339),
                    last_commit: acc.last.and_then(seconds_to_rfc3339),
                },
            )
        })
        .collect()
}

fn history_state(repo: &Repository) -> String {
    let shallow = std::fs::read(repo.path().join("shallow")).unwrap_or_default();
    let grafts = std::fs::read(repo.path().join("info/grafts")).unwrap_or_default();
    let mut state = Vec::with_capacity(shallow.len() + grafts.len() + 1);
    state.extend_from_slice(&shallow);
    state.push(0);
    state.extend_from_slice(&grafts);
    format!("history:{:016x}", xxh3_64(&state))
}

fn seconds_to_rfc3339(seconds: i64) -> Option<String> {
    DateTime::from_timestamp(seconds, 0).map(|dt| dt.to_rfc3339())
}

/// Selects which diff to use when restricting the scan to changed files.
#[derive(Debug, Clone)]
pub enum DiffScope {
    /// Files changed between the given git ref's tree and the working tree
    /// (including staged and untracked files).
    Since(String),
    /// Files staged in the index vs HEAD.
    Staged,
    /// All uncommitted working-tree changes (staged + unstaged + untracked) vs HEAD.
    Working,
}

/// Resolve the exact Git tree used as the base of a diff-scoped scan.
pub fn diff_base_tree_id(root: &Path, scope: &DiffScope) -> Result<Option<String>> {
    let repo = Repository::discover(root)
        .map_err(|e| anyhow::anyhow!("diff-scoped scan requires a git repository: {e}"))?;
    let tree = match scope {
        DiffScope::Since(reference) => Some(repo.revparse_single(reference)?.peel_to_tree()?),
        DiffScope::Staged | DiffScope::Working => match repo.head() {
            Ok(head) => Some(head.peel_to_tree()?),
            Err(error)
                if matches!(
                    error.code(),
                    git2::ErrorCode::UnbornBranch | git2::ErrorCode::NotFound
                ) =>
            {
                None
            }
            Err(error) => return Err(error.into()),
        },
    };
    Ok(tree.map(|tree| tree.id().to_string()))
}

/// Returns repo-relative paths of files touched by the given diff scope.
/// Errors if `root` is not inside a git repository.
pub fn changed_files(root: &Path, scope: &DiffScope) -> Result<HashSet<PathBuf>> {
    changed_files_with_base(root, scope, None)
}

pub(crate) fn changed_files_with_base(
    root: &Path,
    scope: &DiffScope,
    base_tree_id: Option<&str>,
) -> Result<HashSet<PathBuf>> {
    let repo = Repository::discover(root)
        .map_err(|e| anyhow::anyhow!("diff-scoped scan requires a git repository: {e}"))?;
    let mut opts = DiffOptions::new();
    opts.include_untracked(true).recurse_untracked_dirs(true);
    let base_tree = resolved_base_tree(&repo, scope, base_tree_id)?;
    let diff = match scope {
        DiffScope::Since(_) | DiffScope::Working => {
            repo.diff_tree_to_workdir_with_index(base_tree.as_ref(), Some(&mut opts))?
        }
        DiffScope::Staged => repo.diff_tree_to_index(base_tree.as_ref(), None, Some(&mut opts))?,
    };
    let mut paths = HashSet::new();
    for delta in diff.deltas() {
        if let Some(p) = delta.new_file().path().or_else(|| delta.old_file().path()) {
            paths.insert(p.to_path_buf());
        }
    }
    Ok(paths)
}

/// Return changed files with precise old/new physical-line ranges.
pub fn changed_lines(root: &Path, scope: &DiffScope) -> Result<Vec<ReviewChangedFile>> {
    changed_lines_with_base(root, scope, None)
}

pub(crate) fn changed_lines_with_base(
    root: &Path,
    scope: &DiffScope,
    base_tree_id: Option<&str>,
) -> Result<Vec<ReviewChangedFile>> {
    let repo = Repository::discover(root)
        .map_err(|e| anyhow::anyhow!("diff review requires a git repository: {e}"))?;
    let mut opts = DiffOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(true)
        .show_untracked_content(true)
        .context_lines(0);
    let mut diff = diff_for_scope(&repo, scope, base_tree_id, &mut opts)?;
    let mut find = DiffFindOptions::new();
    find.renames(true).for_untracked(true);
    diff.find_similar(Some(&mut find))?;

    let mut files = Vec::new();
    let mut indices = HashMap::new();
    for delta in diff.deltas() {
        let (old_path, path) = review_paths(&delta);
        let key = (old_path.clone(), path.clone());
        indices.insert(key, files.len());
        files.push(ReviewChangedFile {
            old_path,
            path,
            status: delta_status(delta.status()).to_string(),
            old_ranges: Vec::new(),
            ranges: Vec::new(),
            binary: false,
        });
    }

    diff.print(DiffFormat::Patch, |delta, _, line| {
        let key = review_paths(&delta);
        let Some(index) = indices.get(&key).copied() else {
            return true;
        };
        match line.origin_value() {
            DiffLineType::Addition | DiffLineType::AddEOFNL => {
                if let Some(line) = line.new_lineno() {
                    push_line(&mut files[index].ranges, line as usize);
                }
            }
            DiffLineType::Deletion | DiffLineType::DeleteEOFNL => {
                if let Some(line) = line.old_lineno() {
                    push_line(&mut files[index].old_ranges, line as usize);
                }
            }
            DiffLineType::Binary => files[index].binary = true,
            _ => {}
        }
        true
    })?;

    files.sort_by(|a, b| {
        a.path
            .as_ref()
            .or(a.old_path.as_ref())
            .cmp(&b.path.as_ref().or(b.old_path.as_ref()))
    });
    Ok(files)
}

fn diff_for_scope<'repo>(
    repo: &'repo Repository,
    scope: &DiffScope,
    base_tree_id: Option<&str>,
    opts: &mut DiffOptions,
) -> Result<Diff<'repo>> {
    let base_tree = resolved_base_tree(repo, scope, base_tree_id)?;
    Ok(match scope {
        DiffScope::Since(_) | DiffScope::Working => {
            repo.diff_tree_to_workdir_with_index(base_tree.as_ref(), Some(opts))?
        }
        DiffScope::Staged => repo.diff_tree_to_index(base_tree.as_ref(), None, Some(opts))?,
    })
}

fn resolved_base_tree<'repo>(
    repo: &'repo Repository,
    scope: &DiffScope,
    base_tree_id: Option<&str>,
) -> Result<Option<git2::Tree<'repo>>> {
    if let Some(id) = base_tree_id {
        return Ok(Some(repo.find_tree(git2::Oid::from_str(id)?)?));
    }
    match scope {
        DiffScope::Since(reference) => Ok(Some(repo.revparse_single(reference)?.peel_to_tree()?)),
        DiffScope::Staged | DiffScope::Working => match repo.head() {
            Ok(head) => Ok(Some(head.peel_to_tree()?)),
            Err(error)
                if matches!(
                    error.code(),
                    git2::ErrorCode::UnbornBranch | git2::ErrorCode::NotFound
                ) =>
            {
                Ok(None)
            }
            Err(error) => Err(error.into()),
        },
    }
}

fn review_paths(delta: &DiffDelta<'_>) -> (Option<PathBuf>, Option<PathBuf>) {
    let old = delta.old_file().path().map(Path::to_path_buf);
    let new = delta.new_file().path().map(Path::to_path_buf);
    match delta.status() {
        Delta::Added | Delta::Untracked => (None, new.or(old)),
        Delta::Deleted => (old.or(new), None),
        _ => (old, new),
    }
}

fn delta_status(status: Delta) -> &'static str {
    match status {
        Delta::Added | Delta::Untracked => "added",
        Delta::Deleted => "deleted",
        Delta::Renamed => "renamed",
        Delta::Copied => "copied",
        Delta::Typechange => "typechange",
        Delta::Conflicted => "conflicted",
        _ => "modified",
    }
}

fn push_line(ranges: &mut Vec<LineRange>, line: usize) {
    if let Some(last) = ranges.last_mut()
        && line <= last.end.saturating_add(1)
    {
        last.end = last.end.max(line);
        return;
    }
    ranges.push(LineRange {
        start: line,
        end: line,
    });
}

#[cfg(test)]
mod tests {
    use super::churn_cache::ChurnCache;
    use super::native_churn::NativeGit;
    use super::{
        DiffScope, changed_files_with_base, collect, collect_impl, collect_impl_with_native,
        diff_base_tree_id,
    };
    use git2::Repository;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn commit_all(repo: &Repository, message: &str) {
        let mut index = repo.index().unwrap();
        index
            .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let signature = git2::Signature::now("reposcout tests", "tests@example.com").unwrap();
        let parent = repo.head().ok().and_then(|head| head.peel_to_commit().ok());
        let parents = parent.iter().collect::<Vec<_>>();
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            message,
            &tree,
            &parents,
        )
        .unwrap();
    }

    #[test]
    fn collect_git_churn_for_repo_files() {
        if Repository::discover(".").is_err() {
            return;
        }

        let files = [
            PathBuf::from("src/main.rs"),
            PathBuf::from("src/git.rs"),
            PathBuf::from("does/not/exist.rs"),
        ];
        let churn = collect(Path::new("."), &files, 0);

        let main = churn
            .get(Path::new("src/main.rs"))
            .expect("src/main.rs should be present in git history");
        assert!(main.commits >= 1);
        assert!(main.authors >= 1);
        assert!(main.last_commit.is_some());
        assert!(!churn.contains_key(Path::new("does/not/exist.rs")));
    }

    #[test]
    fn churn_follows_file_history_across_renames() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let old = dir.path().join("old.rs");
        let new = dir.path().join("new.rs");
        fs::write(&old, "fn value() -> i32 { 1 }\n").unwrap();
        commit_all(&repo, "add old");
        fs::write(&old, "fn value() -> i32 { 2 }\n").unwrap();
        commit_all(&repo, "modify old");
        fs::rename(&old, &new).unwrap();
        commit_all(&repo, "rename old to new");

        let churn = collect(dir.path(), &[PathBuf::from("new.rs")], 0);
        assert_eq!(churn[Path::new("new.rs")].commits, 3);
    }

    #[test]
    fn native_git_stream_matches_libgit2_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let old = dir.path().join("old.rs");
        let new = dir.path().join("new.rs");
        let other = dir.path().join("other.rs");
        fs::write(&old, "fn value() -> i32 { 1 }\n").unwrap();
        fs::write(&other, "fn other() {}\n").unwrap();
        commit_all(&repo, "add files");
        fs::write(&old, "fn value() -> i32 { 2 }\n").unwrap();
        commit_all(&repo, "modify old");
        fs::rename(&old, &new).unwrap();
        commit_all(&repo, "rename old to new");
        let wanted = [PathBuf::from("new.rs"), PathBuf::from("other.rs")];

        let mut native_cache = ChurnCache::for_repo(dir.path(), false);
        let (native, native_stats) = collect_impl(dir.path(), &wanted, 0, &mut native_cache);
        let mut fallback_cache = ChurnCache::for_repo(dir.path(), false);
        let (fallback, fallback_stats) = collect_impl_with_native(
            dir.path(),
            &wanted,
            0,
            &mut fallback_cache,
            &NativeGit::with_executable(dir.path().join("missing-git")),
        );

        assert_eq!(
            serde_json::to_value(native).unwrap(),
            serde_json::to_value(fallback).unwrap()
        );
        assert_eq!(native_stats.native_batches, 1);
        assert_eq!(native_stats.native_events, 2);
        assert_eq!(native_stats.native_fallbacks, 0);
        assert_eq!(fallback_stats.native_fallbacks, 1);
        assert_eq!(fallback_stats.tree_diffs, 3);
    }

    #[test]
    fn churn_only_probes_renames_when_a_tracked_addition_can_be_one() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let source = dir.path().join("lib.rs");
        fs::write(&source, "fn value() -> i32 { 1 }\n").unwrap();
        commit_all(&repo, "add");
        fs::write(&source, "fn value() -> i32 { 2 }\n").unwrap();
        commit_all(&repo, "modify once");
        fs::write(&source, "fn value() -> i32 { 3 }\n").unwrap();
        commit_all(&repo, "modify twice");

        let mut cache = ChurnCache::for_repo(dir.path(), false);
        let missing_git = dir.path().join("missing-git");
        let (churn, stats) = collect_impl_with_native(
            dir.path(),
            &[PathBuf::from("lib.rs")],
            0,
            &mut cache,
            &NativeGit::with_executable(missing_git),
        );

        assert_eq!(churn[Path::new("lib.rs")].commits, 3);
        assert_eq!(stats.tree_diffs, 3);
        assert_eq!(stats.rename_probes, 0);
        assert_eq!(stats.native_fallbacks, 1);
    }

    #[test]
    fn churn_cache_reuses_views_and_only_diffs_new_commits() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let old = dir.path().join("old.rs");
        let new = dir.path().join("new.rs");
        fs::write(&old, "fn value() -> i32 { 1 }\n").unwrap();
        commit_all(&repo, "add old");
        fs::write(&old, "fn value() -> i32 { 2 }\n").unwrap();
        commit_all(&repo, "modify old");
        fs::rename(&old, &new).unwrap();
        commit_all(&repo, "rename old to new");
        let wanted = [PathBuf::from("new.rs")];

        let mut cold_cache = ChurnCache::for_test(cache_dir.path());
        let (cold, cold_stats) = collect_impl(dir.path(), &wanted, 0, &mut cold_cache);
        assert_eq!(cold[Path::new("new.rs")].commits, 3);
        assert_eq!(cold_stats.tree_diffs, 2);
        assert_eq!(cold_stats.rename_probes, 1);
        assert_eq!(cold_stats.native_batches, 1);
        assert_eq!(cold_stats.native_events, 2);
        assert_eq!(cold_stats.native_fallbacks, 0);
        assert_eq!(cold_stats.event_hits, 0);
        assert_eq!(cold_stats.view_hits, 0);

        let mut warm_cache = ChurnCache::for_test(cache_dir.path());
        let (warm, warm_stats) = collect_impl(dir.path(), &wanted, 0, &mut warm_cache);
        assert_eq!(warm[Path::new("new.rs")].commits, 3);
        assert_eq!(warm_stats.tree_diffs, 0);
        assert_eq!(warm_stats.view_hits, 1);

        fs::write(&new, "fn value() -> i32 { 3 }\n").unwrap();
        commit_all(&repo, "modify new");
        let mut advanced_cache = ChurnCache::for_test(cache_dir.path());
        let (advanced, advanced_stats) = collect_impl(dir.path(), &wanted, 0, &mut advanced_cache);
        assert_eq!(advanced[Path::new("new.rs")].commits, 4);
        assert_eq!(advanced_stats.tree_diffs, 0);
        assert_eq!(advanced_stats.native_batches, 1);
        assert_eq!(advanced_stats.native_events, 1);
        assert_eq!(advanced_stats.event_hits, 3);
        assert_eq!(advanced_stats.rename_probes, 0);
        assert_eq!(advanced_stats.view_hits, 0);

        let mut capped_cache = ChurnCache::for_test(cache_dir.path());
        let (capped, capped_stats) = collect_impl(dir.path(), &wanted, 2, &mut capped_cache);
        assert_eq!(capped[Path::new("new.rs")].commits, 2);
        assert_eq!(capped_stats.tree_diffs, 0);
        assert_eq!(capped_stats.event_hits, 2);
        assert_eq!(capped_stats.view_hits, 0);
    }

    #[test]
    fn diff_base_uses_resolved_tree_identity() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        fs::write(dir.path().join("lib.rs"), "fn value() {}\n").unwrap();
        commit_all(&repo, "initial");

        let head = diff_base_tree_id(dir.path(), &DiffScope::Since("HEAD".to_string())).unwrap();
        let working = diff_base_tree_id(dir.path(), &DiffScope::Working).unwrap();
        assert_eq!(head, working);
        assert!(head.is_some());
    }

    #[test]
    fn changed_files_can_reuse_a_previously_resolved_base_tree() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        let source = dir.path().join("lib.rs");
        fs::write(&source, "fn value() -> u8 { 1 }\n").unwrap();
        commit_all(&repo, "first");
        let base = diff_base_tree_id(dir.path(), &DiffScope::Working)
            .unwrap()
            .unwrap();
        fs::write(&source, "fn value() -> u8 { 2 }\n").unwrap();
        commit_all(&repo, "second");

        let changed =
            changed_files_with_base(dir.path(), &DiffScope::Working, Some(&base)).unwrap();
        assert!(changed.contains(Path::new("lib.rs")));
    }
}
