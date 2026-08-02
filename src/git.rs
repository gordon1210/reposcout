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

/// Resource limits for Git churn collection.
#[derive(Debug, Clone)]
pub struct ChurnLimits {
    pub max_commits: usize,
    pub max_deltas_per_commit: usize,
    pub max_total_deltas: usize,
    pub max_output_bytes: u64,
    pub max_path_bytes: usize,
    pub max_cache_bytes: u64,
    pub deadline: Option<std::time::Instant>,
    /// When true, a native-Git failure does not fall back to libgit2 materialization
    /// of unbounded commit diffs (safe profile).
    pub skip_libgit2_fallback: bool,
}

impl Default for ChurnLimits {
    fn default() -> Self {
        Self {
            max_commits: 5_000,
            max_deltas_per_commit: 50_000,
            max_total_deltas: 500_000,
            max_output_bytes: 64 * 1024 * 1024,
            max_path_bytes: 4_096,
            max_cache_bytes: 64 * 1024 * 1024,
            deadline: None,
            skip_libgit2_fallback: false,
        }
    }
}

impl ChurnLimits {
    #[must_use]
    pub fn with_max_commits(max_commits: usize) -> Self {
        Self {
            max_commits,
            ..Self::default()
        }
    }
}

/// Result of a churn walk, including partial-limit diagnostics.
#[derive(Debug, Default)]
pub struct ChurnCollection {
    pub churn: HashMap<PathBuf, Churn>,
    pub partial: bool,
    pub deltas_omitted: usize,
}

#[must_use]
pub fn collect(root: &Path, files: &[PathBuf], max_commits: usize) -> HashMap<PathBuf, Churn> {
    let limits = ChurnLimits::with_max_commits(max_commits);
    let mut cache = ChurnCache::for_repo(root, false, limits.max_cache_bytes);
    collect_impl(root, files, &limits, &mut cache).churn
}

/// Collect churn while reusing immutable commit events and exact result views
/// from the OS cache. The frozen [`collect`] adapter remains uncached for
/// library callers and deterministic tests.
#[must_use]
pub fn collect_with_cache(
    root: &Path,
    files: &[PathBuf],
    limits: &ChurnLimits,
    use_cache: bool,
) -> HashMap<PathBuf, Churn> {
    let mut cache = ChurnCache::for_repo(root, use_cache, limits.max_cache_bytes);
    collect_impl(root, files, limits, &mut cache).churn
}

pub(crate) fn collect_with_diagnostics(
    root: &Path,
    files: &[PathBuf],
    limits: &ChurnLimits,
    use_cache: bool,
) -> ChurnCollection {
    let mut cache = ChurnCache::for_repo(root, use_cache, limits.max_cache_bytes);
    collect_impl(root, files, limits, &mut cache)
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
    partial: bool,
    deltas_omitted: usize,
    total_deltas: usize,
}

fn collect_impl(
    root: &Path,
    files: &[PathBuf],
    limits: &ChurnLimits,
    cache: &mut ChurnCache,
) -> ChurnCollection {
    collect_impl_with_native(root, files, limits, cache, &NativeGit::default()).0
}

fn collect_impl_with_native(
    root: &Path,
    files: &[PathBuf],
    limits: &ChurnLimits,
    cache: &mut ChurnCache,
    native_git: &NativeGit,
) -> (ChurnCollection, CollectStats) {
    let mut stats = CollectStats::default();
    let Ok(repo) = Repository::discover(root) else {
        return (ChurnCollection::default(), stats);
    };

    let wanted: HashSet<PathBuf> = files.iter().cloned().collect();
    if wanted.is_empty() {
        return (ChurnCollection::default(), stats);
    }

    let Ok(head) = repo.head().and_then(|head| head.peel_to_commit()) else {
        return (ChurnCollection::default(), stats);
    };
    let history = history_fingerprint(&repo);
    // Oversized .git/shallow or grafts can blow past resource caps; do not load
    // or persist cache entries for that run.
    let use_event_cache = history.cacheable;
    let view_identity = ViewIdentity::new(
        head.id().to_string(),
        history.state.clone(),
        limits,
        wanted.iter().cloned(),
    );
    if use_event_cache && let Some(churn) = cache.get_view(&view_identity) {
        stats.view_hits += 1;
        cache.save();
        return (
            ChurnCollection {
                churn,
                partial: false,
                deltas_omitted: 0,
            },
            stats,
        );
    }
    if use_event_cache {
        cache.load_events(&history.state);
    }

    let Some((oids, mut complete)) = walk_history_oids(&repo, head.id(), limits, &mut stats) else {
        return (ChurnCollection::default(), stats);
    };
    let merge_oids = merge_commit_oids(&repo, &oids);
    let uncached = uncached_event_oids(&oids, &merge_oids, cache);
    let streamed_events = collect_streamed_events(
        &repo,
        &uncached,
        limits,
        native_git,
        &mut stats,
        &mut complete,
    );
    let aliases = wanted
        .iter()
        .cloned()
        .map(|path| (path.clone(), HashSet::from([path])))
        .collect();
    let mut processor = HistoryProcessor {
        repo: &repo,
        limits,
        cache,
        use_event_cache,
        merge_oids: &merge_oids,
        streamed_events,
        aliases,
        acc: HashMap::new(),
        stats: &mut stats,
        complete,
    };
    processor.process(oids);
    let (acc, complete) = processor.finish();
    let churn = finish_churn(acc);
    if use_event_cache && complete && !stats.partial {
        cache.put_view(view_identity, &churn);
    }
    if use_event_cache {
        cache.save();
    }
    (
        ChurnCollection {
            churn,
            partial: stats.partial || !complete,
            deltas_omitted: stats.deltas_omitted,
        },
        stats,
    )
}

fn walk_history_oids(
    repo: &Repository,
    head: git2::Oid,
    limits: &ChurnLimits,
    stats: &mut CollectStats,
) -> Option<(Vec<git2::Oid>, bool)> {
    let mut revwalk = repo.revwalk().ok()?;
    revwalk.push(head).ok()?;
    // Rename aliases must be discovered from children before their parents,
    // even when commit timestamps are skewed or identical.
    let _ = revwalk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME);
    let mut oids = Vec::new();
    let mut complete = true;
    for oid in revwalk {
        if limits
            .deadline
            .is_some_and(|deadline| std::time::Instant::now() >= deadline)
        {
            complete = false;
            stats.partial = true;
            break;
        }
        if limits.max_commits > 0 && oids.len() >= limits.max_commits {
            break;
        }
        match oid {
            Ok(oid) => oids.push(oid),
            Err(_) => complete = false,
        }
    }
    Some((oids, complete))
}

fn merge_commit_oids(repo: &Repository, oids: &[git2::Oid]) -> HashSet<git2::Oid> {
    oids.iter()
        .copied()
        .filter(|oid| {
            repo.find_commit(*oid)
                .is_ok_and(|commit| commit.parent_count() > 1)
        })
        .collect()
}

fn uncached_event_oids(
    oids: &[git2::Oid],
    merge_oids: &HashSet<git2::Oid>,
    cache: &ChurnCache,
) -> Vec<git2::Oid> {
    oids.iter()
        .copied()
        .filter(|oid| !merge_oids.contains(oid) && cache.event(&oid.to_string()).is_none())
        .collect()
}

fn collect_streamed_events(
    repo: &Repository,
    uncached: &[git2::Oid],
    limits: &ChurnLimits,
    native_git: &NativeGit,
    stats: &mut CollectStats,
    complete: &mut bool,
) -> HashMap<String, CommitEvent> {
    if uncached.is_empty() {
        return HashMap::new();
    }
    let Ok((events, stream_stats)) = native_git.collect_events(repo, uncached, limits) else {
        stats.native_fallbacks = stats.native_fallbacks.saturating_add(1);
        if limits.skip_libgit2_fallback {
            stats.partial = true;
            *complete = false;
        }
        return HashMap::new();
    };
    stats.native_batches = stats
        .native_batches
        .saturating_add(usize::from(!events.is_empty()));
    stats.native_events = stats.native_events.saturating_add(events.len());
    stats.partial |= stream_stats.partial;
    stats.deltas_omitted = stats
        .deltas_omitted
        .saturating_add(stream_stats.deltas_omitted);
    if stream_stats.partial {
        *complete = false;
    }
    events
}

struct HistoryProcessor<'a> {
    repo: &'a Repository,
    limits: &'a ChurnLimits,
    cache: &'a mut ChurnCache,
    use_event_cache: bool,
    merge_oids: &'a HashSet<git2::Oid>,
    streamed_events: HashMap<String, CommitEvent>,
    aliases: HashMap<PathBuf, HashSet<PathBuf>>,
    acc: HashMap<PathBuf, Acc>,
    stats: &'a mut CollectStats,
    complete: bool,
}

impl HistoryProcessor<'_> {
    fn process(&mut self, oids: Vec<git2::Oid>) {
        for oid in oids {
            if self.limit_reached() {
                break;
            }
            let Some(mut event) = self.event_for(oid) else {
                continue;
            };
            self.resolve_renames(oid, &mut event);
            if !enforce_event_limits(&mut event, self.limits, self.stats) {
                self.complete = false;
            }
            if self.use_event_cache && !self.stats.partial {
                self.cache.put_event(event.clone());
            }
            apply_event(&event, &mut self.aliases, &mut self.acc);
            if self.stats.partial && self.stats.total_deltas >= self.limits.max_total_deltas {
                break;
            }
        }
    }

    fn limit_reached(&mut self) -> bool {
        let deadline_reached = self
            .limits
            .deadline
            .is_some_and(|deadline| std::time::Instant::now() >= deadline);
        let delta_limit_reached = self.stats.total_deltas >= self.limits.max_total_deltas;
        if deadline_reached || delta_limit_reached {
            self.complete = false;
            self.stats.partial = true;
            true
        } else {
            false
        }
    }

    fn event_for(&mut self, oid: git2::Oid) -> Option<CommitEvent> {
        let oid_string = oid.to_string();
        if self.merge_oids.contains(&oid) {
            let Some(commit) = self.repo.find_commit(oid).ok() else {
                self.complete = false;
                return None;
            };
            // The branch commit already carries the authored change. Counting
            // the merge's first-parent diff as another touch inflates churn.
            return Some(commit_event(&commit, oid, Vec::new(), true));
        }
        if let Some(event) = self.cache.event(&oid_string).cloned() {
            self.stats.event_hits = self.stats.event_hits.saturating_add(1);
            return Some(event);
        }
        if let Some(event) = self.streamed_events.remove(&oid_string) {
            return Some(event);
        }
        if self.limits.skip_libgit2_fallback {
            // Avoid materializing unbounded libgit2 diffs under the safe profile.
            self.complete = false;
            self.stats.partial = true;
            return None;
        }
        let event = analyze_commit(self.repo, oid, &self.aliases, self.limits, self.stats);
        if event.is_none() {
            self.complete = false;
        }
        event
    }

    fn resolve_renames(&mut self, oid: git2::Oid, event: &mut CommitEvent) {
        let needed =
            !event.renames_resolved && event_needs_rename_resolution(&event.deltas, &self.aliases);
        if !needed {
            return;
        }
        // Rename similarity via libgit2 materializes a full tree-diff and is
        // unbounded. Never run it under the safe profile or after truncation.
        if self.limits.skip_libgit2_fallback || self.stats.partial {
            self.complete = false;
            self.stats.partial = true;
        } else if let Some(resolved) =
            resolve_commit_renames(self.repo, oid, self.limits, self.stats)
        {
            *event = resolved;
        }
    }

    fn finish(self) -> (HashMap<PathBuf, Acc>, bool) {
        (self.acc, self.complete)
    }
}

fn analyze_commit(
    repo: &Repository,
    oid: git2::Oid,
    aliases: &HashMap<PathBuf, HashSet<PathBuf>>,
    limits: &ChurnLimits,
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
    let mut deltas = cached_deltas(&diff, limits, stats);
    let mut renames_resolved = false;
    if !stats.partial
        && event_needs_rename_resolution(&deltas, aliases)
        && deltas.len() <= limits.max_deltas_per_commit
    {
        stats.rename_probes += 1;
        let mut find = DiffFindOptions::new();
        find.renames(true);
        if diff.find_similar(Some(&mut find)).is_ok() {
            deltas = cached_deltas(&diff, limits, stats);
            renames_resolved = true;
        }
    }

    Some(commit_event(&commit, oid, deltas, renames_resolved))
}

fn resolve_commit_renames(
    repo: &Repository,
    oid: git2::Oid,
    limits: &ChurnLimits,
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
    Some(commit_event(
        &commit,
        oid,
        cached_deltas(&diff, limits, stats),
        true,
    ))
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

fn cached_deltas(
    diff: &Diff<'_>,
    limits: &ChurnLimits,
    stats: &mut CollectStats,
) -> Vec<CachedDelta> {
    let mut deltas = Vec::new();
    for delta in diff.deltas() {
        if deltas.len() >= limits.max_deltas_per_commit {
            stats.partial = true;
            stats.deltas_omitted = stats.deltas_omitted.saturating_add(1);
            continue;
        }
        let old_path = delta.old_file().path().map(Path::to_path_buf);
        let new_path = delta.new_file().path().map(Path::to_path_buf);
        if path_too_long(old_path.as_deref(), limits.max_path_bytes)
            || path_too_long(new_path.as_deref(), limits.max_path_bytes)
        {
            stats.partial = true;
            stats.deltas_omitted = stats.deltas_omitted.saturating_add(1);
            continue;
        }
        deltas.push(CachedDelta {
            kind: match delta.status() {
                Delta::Added => DeltaKind::Added,
                Delta::Deleted => DeltaKind::Deleted,
                Delta::Renamed => DeltaKind::Renamed,
                _ => DeltaKind::Other,
            },
            old_path,
            new_path,
        });
    }
    deltas
}

fn path_too_long(path: Option<&Path>, max_path_bytes: usize) -> bool {
    path.is_some_and(|path| path.as_os_str().len() > max_path_bytes)
}

/// Apply path, per-commit, and global delta limits to one final commit event.
/// Returns false when the event was truncated (scan is incomplete).
fn enforce_event_limits(
    event: &mut CommitEvent,
    limits: &ChurnLimits,
    stats: &mut CollectStats,
) -> bool {
    let mut complete = true;

    let before_paths = event.deltas.len();
    event.deltas.retain(|delta| {
        !path_too_long(delta.old_path.as_deref(), limits.max_path_bytes)
            && !path_too_long(delta.new_path.as_deref(), limits.max_path_bytes)
    });
    let removed_paths = before_paths.saturating_sub(event.deltas.len());
    if removed_paths > 0 {
        stats.deltas_omitted = stats.deltas_omitted.saturating_add(removed_paths);
        stats.partial = true;
        complete = false;
    }

    if event.deltas.len() > limits.max_deltas_per_commit {
        stats.deltas_omitted = stats.deltas_omitted.saturating_add(
            event
                .deltas
                .len()
                .saturating_sub(limits.max_deltas_per_commit),
        );
        event.deltas.truncate(limits.max_deltas_per_commit);
        stats.partial = true;
        complete = false;
    }

    if stats.total_deltas.saturating_add(event.deltas.len()) > limits.max_total_deltas {
        let allowed = limits.max_total_deltas.saturating_sub(stats.total_deltas);
        stats.deltas_omitted = stats
            .deltas_omitted
            .saturating_add(event.deltas.len().saturating_sub(allowed));
        event.deltas.truncate(allowed);
        stats.partial = true;
        complete = false;
    }
    stats.total_deltas = stats.total_deltas.saturating_add(event.deltas.len());
    complete
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

struct HistoryFingerprint {
    state: String,
    /// False when shallow/grafts metadata was oversized or not a regular file.
    cacheable: bool,
}

/// Bound for Git metadata files that feed the churn history fingerprint.
const MAX_HISTORY_META_BYTES: u64 = 1024 * 1024;

fn history_fingerprint(repo: &Repository) -> HistoryFingerprint {
    let shallow = read_git_meta_bounded(&repo.path().join("shallow"));
    let grafts = read_git_meta_bounded(&repo.path().join("info/grafts"));
    let cacheable = shallow.is_some() && grafts.is_some();
    let mut state = Vec::with_capacity(
        shallow.as_ref().map_or(0, Vec::len) + grafts.as_ref().map_or(0, Vec::len) + 16,
    );
    match &shallow {
        Some(bytes) => state.extend_from_slice(bytes),
        None => state.extend_from_slice(b"shallow-omitted"),
    }
    state.push(0);
    match &grafts {
        Some(bytes) => state.extend_from_slice(bytes),
        None => state.extend_from_slice(b"grafts-omitted"),
    }
    HistoryFingerprint {
        state: format!("history:{:016x}", xxh3_64(&state)),
        cacheable,
    }
}

/// Read a Git metadata file with a hard size bound. Missing files yield empty
/// content; oversized or non-regular files yield `None`.
fn read_git_meta_bounded(path: &Path) -> Option<Vec<u8>> {
    match crate::fs_budget::read_bytes_limited(path, MAX_HISTORY_META_BYTES) {
        Ok(bytes) => Some(bytes),
        Err(crate::fs_budget::ReadOutcome::Unreadable) => {
            if path.exists() {
                None
            } else {
                Some(Vec::new())
            }
        }
        Err(_) => None,
    }
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
///
/// # Errors
///
/// Returns an error when `root` is not in a Git repository or the requested
/// reference, `HEAD`, or tree cannot be resolved.
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
///
/// # Errors
///
/// Returns an error when the repository, selected reference, base tree, or
/// requested Git diff cannot be resolved.
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
///
/// # Errors
///
/// Returns an error when the repository, selected reference, base tree, rename
/// detection, or line-level Git diff cannot be resolved.
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
mod tests;
